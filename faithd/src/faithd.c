#define _GNU_SOURCE

#include <openssl/crypto.h>
#include <openssl/err.h>
#include <openssl/rand.h>
#include <openssl/ssl.h>

#include <sys/epoll.h>
#include <sys/socket.h>
#include <sys/types.h>

#include <arpa/inet.h>
#include <netinet/in.h>

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <limits.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define NOB_IMPLEMENTATION
#include "shared.h"

#define STB_DS_IMPLEMENTATION
#include "../third_party/stb_ds.h"

#define PORT       4433
#define MAX_EVENTS 1024

struct server_cfg_t {
  int verbose_logging;
};

#define CLIENT_STATES(X)                                                       \
  X(CLIENT_HANDSHAKE, 0)                                                       \
  X(CLIENT_OPEN, 1)                                                            \
  X(CLIENT_CLOSING, 2)                                                         \
  X(CLIENT_WAIT_FOR_HELLO_ACK, 3)

enum client_state_t {
#define X(name, value) name = value,
  CLIENT_STATES(X)
#undef X
};

struct client_conn_t {
  // connection id
  uint64_t            conn_id;
  uint64_t            client_id;
  uint64_t            device_id;
  int                 fd;
  SSL                *ssl;
  enum client_state_t state;

  uint32_t evs_mask_want;

  uint8_t *out_buf;
  size_t   out_size;
  size_t   out_off;

  uint8_t *in_buf;
  size_t   in_size;
  size_t   in_cap;
  size_t   in_off;

  struct client_conn_t *next;
  struct client_conn_t *prev;

  int authorized;
};

struct client_session_data_t {
  struct client_conn_t *conn;
};

struct client_route_device_t {
  device_id_t                   key; /* device id */
  struct client_session_data_t *value;
};

/* nested hashmap [auth id -> device id -> conn/sess data] */
struct client_route_user_t {
  client_id_t key; /* client/auth id */
  struct client_route_device_t
      *value; /* a hashmap of device id -> client conn/sess data*/
};

struct routing_state_t {
  struct client_route_user_t *active_users;
};

struct server_state_t {
  int      listenfd;
  int      epoll_fd;
  SSL_CTX *ssl_ctx;

  atomic_uint_fast64_t  next_client_id;
  struct client_conn_t *clients;

  struct server_cfg_t    cfg;
  struct routing_state_t rt;
};

enum read_frame_result_t {
  READ_FRAME_OK,
  READ_FRAME_GOT_BYTES,
  READ_FRAME_WANT_READ,
  READ_FRAME_WANT_WRITE,
  READ_FRAME_CLOSED,
  READ_FRAME_ERROR
};

static volatile sig_atomic_t shutdown_requested = 0;

static void handle_shutdown_signal(int sig) {
  (void)sig;
  shutdown_requested = 1;
}

static int set_nonblocking(int fd) {
  // get flags from file descriptor
  int flags;
  if ((flags = fcntl(fd, F_GETFL, 0)) < 0)
    return -1;
  // add nonblocking flag
  if (fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0)
    return -1;
  return 0;
}

static const char *client_state_name(enum client_state_t state) {
  switch (state) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    CLIENT_STATES(X)
#undef X
  default:
    return "FAITH_EVENT_UNKNOWN";
  }
}

static void set_client_state(struct server_state_t *s, struct client_conn_t *cl,
                             enum client_state_t state) {
  if (!cl)
    return;
  cl->state = state;

  if (s->cfg.verbose_logging) {
    nob_log(INFO, "[client=%" PRIu64 " fd=%i]: Client changed state to %s",
            cl->conn_id, cl->fd, client_state_name(state));
  }
}

static void server_add_client(struct server_state_t *s,
                              struct client_conn_t  *cl) {
  if (!s || !cl)
    return;

  cl->prev = NULL;
  cl->next = s->clients;

  if (s->clients) {
    s->clients->prev = cl;
  }
  s->clients = cl;
}

static void server_remove_client(struct server_state_t *s,
                                 struct client_conn_t  *cl) {
  if (!s || !cl)
    return;

  if (cl->prev) {
    cl->prev->next = cl->next;
  } else {
    s->clients = cl->next;
  }

  if (cl->next) {
    cl->next->prev = cl->prev;
  }
  cl->next = NULL;
  cl->prev = NULL;
}

static faith_status_code_t
routing_register_session(struct routing_state_t *rt, client_id_t auth_id,
                         device_id_t                   device_id,
                         struct client_session_data_t *sess) {
  if (!rt || !sess)
    return FAITH_ERR_INVALID;

  if (auth_id == 0)
    return FAITH_ERR_INVALID;

  if (device_id == 0)
    return FAITH_ERR_INVALID;

  struct client_route_device_t *devmap = hmget(rt->active_users, auth_id);
  hmput(devmap, device_id, sess);
  hmput(rt->active_users, auth_id, devmap);

  nob_log(
      INFO,
      "Registered session with auth_id=%i, device_id=%i (online clients: %zu)",
      auth_id, device_id, hmlen(rt->active_users));

  return FAITH_OK;
}

static faith_status_code_t
routing_unregister_session(struct routing_state_t *rt, client_id_t auth_id,
                           device_id_t device_id) {
  if (!rt || auth_id == 0)
    return FAITH_ERR_INVALID;

  struct client_route_user_t *user = hmgetp_null(rt->active_users, auth_id);
  if (!user)
    return FAITH_ERR_INVALID;

  struct client_route_device_t *devmap = user->value;

  struct client_route_device_t *dev = hmgetp_null(devmap, device_id);
  if (!dev)
    return FAITH_ERR_INVALID;

  /* dev->value is dynamically allocated struct client_session_data_t * */
  free(dev->value);
  dev->value = NULL;

  hmdel(devmap, device_id);

  if (hmlen(devmap) == 0) {
    hmfree(devmap);
    hmdel(rt->active_users, auth_id);
  } else {
    /* Write back in case stb_ds moved/updated the devmap pointer. */
    hmput(rt->active_users, auth_id, devmap);
  }

  nob_log(INFO,
          "Unregistered session with auth_id=%i, device_id=%i (online clients: "
          "%zu)",
          auth_id, device_id, hmlen(rt->active_users));

  return FAITH_OK;
}

static void routing_destroy(struct routing_state_t *rt) {
  if (!rt)
    return;

  hmfree(rt->active_users);
}

static faith_status_code_t close_client(struct server_state_t *s,
                                        struct client_conn_t  *cl) {
  if (!cl)
    return FAITH_ERR_INVALID;

  _FH_CHECK_RETURN(
      routing_unregister_session(&s->rt, cl->client_id, cl->device_id));

  /* Unlink client from linked list of clients */
  server_remove_client(s, cl);

  epoll_ctl(s->epoll_fd, EPOLL_CTL_DEL, cl->fd, NULL);

  if (cl->ssl) {
    SSL_set_shutdown(cl->ssl, SSL_SENT_SHUTDOWN | SSL_RECEIVED_SHUTDOWN);
    SSL_free(cl->ssl);
    cl->ssl = NULL;
  }

  int log_fd = cl->fd;
  if (cl->fd >= 0) {
    close(cl->fd);
    cl->fd = -1;
  }

  if (cl->out_buf != NULL) {
    free(cl->out_buf);
    cl->out_buf = NULL;
  }

  if (cl->in_buf != NULL) {
    free(cl->in_buf);
    cl->in_buf = NULL;
  }

  nob_log(INFO, "[client=%" PRIu64 " fd=%i]: Closed client", cl->conn_id,
          log_fd);

  free(cl);

  return FAITH_OK;
}

static int modify_client_ev_mask(int epoll_fd, struct client_conn_t *cl,
                                 uint32_t mask) {
  if (!cl) {
    errno = EINVAL;
    return -1;
  }

  struct epoll_event ev;

  memset(&ev, 0, sizeof(ev));
  ev.events = mask;
  ev.data.ptr = cl;

  cl->evs_mask_want = mask;

  return epoll_ctl(epoll_fd, EPOLL_CTL_MOD, cl->fd, &ev);
}

void server_destroy(struct server_state_t *s) {
  if (!s)
    return;

  nob_log(INFO, "Destroying server context...");

  if (s->listenfd >= 0) {
    if (close(s->listenfd) < 0) {
      nob_log(ERROR, "close() on listen FD failed: %s", strerror(errno));
    }
    s->listenfd = -1;
  }

  if (s->epoll_fd >= 0) {
    if (close(s->epoll_fd) < 0) {
      nob_log(ERROR, "close() epoll FD failed: %s", strerror(errno));
    }
    s->epoll_fd = -1;
  }

  if (s->ssl_ctx) {
    SSL_CTX_free(s->ssl_ctx);
    s->ssl_ctx = NULL;
  }

  while (s->clients) {
    close_client(s, s->clients);
  }

  routing_destroy(&s->rt);

  nob_log(INFO, "Destroyed server context.");
}

static faith_status_code_t decode_ping(const uint8_t *payload,
                                       size_t         payload_size,
                                       uint64_t      *ping_nonce,
                                       uint64_t      *server_time_ms) {

  const size_t ping_size = sizeof(uint64_t) * 2;

  if (payload == NULL || ping_nonce == NULL || server_time_ms == NULL)
    return FAITH_ERR_INVALID;

  if (payload_size != ping_size)
    return FAITH_ERR_BAD_FRAME;

  *ping_nonce = faith_read_u64_be(payload);
  *server_time_ms = faith_read_u64_be(payload + sizeof(uint64_t));

  return FAITH_OK;
}

static faith_status_code_t enqueue_input_bytes(struct client_conn_t *cl,
                                               const uint8_t        *bytes,
                                               size_t                n_bytes,
                                               const struct server_cfg_t *cfg) {
  if (!cl || !cfg)
    return FAITH_ERR_INVALID;

  if (cfg->verbose_logging) {
    nob_log(INFO,
            "[client=%" PRIu64
            " fd=%i] Trying to enqueue %zu incoming bytes...",
            cl->conn_id, cl->fd, n_bytes);
  }

  if (n_bytes == 0) {
    if (cfg->verbose_logging) {
      nob_log(WARNING,
              "[client=%" PRIu64
              " fd=%i] Tried to enqueue zero length input bytes",
              cl->conn_id, cl->fd);
    }
    return FAITH_OK;
  }

  if (cl->in_size > SIZE_MAX - n_bytes)
    return FAITH_ERR_INVALID;

  size_t needed = cl->in_size + n_bytes;

  if (needed > cl->in_cap) {
    size_t new_cap = cl->in_cap ? cl->in_cap : 4096;

    while (new_cap < needed) {
      if (new_cap > SIZE_MAX / 2)
        return FAITH_ERR_INVALID;
      new_cap *= 2;
    }

    uint8_t *p = realloc(cl->in_buf, new_cap);
    if (!p)
      return FAITH_ERR_NOMEM;

    cl->in_buf = p;
    cl->in_cap = new_cap;
  }

  memcpy(cl->in_buf + cl->in_size, bytes, n_bytes);
  cl->in_size += n_bytes;

  if (cfg->verbose_logging) {
    nob_log(INFO,
            "[client=%" PRIu64
            " fd=%i] Successfully enqueued %li incoming bytes...",
            cl->conn_id, cl->fd, n_bytes);
  }

  return FAITH_OK;
}

static faith_status_code_t enqueue_output_bytes(struct client_conn_t *cl,
                                                const uint8_t        *bytes,
                                                size_t                n_bytes) {
  if (!cl)
    return FAITH_ERR_INVALID;

  if (n_bytes == 0)
    return FAITH_OK;

  if (cl->out_size > SIZE_MAX - n_bytes)
    return FAITH_ERR_INVALID;

  uint8_t *p = realloc(cl->out_buf, cl->out_size + n_bytes);
  if (!p)
    return FAITH_ERR_NOMEM;

  cl->out_buf = p;

  memcpy(cl->out_buf + cl->out_size, bytes, n_bytes);
  cl->out_size += n_bytes;

  return FAITH_OK;
}

static faith_status_code_t encode_pong(uint8_t *out_buf, size_t *out_size,
                                       size_t buf_cap_in_bytes, uint64_t nonce,
                                       uint64_t sent_at_ms) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t pong_size = sizeof(uint64_t) * 2;

  if (buf_cap_in_bytes < pong_size)
    return FAITH_ERR_OVERFLOW;

  _FH_CHECK_RETURN(faith_write_u64_be(out_buf, nonce));
  _FH_CHECK_RETURN(faith_write_u64_be(out_buf + sizeof(uint64_t), sent_at_ms));

  *out_size = pong_size;

  return FAITH_OK;
}

static faith_status_code_t
server_send_over_wire(struct client_conn_t *cl, const uint8_t *payload,
                      size_t payload_size, faith_frame_msg_type_t msg_type,
                      int epoll_fd) {
  if (!cl)
    return FAITH_ERR_INVALID;

  uint8_t *wire_data = NULL;
  size_t   wire_size = 0;

  faith_status_code_t rc = faith_encode_frame(msg_type, payload, payload_size,
                                              &wire_data, &wire_size);

  if (rc != FAITH_OK)
    return rc;

  rc = enqueue_output_bytes(cl, wire_data, wire_size);

  free(wire_data);

  if (rc != FAITH_OK) {
    nob_log(ERROR, "enqueue_output_bytes failed: %s (%d)",
            faith_status_code_name(rc), (int)rc);
    return rc;
  }

  if (modify_client_ev_mask(epoll_fd, cl,
                            EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLERR |
                                EPOLLHUP) < 0) {
    nob_log(ERROR, "failed to enable EPOLLOUT: %s", strerror(errno));
    return FAITH_ERR_IO;
  }

  nob_log(INFO, "[client=%" PRIu64 " fd=%d] queued frame: %s (%zu bytes)",
          cl->conn_id, cl->fd, faith_frame_msg_name(msg_type), wire_size);

  return FAITH_OK;
}

static faith_status_code_t
server_send_pong(struct client_conn_t *cl, faith_frame_t *frame, int epoll_fd) {
  uint64_t nonce;
  uint64_t client_sent_at_ms;

  if (!cl || !frame)
    return FAITH_ERR_INVALID;

  _FH_CHECK_RETURN(decode_ping(frame->payload, frame->payload_size, &nonce,
                               &client_sent_at_ms));

  nob_log(INFO,
          "[client=%" PRIu64 " fd=%i] server got PING: nonce=%" PRIu64
          ", client_sent_at_ms=%lu",
          cl->conn_id, cl->fd, nonce, client_sent_at_ms);

  /* Send PONG over wire protocol */

  uint64_t server_sent_at_ms = faith_now_ms();

  size_t   buf_cap_in_bytes = sizeof(uint64_t) * 2;
  uint8_t *payload = malloc(buf_cap_in_bytes);
  if (!payload)
    return FAITH_ERR_NOMEM;

  size_t payload_size = 0;

  {
    _FH_CHECK(encode_pong(payload, &payload_size, buf_cap_in_bytes, nonce,
                          server_sent_at_ms));
    if (_fh_rc != FAITH_OK) {
      free(payload);
      return _fh_rc;
    }
  }

  {
    _FH_CHECK(server_send_over_wire(cl, payload, payload_size, FAITH_MSG_PONG,
                                    epoll_fd));
    if (_fh_rc != FAITH_OK) {
      free(payload);
      return _fh_rc;
    }
  }

  if (modify_client_ev_mask(epoll_fd, cl,
                            EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLERR |
                                EPOLLHUP) < 0) {
    nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
    free(payload);
    return FAITH_ERR_IO;
  }

  nob_log(
      INFO,
      "[client=%" PRIu64
      " fd=%i] Server sent PONG to client. nonce=%lu, server_sent_at_ms=%lu",
      cl->conn_id, cl->fd, nonce, server_sent_at_ms);

  free(payload);

  return FAITH_OK;
}

static faith_status_code_t server_send_envelope(struct client_conn_t   *cl,
                                                const faith_envelope_t *envl,
                                                int epoll_fd) {
  if (!cl || !envl)
    return FAITH_ERR_INVALID;

  size_t   cap = FAITH_ENVL_HEADER_SIZE + envl->body_size;
  uint8_t *payload = malloc(cap);
  if (!payload)
    return FAITH_ERR_NOMEM;

  size_t payload_size = 0;

  faith_status_code_t rc;
  {
    _FH_CHECK(faith_encode_envelope(payload, &payload_size, cap, envl));
    rc = _fh_rc;
  }

  if (rc == FAITH_OK) {
    _FH_CHECK(server_send_over_wire(cl, payload, payload_size, FAITH_MSG_ENVL,
                               epoll_fd));
    rc = _fh_rc;
  }

  nob_log(
      INFO,
      "[client=%" PRIu64 " fd=%i] Server sent envelope: type=%s body_size=%u",
      cl->conn_id, cl->fd, faith_envelope_name(envl->type), envl->body_size);

  free(payload);
  return rc;
}

static faith_status_code_t server_handle_hello(struct server_state_t  *s,
                                               struct client_conn_t   *cl,
                                               const faith_envelope_t *hello,
                                               int epoll_fd) {

  if (!cl || !hello)
    return FAITH_ERR_INVALID;
  if (hello->type != FAITH_ENVELOPE_HELLO)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_WAIT_FOR_HELLO_ACK) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got invalid HELLO from client.",
            cl->conn_id, cl->fd);
    return FAITH_ERR_INVALID;
  }

  if (cl->authorized != 1) {
    return FAITH_ERR_UNAUTHORIZED;
  }

  if (hello->sender_id == 0) {
    return FAITH_ERR_INVALID;
  }

  if (hello->body_size != sizeof(uint32_t))
    return FAITH_ERR_BAD_FRAME;

  device_id_t device_id = faith_read_u32_be(hello->body);

  /* Send HELLO_OK evelope back to client */
  faith_envelope_t hello_ok = {0};
  hello_ok.type = FAITH_ENVELOPE_HELLO_OK;
  hello_ok.recipient_id = hello->sender_id;

  cl->client_id = hello->sender_id;
  cl->device_id = device_id;

  _FH_CHECK_RETURN(server_send_envelope(cl, &hello_ok, epoll_fd));

  /* allocate session data */
  struct client_session_data_t *sess = calloc(1, sizeof(*sess));
  sess->conn = cl;

  /* register client session */
  _FH_CHECK_RETURN(
      routing_register_session(&s->rt, cl->client_id, cl->device_id, sess));

  set_client_state(s, cl, CLIENT_OPEN);

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server accepted HELLO (client id: %" PRIu64 ")",
          cl->conn_id, cl->fd, cl->client_id);

  return FAITH_OK;
}

static faith_status_code_t server_handle_envl(struct server_state_t *s,
                                              struct client_conn_t  *cl,
                                              faith_frame_t         *frame,
                                              int                    epoll_fd) {

  if (!frame || !cl)
    return FAITH_ERR_INVALID;

  faith_envelope_t envl;

  _FH_CHECK_RETURN(faith_decode_envelope(frame->payload, frame->payload_size, &envl));

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server got envelope: type=%s body_size=%u",
          cl->conn_id, cl->fd, faith_envelope_name(envl.type), envl.body_size);

  faith_status_code_t rc = FAITH_OK;
  if (cl->state != CLIENT_OPEN && envl.type != FAITH_ENVELOPE_HELLO) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Client is not acknowledged yet. Will "
            "not handle envelope (%s).",
            cl->conn_id, cl->fd, faith_envelope_name(envl.type));

    rc = FAITH_ERR_IO;
    goto defer;
  }

  switch (envl.type) {
  case FAITH_ENVELOPE_HELLO: {
    _FH_CHECK(server_handle_hello(s, cl, &envl, epoll_fd));
    rc = _fh_rc;
    break;
  }
  default:
    break;
  }

defer:
  if (envl.body != NULL)
    free(envl.body);

  return rc;
}

static faith_status_code_t handle_frame(struct server_state_t *s,
                                        struct client_conn_t  *cl,
                                        faith_frame_t *frame, int epfd) {
  if (!cl)
    return FAITH_ERR_INVALID;

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server got frame: msg_type=%s payload_size=%zu",
          cl->conn_id, cl->fd, faith_frame_msg_name(frame->msg_type),
          frame->payload_size);

  if (cl->state != CLIENT_OPEN && frame->msg_type != FAITH_MSG_ENVL) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Client is not acknowledged yet. Rejecting frame (type=%s)",
            cl->conn_id, cl->fd, faith_frame_msg_name(frame->msg_type));
    return FAITH_ERR_IO;
  }

  switch (frame->msg_type) {
  case FAITH_MSG_PING:
    _FH_CHECK_RETURN(server_send_pong(cl, frame, epfd));
    break;
  case FAITH_MSG_ENVL:
    _FH_CHECK_RETURN(server_handle_envl(s, cl, frame, epfd));
    break;
  default:
    return FAITH_ERR_BAD_FRAME;
  }

  return FAITH_OK;
}

static enum read_frame_result_t
read_more_ssl_bytes(struct client_conn_t *cl, const struct server_cfg_t *cfg) {
  if (!cl || !cfg)
    return READ_FRAME_ERROR;

  uint8_t tmp[4096];

  int nread = SSL_read(cl->ssl, tmp, sizeof(tmp));

  if (nread > 0) {
    faith_status_code_t rc = enqueue_input_bytes(cl, tmp, (size_t)nread, cfg);

    if (rc != FAITH_OK)
      return READ_FRAME_ERROR;

    return READ_FRAME_GOT_BYTES;
  }

  int err = SSL_get_error(cl->ssl, nread);

  if (err == SSL_ERROR_WANT_READ)
    return READ_FRAME_WANT_READ;

  if (err == SSL_ERROR_WANT_WRITE)
    return READ_FRAME_WANT_WRITE;

  if (err == SSL_ERROR_ZERO_RETURN)
    return READ_FRAME_CLOSED;

  return READ_FRAME_ERROR;
}

static faith_status_code_t try_parse_frame_from_buffer(const uint8_t *buf,
                                                       size_t         size,
                                                       faith_frame_t *frame,
                                                       size_t *consumed_out) {
  if (!frame || !consumed_out)
    return FAITH_ERR_INVALID;

  if (!buf) {
    if (size == 0) {
      return FAITH_ERR_INCOMPLETE;
    }
    return FAITH_ERR_INVALID;
  }

  if (size < FAITH_HEADER_SIZE) {
    return FAITH_ERR_INCOMPLETE;
  }

  uint32_t frame_size = faith_read_u32_be(buf + 0);

  if (frame_size < FAITH_HEADER_SIZE - (sizeof(uint16_t) + sizeof(uint16_t)))
    return FAITH_ERR_BAD_FRAME;

  if (frame_size > FAITH_MAX_FRAME_LEN)
    return FAITH_ERR_FRAME_TOO_LARGE;

  size_t total_frame_size = sizeof(uint32_t) + frame_size;

  if (size < total_frame_size) {
    return FAITH_ERR_INCOMPLETE;
  }

  uint16_t proto_ver = faith_read_u16_be(buf + sizeof(uint32_t));
  uint16_t msg_type =
      faith_read_u16_be(buf + sizeof(uint32_t) + sizeof(uint16_t));

  if (proto_ver != FAITH_PROTO_VERSION)
    return FAITH_ERR_UNSUPPORTED_VER;

  memset(frame, 0, sizeof(*frame));

  frame->proto_ver = proto_ver;
  frame->msg_type = msg_type;
  frame->frame_size = frame_size;
  frame->payload_size = frame_size - (sizeof(proto_ver) + sizeof(msg_type));

  if (frame->payload_size > 0) {
    frame->payload = malloc(frame->payload_size);
    if (!frame->payload)
      return FAITH_ERR_NOMEM;

    memcpy(frame->payload, buf + FAITH_HEADER_SIZE, frame->payload_size);
  }

  *consumed_out = frame_size + sizeof(uint32_t);

  return FAITH_OK;
}

static enum read_frame_result_t
try_read_one_frame(struct client_conn_t *cl, faith_frame_t *frame,
                   const struct server_cfg_t *cfg) {
  while (1) {
    size_t consumed = 0;

    if (cfg->verbose_logging) {
      nob_log(INFO,
              "[client=%" PRIu64 " fd=%i] Trying to parse frame buffer...",
              cl->conn_id, cl->fd);
    }

    faith_status_code_t rc =
        try_parse_frame_from_buffer(cl->in_buf, cl->in_size, frame, &consumed);

    if (rc == FAITH_OK) {

      if (cfg->verbose_logging) {
        nob_log(
            INFO,
            "[client=%" PRIu64
            " fd=%i] Successfully parsed full frame from buffer (%li bytes)",
            cl->conn_id, cl->fd, consumed);
      }

      size_t remaining = cl->in_size - consumed;

      if (remaining > 0) {
        memmove(cl->in_buf, cl->in_buf + consumed, remaining);
      }

      cl->in_size -= consumed;

      return READ_FRAME_OK;
    }

    if (rc == FAITH_ERR_INCOMPLETE) {
      /* Not enough bytes yet, so read more decrypted TLS data. */
      if (cfg->verbose_logging) {
        nob_log(INFO,
                "[client=%" PRIu64
                " fd=%i] Frame incomplete, reading more bytes...",
                cl->conn_id, cl->fd);
      }
      enum read_frame_result_t rr = read_more_ssl_bytes(cl, cfg);

      if (rr == READ_FRAME_GOT_BYTES) {
        if (cfg->verbose_logging) {
          nob_log(INFO,
                  "[client=%" PRIu64
                  " fd=%i] Got new bytes, parsing frame again...",
                  cl->conn_id, cl->fd);
        }
        continue;
      }

      if (rr == READ_FRAME_WANT_READ) {
        if (cfg->verbose_logging) {
          nob_log(INFO,
                  "[client=%" PRIu64
                  " fd=%i] SSL_read needs to wait for socket to be readable",
                  cl->conn_id, cl->fd);
        }
        return rr;
      }

      if (rr == READ_FRAME_WANT_WRITE) {
        if (cfg->verbose_logging) {
          nob_log(INFO,
                  "[client=%" PRIu64
                  " fd=%i] SSL_read needs to wait for socket to be writable",
                  cl->conn_id, cl->fd);
        }
        return rr;
      }

      if (rr == READ_FRAME_CLOSED) {
        nob_log(INFO,
                "[client=%" PRIu64
                " fd=%i] Connection closed while reading incomplete frame.",
                cl->conn_id, cl->fd);
        return rr;
      }

      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%i] Error while reading incomplete frame.",
              cl->conn_id, cl->fd);

      return rr;
    }

    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Failed to read frame",
            cl->conn_id, cl->fd);

    return READ_FRAME_ERROR;
  }
}

static int drive_client_read(struct server_state_t *s, struct client_conn_t *cl,
                             const struct server_cfg_t *cfg) {
  for (;;) {
    faith_frame_t frame;

    if (cfg->verbose_logging) {
      nob_log(INFO, "[client=%" PRIu64 " fd=%i]: HANDLING NEW CLIENT FRAME.",
              cl->conn_id, cl->fd);
    }
    enum read_frame_result_t rr = try_read_one_frame(cl, &frame, cfg);

    if (rr == READ_FRAME_OK) {
      /* Handle protocol frame */
      {
        _FH_CHECK(handle_frame(s, cl, &frame, s->epoll_fd));
        if (_fh_rc) {

          faith_frame_free(&frame);
          return -1;
        }
      }
      faith_frame_free(&frame);

      if (cfg->verbose_logging) {
        nob_log(INFO,
                "[client=%" PRIu64 " fd=%i]: SUCCESS HANDLING CLIENT FRAME.",
                cl->conn_id, cl->fd);
      }

      if (cl->out_buf && cl->out_off < cl->out_size) {
        return 0;
      }
      continue;
    }

    if (rr == READ_FRAME_WANT_READ) {
      uint32_t mask = EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP;

      if (cl->out_buf && cl->out_off < cl->out_size) {
        mask |= EPOLLOUT;
      }

      if (modify_client_ev_mask(s->epoll_fd, cl, mask) < 0) {
        return -1;
      }
      return 0;
    }

    if (rr == READ_FRAME_WANT_WRITE) {
      if (modify_client_ev_mask(s->epoll_fd, cl,
                                EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLERR |
                                    EPOLLHUP) < 0) {
        nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
        return -1;
      }
      return 0;
    }

    if (rr == READ_FRAME_CLOSED)
      return -1;

    nob_log(ERROR, "[client=%" PRIu64 " fd=%i]: Failed to read client frame.",
            cl->conn_id, cl->fd);

    return -1;
  }
}

static void accept_clients(struct server_state_t *s) {
  while (1) {
    struct sockaddr_in addr;
    socklen_t          addrsz = sizeof(addr);

    int client_fd =
        accept4(s->listenfd, (struct sockaddr *)&addr, &addrsz, SOCK_NONBLOCK);

    if (client_fd < 0) {
      // no clients left to accept
      if (errno == EAGAIN || errno == EWOULDBLOCK)
        return;

      nob_log(ERROR, "accept4() failed: %s", strerror(errno));
      return;
    }

    // TODO: Arena allocator
    struct client_conn_t *cl = calloc(1, sizeof(*cl));
    if (!cl) {
      nob_log(WARNING, "failed to allocate memory with calloc() for pending "
                       "client connection.");
      close(client_fd);
      continue;
    }

    cl->conn_id = atomic_fetch_add(&s->next_client_id, 1);

    nob_log(INFO, "accepted new client id=%" PRIu64 " fd=%i", cl->conn_id,
            client_fd);

    cl->fd = client_fd;
    set_client_state(s, cl, CLIENT_HANDSHAKE);

    /* Add client to linked list of clients */
    server_add_client(s, cl);

    // TODO: temporary
    cl->authorized = 1;

    // create SSL object for client
    cl->ssl = SSL_new(s->ssl_ctx);
    if (!cl->ssl) {
      nob_log(ERROR, "SSL_new() failed for client connection (FD: %i)",
              client_fd);
      _FH_CHECK(close_client(s, cl));
      continue;
    }

    // set file descriptor of ssl object
    SSL_set_fd(cl->ssl, cl->fd);
    SSL_set_accept_state(cl->ssl);

    struct epoll_event ev;
    memset(&ev, 0, sizeof(ev));

    ev.events = EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP;
    ev.data.ptr = cl;
    cl->evs_mask_want = ev.events;

    if (epoll_ctl(s->epoll_fd, EPOLL_CTL_ADD, client_fd, &ev) < 0) {
      nob_log(ERROR, "epoll_ctl() failed for client (FD: %i): %s", client_fd,
              strerror(errno));
      _FH_CHECK(close_client(s, cl));
      continue;
    }
  }
}

static faith_status_code_t drive_tls_handshake(struct server_state_t *s,
                                               struct client_conn_t  *cl) {
  int rc = SSL_accept(cl->ssl);

  uint32_t base_mask = EPOLLRDHUP | EPOLLERR | EPOLLHUP;

  if (rc == 1) {
    set_client_state(s, cl, CLIENT_WAIT_FOR_HELLO_ACK);
    base_mask |= EPOLLIN;

    if (modify_client_ev_mask(s->epoll_fd, cl, base_mask) < 0) {
      nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
      return FAITH_ERR_IO;
    }

    return FAITH_OK;
  }

  int err = SSL_get_error(cl->ssl, rc);

  if (err == SSL_ERROR_WANT_READ) {
    base_mask |= EPOLLIN;

    if (modify_client_ev_mask(s->epoll_fd, cl, base_mask) < 0) {
      nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
      return FAITH_ERR_IO;
    }

    return FAITH_OK;
  }

  if (err == SSL_ERROR_WANT_WRITE) {
    base_mask |= EPOLLOUT;

    if (modify_client_ev_mask(s->epoll_fd, cl, base_mask) < 0) {
      nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
      return FAITH_ERR_IO;
    }

    return FAITH_OK;
  }

  nob_log(ERROR, "TLS handshake failed");
  ERR_print_errors_fp(stderr);

  return FAITH_ERR_IO;
}

static faith_status_code_t flush_client_output(int                   epoll_fd,
                                               struct client_conn_t *cl) {

  if (!cl || (!cl->out_buf && cl->out_size > 0))
    return FAITH_ERR_INVALID;
  if (!cl->out_buf)
    return FAITH_OK;

  while (cl->out_off < cl->out_size) {
    size_t remaining = cl->out_size - cl->out_off;
    /* becaused this is passed as int to SSL, we need to clamp to integer
     * range */
    int clamped = remaining > INT_MAX ? INT_MAX : (int)remaining;

    int nwrite = SSL_write(cl->ssl, cl->out_buf + cl->out_off, clamped);
    nob_log(INFO, "[client=%" PRIu64 " fd=%i]: wrote %i bytes over the wire.",
            cl->conn_id, cl->fd, nwrite);

    if (nwrite > 0) {
      cl->out_off += (size_t)nwrite;
      continue;
    }

    int err = SSL_get_error(cl->ssl, nwrite);

    if (err == SSL_ERROR_WANT_WRITE || err == SSL_ERROR_WANT_READ) {
      if (modify_client_ev_mask(epoll_fd, cl,
                                EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLERR |
                                    EPOLLHUP) < 0) {
        nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
        return FAITH_ERR_IO;
      }

      return FAITH_OK;
    }

    nob_log(ERROR, "SSL_write failed");
    ERR_print_errors_fp(stderr);

    return FAITH_ERR_IO;
  }

  free(cl->out_buf);
  cl->out_buf = NULL;
  cl->out_size = 0;
  cl->out_off = 0;

  if (modify_client_ev_mask(epoll_fd, cl,
                            EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP) < 0) {
    nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
    return FAITH_ERR_IO;
  }

  return FAITH_OK;
}

int loop(struct server_state_t *s) {
  struct epoll_event events[MAX_EVENTS];

  while (!shutdown_requested) {
    int n = epoll_wait(s->epoll_fd, events, MAX_EVENTS, -1);

    if (n < 0) {
      if (errno == EINTR)
        continue;

      nob_log(ERROR, "epoll_wait() failed: %s", strerror(errno));

      return 1;
    }

    for (int i = 0; i < n; i++) {
      uint32_t revents = events[i].events;

      if (events[i].data.fd == s->listenfd) {
        accept_clients(s);
        continue;
      }

      struct client_conn_t *c = events[i].data.ptr;

      if (revents & (EPOLLERR | EPOLLHUP | EPOLLRDHUP)) {
        _FH_CHECK(close_client(s, c));
        continue;
      }

      int dead = 0;

      if (c->state == CLIENT_HANDSHAKE) {
        if (drive_tls_handshake(s, c) < 0)
          dead = 1;
      } else if (c->state == CLIENT_OPEN ||
                 c->state == CLIENT_WAIT_FOR_HELLO_ACK) {
        if ((revents & EPOLLIN) && drive_client_read(s, c, &s->cfg) < 0)
          dead = 1;

        if (!dead && (revents & EPOLLOUT) &&
            flush_client_output(s->epoll_fd, c) < 0)
          dead = 1;
      }

      if (dead) {
        _FH_CHECK(close_client(s, c));
      }
    }
  }

  return 0;
}

static int init_ssl(SSL_CTX **ctx) {
  int result = 0;

  *ctx = SSL_CTX_new(TLS_server_method());
  if (*ctx == NULL) {
    nob_log(ERROR, "failed to create server SSL_CTX");
    return_defer(1);
  }

  if (!SSL_CTX_set_min_proto_version(*ctx, TLS1_2_VERSION)) {
    nob_log(ERROR, "failed to set the minimum TLS protocol version");
    return_defer(1);
  }

  long opts = SSL_OP_IGNORE_UNEXPECTED_EOF | SSL_OP_NO_RENEGOTIATION |
              SSL_OP_SERVER_PREFERENCE;

  SSL_CTX_set_options(*ctx, opts);

  if (SSL_CTX_use_certificate_chain_file(*ctx, "chain.pem") <= 0) {
    nob_log(ERROR, "failed to load the server certificate chain file");
    return_defer(1);
  }

  if (SSL_CTX_use_PrivateKey_file(*ctx, "pkey.pem", SSL_FILETYPE_PEM) <= 0) {
    nob_log(ERROR, "failed loading the server private key file, "
                   "possible key/cert mismatch?");
    return_defer(1);
  }

  static const char cache_id[] = "faithd-server";

  SSL_CTX_set_session_id_context(*ctx, (void *)cache_id, sizeof(cache_id));
  SSL_CTX_set_session_cache_mode(*ctx, SSL_SESS_CACHE_SERVER);
  SSL_CTX_sess_set_cache_size(*ctx, 1024);
  SSL_CTX_set_timeout(*ctx, 3600);
  SSL_CTX_set_verify(*ctx, SSL_VERIFY_NONE, NULL);

  nob_log(INFO, "SSL initialized.");

  return 0;

defer:
  ERR_print_errors_fp(stderr);
  if (*ctx) {
    SSL_CTX_free(*ctx);
    *ctx = NULL;
  }
  return result;
}

static int init_listen_sock(void) {
  struct sockaddr_in servaddr;

  int listenfd = -1;

  listenfd = socket(AF_INET, SOCK_STREAM, 0);
  if (listenfd == -1) {
    nob_log(ERROR, "socket creation failed");
    return -1;
  }

  if (set_nonblocking(listenfd) < 0) {
    nob_log(ERROR, "failed to set socket to O_NONBLOCK");
    close(listenfd);
    return -1;
  }

  nob_log(INFO, "Socket successfully created");

  memset(&servaddr, 0, sizeof(servaddr));

  servaddr.sin_family = AF_INET;
  servaddr.sin_addr.s_addr = htonl(INADDR_ANY);
  servaddr.sin_port = htons(PORT);

  int yes = 1;
  if (setsockopt(listenfd, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes)) < 0) {
    nob_log(ERROR, "setsockopt(SO_REUSEADDR) failed: %s", strerror(errno));
    close(listenfd);
    return -1;
  }

  if ((bind(listenfd, (struct sockaddr *)&servaddr, sizeof(servaddr))) != 0) {
    nob_log(ERROR, "socket bind failed");
    close(listenfd);
    return -1;
  }

  nob_log(INFO, "Socket successfully bound");

  if ((listen(listenfd, SOMAXCONN)) != 0) {
    nob_log(ERROR, "listen() failed: %s", strerror(errno));
    close(listenfd);
    return -1;
  }

  nob_log(INFO, "Server listening..");

  return listenfd;
}

static int init_epoll_fd(int listenfd) {
  int epoll_fd = epoll_create1(EPOLL_CLOEXEC);
  if (epoll_fd == -1) {
    nob_log(ERROR, "epoll_create1(EPOLL_CLOEXEC) failed: %s", strerror(errno));
    return -1;
  }
  struct epoll_event ev = {0};

  ev.events = EPOLLIN;
  ev.data.fd = listenfd;

  if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, listenfd, &ev) == -1) {
    nob_log(ERROR, "Failed to add listening FD to epoll FD with epoll_ctl: %s",
            strerror(errno));
    close(epoll_fd);
    return -1;
  }

  return epoll_fd;
}

static int install_signal_handlers(void) {
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));

  sa.sa_handler = handle_shutdown_signal;
  sigemptyset(&sa.sa_mask);

  if (sigaction(SIGINT, &sa, NULL) < 0) {
    nob_log(ERROR, "sigaction(SIGINT) failed: %s", strerror(errno));
    return 1;
  }

  if (sigaction(SIGTERM, &sa, NULL) < 0) {
    nob_log(ERROR, "sigaction(SIGTERM) failed: %s", strerror(errno));
    return 1;
  }

  return 0;
}

static void print_usage(const char *prog) {
  printf("Usage: %s [options]\n"
         "\n"
         "Options:\n"
         "  -v, --verbose    Enable verbose logging\n"
         "  -h, --help       Show this help message\n",
         prog);
}

static int parse_args(int argc, char **argv, struct server_state_t *s) {
  if (!s)
    return 1;

  for (int i = 1; i < argc; i++) {
    const char *arg = argv[i];

    if (strcmp(arg, "-v") == 0 || strcmp(arg, "--verbose") == 0) {
      s->cfg.verbose_logging = 1;
      continue;
    }

    if (strcmp(arg, "-h") == 0 || strcmp(arg, "--help") == 0) {
      print_usage(argv[0]);
      return 2;
    }

    fprintf(stderr, "Unknown option: %s\n\n", arg);
    print_usage(argv[0]);
    return 1;
  }

  return 0;
}

static int routing_init(struct routing_state_t *rt) {
  if (!rt)
    return 1;

  // rt->online_clients = map64_init();

  return 0;
}

static int server_init(struct server_state_t *s) {
  if (!s)
    return 1;

  if (install_signal_handlers() != 0)
    exit(1);

  if (init_ssl(&s->ssl_ctx) != 0) {
    server_destroy(s);
    return 1;
  }

  if ((s->listenfd = init_listen_sock()) < 0) {
    server_destroy(s);
    return 1;
  }

  if ((s->epoll_fd = init_epoll_fd(s->listenfd)) < 0) {
    server_destroy(s);
    return 1;
  }

  if (routing_init(&s->rt) != 0) {
    server_destroy(s);
    return 1;
  }

  return 0;
}

int main(int argc, char **argv) {
  SSL_library_init();
  SSL_load_error_strings();
  OpenSSL_add_ssl_algorithms();

  nob_set_log_handler(faith_log_handler);

  struct server_state_t s = {.listenfd = -1,
                             .epoll_fd = -1,
                             .ssl_ctx = NULL,
                             .next_client_id = 1,
                             .cfg = {0}};

  int arg_rc = parse_args(argc, argv, &s);

  // User only wanted to print help message
  if (arg_rc == 2) {
    return 0;
  }

  if (arg_rc != 0) {
    return 1;
  }

  /* Init */
  int init_rc;

  init_rc = server_init(&s);

  if (init_rc != 0) {
    exit(init_rc);
  }

  /* Main server loop */
  int rc = loop(&s);

  /* Cleanup */
  server_destroy(&s);

  return rc;
}
