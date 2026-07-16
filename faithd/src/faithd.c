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
  X(CLIENT_WAIT_FOR_HELLO, 3)                                                  \
  X(CLIENT_WAIT_FOR_CHALLENGE_RESPONSE, 4)                                     \
  X(CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE, 5)

enum client_state_t {
#define X(name, value) name = value,
  CLIENT_STATES(X)
#undef X
};

struct client_temporary_handshake_params_t {
  uint64_t          nonce;
  uint64_t          server_nonce;
  uint8_t           public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  faith_client_id_t sender_auth_id;
  faith_device_id_t device_id;
};

struct client_conn_t {
  // connection id
  uint64_t            conn_id;
  faith_client_id_t   auth_id;
  faith_device_id_t   device_id;
  int                 fd;
  SSL                *ssl;
  enum client_state_t state;

  uint32_t ev_mask;

  uint8_t *out_buf;
  size_t   out_size;
  size_t   out_cap;
  size_t   out_off;

  uint8_t *in_buf;
  size_t   in_size;
  size_t   in_cap;
  size_t   in_off;

  struct client_conn_t *next;
  struct client_conn_t *prev;

  struct client_temporary_handshake_params_t temp_handshake_params;
  faith_envl_stc_device_link_req_t          *pending_device_link_req;
  struct client_conn_t                      *pending_device_link_conn;

  int authorized;

  int closing;
  int close_after_flush;
};

struct client_identity_t {
  uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
};

struct client_device_session_data_t {
  struct client_conn_t    *conn;
  struct client_identity_t ident;
};

struct client_route_device_t {
  faith_device_id_t                    key; /* device id */
  struct client_device_session_data_t *value;
};

/* nested hashmap [auth id -> device id -> conn/sess data] */
struct client_route_user_t {
  faith_client_id_t key; /* client/auth id */
  struct client_route_device_t
      *value; /* a hashmap of device id -> client conn/sess data*/
};

struct server_msg_request_t {
  faith_client_id_t auth_id_sender;
  faith_client_id_t auth_id_receiver;
  uint64_t          created_at_ms;
  uint64_t          expires_at_ms;
};

/* hashmap [request ID -> server_msg_request_t] */
struct stored_msg_request_t {
  faith_request_id_t           key;
  struct server_msg_request_t *value;
};

struct routing_state_t {
  struct client_route_user_t *active_users;
};

struct storage_state_t {
  struct stored_msg_request_t* msg_requests;
};

struct server_state_t {
  int      listenfd;
  int      epoll_fd;
  SSL_CTX *ssl_ctx;

  atomic_uint_fast64_t  next_client_id;
  struct client_conn_t *clients;

  struct server_cfg_t    cfg;
  struct routing_state_t rt;
  struct storage_state_t storage;
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
    nob_log(INFO, "[client=%" PRIu64 " fd=%i] Client changed state to %s",
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
routing_get_user_from_auth_id(struct routing_state_t      *rt,
                              const faith_client_id_t     *auth_id,
                              struct client_route_user_t **o_user) {
  if (!rt || !auth_id || !o_user)
    return FAITH_ERR_INVALID;

  struct client_route_user_t *user = hmgetp_null(rt->active_users, *auth_id);
  *o_user = user;

  return FAITH_OK;
}

static int routing_auth_id_registered(struct routing_state_t  *rt,
                                      const faith_client_id_t *auth_id) {
  struct client_route_user_t *user = NULL;
  _FH_CHECK(routing_get_user_from_auth_id(rt, auth_id, &user));
  if (_fh_rc != FAITH_OK) {
    return 0;
  }
  return user != NULL;
}

static faith_status_code_t
routing_get_devices(struct routing_state_t        *rt,
                    const faith_client_id_t       *auth_id,
                    struct client_route_device_t **o_devmap) {
  if (!rt || !o_devmap)
    return FAITH_ERR_INVALID;

  struct client_route_user_t *user = NULL;
  _FH_CHECK_RETURN(routing_get_user_from_auth_id(rt, auth_id, &user));
  if (!user)
    return FAITH_ERR_NOT_FOUND;

  *o_devmap = user->value;

  return FAITH_OK;
}

static faith_status_code_t routing_register_session(
    struct routing_state_t *rt, const faith_client_id_t *auth_id,
    const faith_device_id_t *device_id, struct client_conn_t *cl,
    const uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]) {
  if (!rt || !auth_id || !device_id)
    return FAITH_ERR_INVALID;

  /* hmget() inserts <auth_id> if not found */
  struct client_route_device_t *devmap = hmget(rt->active_users, *auth_id);

  /* allocate new session data */
  struct client_device_session_data_t *sess = calloc(1, sizeof(*sess));
  if (!sess) {
    return FAITH_ERR_NOMEM;
  }

  sess->conn = cl;

  /* assign public key to new session */
  memcpy(sess->ident.public_key, public_key, FAITH_ED25519_PUBLIC_KEY_SIZE);

  /* insert session data at <auth_id, device_id pair> */
  hmput(devmap, *device_id, sess);

  /* write pointer back to avoid stale pointers */
  hmput(rt->active_users, *auth_id, devmap);

  char auth_id_hex[33];
  char device_id_hex[33];

  faith_status_code_t _fh_result = FAITH_OK;
  _FH_CHECK_DEFER(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
  _FH_CHECK_DEFER(faith_id128_to_hex(device_id->bytes, device_id_hex));

  nob_log(
      INFO,
      "Registered session with auth_id=%s, device_id=%s (online clients: %zu)",
      auth_id_hex, device_id_hex, hmlen(rt->active_users));

  return FAITH_OK;
defer:
  free(sess);
  return _fh_result;
}

static faith_status_code_t
routing_get_session(struct routing_state_t               *rt,
                    const faith_client_id_t              *auth_id,
                    const faith_device_id_t              *device_id,
                    struct client_device_session_data_t **o_sess) {
  if (!rt || !o_sess || !auth_id || !device_id)
    return FAITH_ERR_INVALID;

  struct client_route_user_t *user = NULL;
  _FH_CHECK_RETURN(routing_get_user_from_auth_id(rt, auth_id, &user));
  if (!user)
    return FAITH_ERR_NOT_FOUND;

  struct client_route_device_t *devmap = user->value;

  struct client_route_device_t *dev = hmgetp_null(devmap, *device_id);

  // Session is registered, but exact device not registered yet, *o_sess will be
  // NULL to indicate this state.
  if (!dev)
    return FAITH_OK;

  *o_sess = dev->value;

  return FAITH_OK;
}

static faith_status_code_t
routing_unregister_session(struct routing_state_t  *rt,
                           const faith_client_id_t *auth_id,
                           const faith_device_id_t *device_id) {
  if (!rt || !auth_id || !device_id)
    return FAITH_ERR_INVALID;

  struct client_route_user_t *user = NULL;
  _FH_CHECK_RETURN(routing_get_user_from_auth_id(rt, auth_id, &user));
  if (!user)
    return FAITH_ERR_NOT_FOUND;

  struct client_route_device_t *devmap = user->value;

  struct client_route_device_t *dev = hmgetp_null(devmap, *device_id);
  if (!dev)
    return FAITH_ERR_NOT_FOUND;

  /* <dev->value> is a dynamically allocated struct client_device_session_data_t
   */
  free(dev->value);
  dev->value = NULL;

  hmdel(devmap, *device_id);

  if (hmlen(devmap) == 0) {
    hmfree(devmap);
    hmdel(rt->active_users, *auth_id);
  } else {
    /* write pointer back to avoid stale pointers */
    hmput(rt->active_users, *auth_id, devmap);
  }

  char auth_id_hex[33];
  char device_id_hex[33];

  _FH_CHECK_RETURN(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(device_id->bytes, device_id_hex));

  nob_log(INFO,
          "Unregistered session with auth_id=%s, device_id=%s (online clients: "
          "%zu)",
          auth_id_hex, device_id_hex, hmlen(rt->active_users));

  return FAITH_OK;
}

static faith_status_code_t storage_store_msg_request(
    struct storage_state_t            *st,
    const faith_request_id_t           *request_id,
    const struct server_msg_request_t  *request) {
  if (!st || !request_id || !request)
    return FAITH_ERR_INVALID;

  /* Refuse to overwrite an existing request. This also handles the extremely
   * unlikely case of a randomly generated server request ID colliding. */
  if (hmgetp_null(st->msg_requests, *request_id))
    return FAITH_ERR_ALREADY_EXISTS;

  struct server_msg_request_t *stored = malloc(sizeof(*stored));
  if (!stored)
    return FAITH_ERR_NOMEM;

  *stored = *request;

  hmput(st->msg_requests, *request_id, stored);

  return FAITH_OK;
}

static faith_status_code_t
storage_remove_msg_request(struct storage_state_t  *st,
                           const faith_request_id_t *request_id) {
  if (!st || !request_id)
    return FAITH_ERR_INVALID;

  struct stored_msg_request_t *entry =
      hmgetp_null(st->msg_requests, *request_id);

  if (!entry)
    return FAITH_ERR_NOT_FOUND;

  free(entry->value);
  entry->value = NULL;

  hmdel(st->msg_requests, *request_id);

  return FAITH_OK;
}
faith_status_code_t
storage_get_msg_request(struct storage_state_t *st,
                        const faith_request_id_t     *request_id,
                        struct server_msg_request_t **out) {
  if (!st || !request_id || !out)
    return FAITH_ERR_INVALID;

  *out = NULL;

  ptrdiff_t index = hmgeti(st->msg_requests, *request_id);

  if (index < 0)
    return FAITH_ERR_NOT_FOUND;

  /* The request was found but not correctly allocated */
  if (!st->msg_requests[index].value)
    return FAITH_ERR_INVALID;

  *out = st->msg_requests[index].value;

  return FAITH_OK;
}

static void routing_destroy(struct routing_state_t *rt) {
  if (!rt)
    return;

  ptrdiff_t user_count = hmlen(rt->active_users);

  for (ptrdiff_t i = 0; i < user_count; ++i) {
    struct client_route_device_t *devices = rt->active_users[i].value;
    ptrdiff_t                     device_count = hmlen(devices);

    for (ptrdiff_t j = 0; j < device_count; ++j) {
      free(devices[j].value);
      devices[j].value = NULL;
    }

    hmfree(devices);
    rt->active_users[i].value = NULL;
  }

  hmfree(rt->active_users);
  rt->active_users = NULL;
}

static faith_status_code_t
server_cancel_pending_device_link(struct server_state_t *s,
                                  struct client_conn_t  *requesting_cl);

static faith_status_code_t
server_disconnect_client(struct server_state_t *s, struct client_conn_t *cl,
                         faith_client_disconnect_reason_t reason,
                         faith_client_reconnect_policy_t  reconnect_policy,
                         uint64_t retry_after_ms, uint64_t banned_until_ms,
                         const char *message);

static faith_status_code_t close_client(struct server_state_t *s,
                                        struct client_conn_t **cl_ptr) {
  if (!s || !cl_ptr || !*cl_ptr)
    return FAITH_ERR_INVALID;

  struct client_conn_t *cl = *cl_ptr;
  faith_status_code_t   result = FAITH_OK;

  *cl_ptr = NULL;

  if (cl->state == CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE) {
    faith_status_code_t rc = server_cancel_pending_device_link(s, cl);

    if (rc != FAITH_OK) {
      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%d] Failed to cancel pending device-link request: %s",
              cl->conn_id, cl->fd, faith_status_code_name(rc));

      if (result == FAITH_OK)
        result = rc;
    }
  }

  server_remove_client(s, cl);

  if (cl->authorized) {
    if (cl->pending_device_link_conn != NULL &&
        cl->pending_device_link_req != NULL) {
      struct client_route_device_t *devices = NULL;
      faith_status_code_t           rc =
          routing_get_devices(&s->rt, &cl->auth_id, &devices);
      if (rc != FAITH_OK || devices == NULL) {
        nob_log(ERROR,
                "[client=%" PRIu64
                " fd=%d] Failed to enumerate account devices: %s",
                cl->conn_id, cl->fd, faith_status_code_name(rc));
        if (result == FAITH_OK)
          result = rc;
      } else if (hmlen(devices) - 1 == 0) {
        rc = server_disconnect_client(
            s, cl->pending_device_link_conn, FAITH_DISCONNECT_TEMPORARY_FAILURE,
            FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
            "All authorized devices of the account you "
            "are trying to link to have disconnected.");
        if (result == FAITH_OK)
          result = rc;
      }
      free(cl->pending_device_link_req);
      cl->pending_device_link_req = NULL;
      cl->pending_device_link_conn = NULL;
    }

    // TODO: persistent sessions 
    faith_status_code_t rc =
        routing_unregister_session(&s->rt, &cl->auth_id, &cl->device_id);

    if (rc != FAITH_OK) {
      nob_log(ERROR, "[client=%" PRIu64 "] routing unregister failed: %s (%d)",
              cl->conn_id, faith_status_code_name(rc), (int)rc);

      result = rc;
    }
  }

  if (cl->fd >= 0) {
    if (epoll_ctl(s->epoll_fd, EPOLL_CTL_DEL, cl->fd, NULL) < 0 &&
        errno != ENOENT) {
      nob_log(ERROR, "[client=%" PRIu64 " fd=%d] epoll delete failed: %s",
              cl->conn_id, cl->fd, strerror(errno));

      if (result == FAITH_OK)
        result = FAITH_ERR_IO;
    }
  }

  if (cl->ssl) {
    /* Frees the local TLS state. It does not perform a graceful
     * TLS shutdown exchange with the peer. */
    SSL_set_shutdown(cl->ssl, SSL_SENT_SHUTDOWN | SSL_RECEIVED_SHUTDOWN);
    SSL_free(cl->ssl);
    cl->ssl = NULL;
  }

  const int      log_fd = cl->fd;
  const uint64_t log_conn_id = cl->conn_id;

  if (cl->fd >= 0) {
    if (close(cl->fd) < 0) {
      nob_log(ERROR, "[client=%" PRIu64 " fd=%d] close failed: %s", cl->conn_id,
              cl->fd, strerror(errno));

      if (result == FAITH_OK)
        result = FAITH_ERR_IO;
    }

    cl->fd = -1;
  }

  free(cl->out_buf);
  cl->out_buf = NULL;

  free(cl->in_buf);
  cl->in_buf = NULL;

  nob_log(INFO, "[client=%" PRIu64 " fd=%d] Closed client", log_conn_id,
          log_fd);

  free(cl);

  return result;
}

static faith_status_code_t server_send_envelope(struct server_state_t  *s,
                                                struct client_conn_t   *cl,
                                                const faith_envelope_t *envl);
static faith_status_code_t
server_disconnect_client(struct server_state_t *s, struct client_conn_t *cl,
                         faith_client_disconnect_reason_t reason,
                         faith_client_reconnect_policy_t  reconnect_policy,
                         uint64_t retry_after_ms, uint64_t banned_until_ms,
                         const char *message) {
  if (!s || !cl)
    return FAITH_ERR_INVALID;

  if (cl->closing)
    return FAITH_OK;

  faith_envl_stc_client_disconnect_t disconnect_envl = {0};

  disconnect_envl.reason = (uint32_t)reason;
  disconnect_envl.reconnect_policy = (uint32_t)reconnect_policy;
  disconnect_envl.retry_after_ms = retry_after_ms;
  disconnect_envl.banned_until_ms = banned_until_ms;

  if (message)
    snprintf(disconnect_envl.msg, sizeof(disconnect_envl.msg), "%s", message);

  uint8_t           body[FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_MAX];
  faith_body_size_t body_size = 0;

  {
    _FH_CHECK(faith_encode_client_disconnect_body(
        body, &body_size, sizeof(body), &disconnect_envl));
    if (_fh_rc != FAITH_OK) {
      /* We cannot produce the final protocol message, so close immediately */
      cl->closing = 1;
      return _fh_rc;
    }
  }

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_CLIENT_DISCONNECT;
  envl.recipient_id = cl->auth_id;
  envl.body = body;
  envl.body_size = body_size;

  {
    _FH_CHECK(server_send_envelope(s, cl, &envl));

    if (_fh_rc != FAITH_OK) {
      /* Sending failed. There is no useful recovery for this connection, so
       * close immediately. */
      cl->closing = 1;
      return _fh_rc;
    }
  }

  /* Stop accepting further application messages but allow the queued
   * DISCONNECT envelope to be written first. */
  cl->close_after_flush = 1;

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

  cl->ev_mask = mask;

  return epoll_ctl(epoll_fd, EPOLL_CTL_MOD, cl->fd, &ev);
}

void server_destroy(struct server_state_t *s) {
  if (!s)
    return;

  nob_log(INFO, "Destroying server context...");

  while (s->clients) {
    struct client_conn_t *cl = s->clients;
    close_client(s, &cl);
  }

  routing_destroy(&s->rt);

  if (s->listenfd >= 0) {
    if (s->epoll_fd >= 0) {
      if (epoll_ctl(s->epoll_fd, EPOLL_CTL_DEL, s->listenfd, NULL) < 0 &&
          errno != ENOENT) {
        nob_log(ERROR, "epoll delete listen FD failed: %s", strerror(errno));
      }
    }

    if (close(s->listenfd) < 0)
      nob_log(ERROR, "close() on listen FD failed: %s", strerror(errno));

    s->listenfd = -1;
  }

  if (s->epoll_fd >= 0) {
    if (close(s->epoll_fd) < 0)
      nob_log(ERROR, "close() epoll FD failed: %s", strerror(errno));

    s->epoll_fd = -1;
  }

  if (s->ssl_ctx) {
    SSL_CTX_free(s->ssl_ctx);
    s->ssl_ctx = NULL;
  }

  nob_log(INFO, "Destroyed server context.");
}

static faith_status_code_t server_decode_ping(const uint8_t *payload,
                                              size_t         payload_size,
                                              uint64_t      *ping_nonce,
                                              uint64_t      *server_time_ms) {

  const size_t ping_size = sizeof(uint64_t) * 2;

  if (!payload || !ping_nonce || !server_time_ms)
    return FAITH_ERR_INVALID;

  if (payload_size != ping_size)
    return FAITH_ERR_BAD_FRAME;

  *ping_nonce = faith_read_u64_be(payload);
  *server_time_ms = faith_read_u64_be(payload + sizeof(uint64_t));

  return FAITH_OK;
}

static faith_status_code_t
server_enqueue_input_bytes(struct client_conn_t *cl, const uint8_t *bytes,
                           size_t n_bytes, const struct server_cfg_t *cfg) {
  if (!cl || cl->closing || !cfg || (!bytes && n_bytes != 0))
    return FAITH_ERR_INVALID;

  if (cfg->verbose_logging) {
    nob_log(INFO,
            "[client=%" PRIu64 " fd=%d] Trying to enqueue %zu input bytes...",
            cl->conn_id, cl->fd, n_bytes);
  }

  if (n_bytes == 0) {
    if (cfg->verbose_logging) {
      nob_log(WARNING,
              "[client=%" PRIu64 " fd=%d] Tried to enqueue zero-length input",
              cl->conn_id, cl->fd);
    }

    return FAITH_OK;
  }

  if (cl->in_size > FAITH_MAX_CLIENT_IN_QUEUE ||
      n_bytes > FAITH_MAX_CLIENT_IN_QUEUE - cl->in_size)
    return FAITH_ERR_OVERFLOW;

  const size_t needed = cl->in_size + n_bytes;

  if (needed > cl->in_cap) {
    size_t new_cap = cl->in_cap ? cl->in_cap : 4096u;

    while (new_cap < needed) {
      if (new_cap >= FAITH_MAX_CLIENT_IN_QUEUE) {
        new_cap = FAITH_MAX_CLIENT_IN_QUEUE;
        break;
      }

      if (new_cap > SIZE_MAX / 2) {
        new_cap = needed;
        break;
      }

      new_cap *= 2;

      if (new_cap > FAITH_MAX_CLIENT_IN_QUEUE)
        new_cap = FAITH_MAX_CLIENT_IN_QUEUE;
    }

    if (new_cap < needed)
      return FAITH_ERR_OVERFLOW;

    uint8_t *p = realloc(cl->in_buf, new_cap);
    if (!p)
      return FAITH_ERR_NOMEM;

    cl->in_buf = p;
    cl->in_cap = new_cap;
  }

  memcpy(cl->in_buf + cl->in_size, bytes, n_bytes);
  cl->in_size = needed;

  if (cfg->verbose_logging) {
    nob_log(INFO,
            "[client=%" PRIu64 " fd=%d] Successfully enqueued %zu input bytes",
            cl->conn_id, cl->fd, n_bytes);
  }

  return FAITH_OK;
}

static faith_status_code_t
server_enqueue_output_bytes(struct client_conn_t *cl, const uint8_t *bytes,
                            size_t n_bytes, const struct server_cfg_t *cfg) {
  if (!cl || cl->closing || !cfg || (!bytes && n_bytes != 0))
    return FAITH_ERR_INVALID;

  if (cfg->verbose_logging) {
    nob_log(INFO,
            "[client=%" PRIu64 " fd=%d] Trying to enqueue %zu output bytes...",
            cl->conn_id, cl->fd, n_bytes);
  }

  if (n_bytes == 0) {
    if (cfg->verbose_logging) {
      nob_log(WARNING,
              "[client=%" PRIu64 " fd=%d] Tried to enqueue zero-length output",
              cl->conn_id, cl->fd);
    }

    return FAITH_OK;
  }

  if (cl->out_size > FAITH_MAX_CLIENT_OUT_QUEUE ||
      n_bytes > FAITH_MAX_CLIENT_OUT_QUEUE - cl->out_size)
    return FAITH_ERR_OVERFLOW;

  const size_t needed = cl->out_size + n_bytes;

  if (needed > cl->out_cap) {
    size_t new_cap = cl->out_cap ? cl->out_cap : 4096u;

    while (new_cap < needed) {
      if (new_cap >= FAITH_MAX_CLIENT_OUT_QUEUE) {
        new_cap = FAITH_MAX_CLIENT_OUT_QUEUE;
        break;
      }

      if (new_cap > SIZE_MAX / 2) {
        new_cap = needed;
        break;
      }

      new_cap *= 2;

      if (new_cap > FAITH_MAX_CLIENT_OUT_QUEUE)
        new_cap = FAITH_MAX_CLIENT_OUT_QUEUE;
    }

    if (new_cap < needed)
      return FAITH_ERR_OVERFLOW;

    uint8_t *p = realloc(cl->out_buf, new_cap);
    if (!p)
      return FAITH_ERR_NOMEM;

    cl->out_buf = p;
    cl->out_cap = new_cap;
  }

  memcpy(cl->out_buf + cl->out_size, bytes, n_bytes);
  cl->out_size = needed;

  if (cfg->verbose_logging) {
    nob_log(INFO,
            "[client=%" PRIu64 " fd=%d] Successfully enqueued %zu output bytes",
            cl->conn_id, cl->fd, n_bytes);
  }

  return FAITH_OK;
}

static faith_status_code_t
server_encode_pong(uint8_t out_buf[FAITH_MSG_PONG_PAYLOAD_SIZE], uint64_t nonce,
                   uint64_t sent_at_ms) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  _FH_CHECK_RETURN(faith_write_u64_be(out_buf, nonce));
  _FH_CHECK_RETURN(faith_write_u64_be(out_buf + sizeof(uint64_t), sent_at_ms));

  return FAITH_OK;
}

static faith_status_code_t
server_send_over_wire(struct server_state_t *s, struct client_conn_t *cl,
                      const uint8_t *payload, size_t payload_size,
                      faith_frame_msg_type_t msg_type) {
  if (!cl || cl->closing || !s || (!payload && payload_size != 0))
    return FAITH_ERR_INVALID;

  uint8_t *wire_data = NULL;
  size_t   wire_size = 0;

  // Allocates memory on pointer passed to <out_data>, which is <wire_data> here
  _FH_CHECK_RETURN(faith_encode_frame(msg_type, payload, payload_size,
                                      &wire_data, &wire_size));

  if (!wire_data || wire_size == 0) {
    free(wire_data);
    return FAITH_ERR_INVALID;
  }

  {
    _FH_CHECK(server_enqueue_output_bytes(cl, wire_data, wire_size, &s->cfg));

    if (_fh_rc != FAITH_OK) {
      if (_fh_rc == FAITH_ERR_OVERFLOW || _fh_rc == FAITH_ERR_NOMEM) {
        server_disconnect_client(
            s, cl, FAITH_DISCONNECT_MEMORY_LIMIT,
            FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
            "The client exceeded it's output data queue limit.");
      }
      return _fh_rc;
    }
  }

  free(wire_data);
  wire_data = NULL;

  // client now wants EPOLLOUT
  if (modify_client_ev_mask(s->epoll_fd, cl, cl->ev_mask | EPOLLOUT) < 0) {
    nob_log(ERROR, "[client=%" PRIu64 " fd=%d]  failed to enable EPOLLOUT: %s",
            cl->conn_id, cl->fd, strerror(errno));

    server_disconnect_client(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                             FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                             "Failed to enable EPOLLOUT for the client.");

    return FAITH_ERR_IO;
  }

  nob_log(INFO, "[client=%" PRIu64 " fd=%d] queued frame: %s (%zu bytes)",
          cl->conn_id, cl->fd, faith_frame_msg_name(msg_type), wire_size);

  return FAITH_OK;
}

static faith_status_code_t server_send_pong(struct server_state_t *s,
                                            struct client_conn_t  *cl,
                                            faith_frame_t         *frame) {
  if (!s || !cl || cl->closing || !frame)
    return FAITH_ERR_INVALID;

  uint64_t nonce;
  uint64_t client_sent_at_ms;

  /* 1. decode PING */
  _FH_CHECK_RETURN(server_decode_ping(frame->payload, frame->payload_size,
                                      &nonce, &client_sent_at_ms));

  nob_log(INFO,
          "[client=%" PRIu64 " fd=%i] server got PING: nonce=%" PRIu64
          ", client_sent_at_ms=%lu",
          cl->conn_id, cl->fd, nonce, client_sent_at_ms);

  /* 2. Send PONG over wire protocol */
  uint64_t server_sent_at_ms = faith_now_ms();

  uint8_t payload[FAITH_MSG_PONG_PAYLOAD_SIZE];

  _FH_CHECK_RETURN(server_encode_pong(payload, nonce, server_sent_at_ms));

  _FH_CHECK_RETURN(
      server_send_over_wire(s, cl, payload, sizeof(payload), FAITH_MSG_PONG));

  nob_log(
      INFO,
      "[client=%" PRIu64
      " fd=%i] Server sent PONG to client. nonce=%lu, server_sent_at_ms=%lu",
      cl->conn_id, cl->fd, nonce, server_sent_at_ms);

  return FAITH_OK;
}

static faith_status_code_t server_send_envelope(struct server_state_t  *s,
                                                struct client_conn_t   *cl,
                                                const faith_envelope_t *envl) {
  if (!s || !cl || cl->closing || !envl)
    return FAITH_ERR_INVALID;

  size_t   cap = FAITH_ENVL_HEADER_SIZE + envl->body_size;
  uint8_t *payload = malloc(cap);
  if (!payload)
    return FAITH_ERR_NOMEM;

  size_t payload_size = 0;

  faith_status_code_t _fh_result = FAITH_OK;
  _FH_CHECK_DEFER(faith_encode_envelope(payload, &payload_size, cap, envl));

  _FH_CHECK_DEFER(
      server_send_over_wire(s, cl, payload, payload_size, FAITH_MSG_ENVL));

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server enqueued envelope: type=%s body_size=%u",
          cl->conn_id, cl->fd, faith_envelope_name(envl->type),
          envl->body_size);

defer:
  free(payload);
  return _fh_result;
}

static faith_status_code_t
server_send_envelope_or_close(struct server_state_t  *s,
                              struct client_conn_t   *cl,
                              const faith_envelope_t *envl) {
  if (!s || !cl || !envl)
    return FAITH_ERR_INVALID;

  faith_status_code_t rc = server_send_envelope(s, cl, envl);

  if (rc != FAITH_OK) {
    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Failed to send %s: %s",
            cl->conn_id, cl->fd, faith_envelope_name(envl->type),
            faith_status_code_name(rc));

    char buf[FAITH_MAX_CLIENT_DISCONNECT_MSG];
    snprintf(buf, sizeof(buf), "Failed to send %s envelope to client.",
             faith_envelope_name(envl->type));
    server_disconnect_client(s, cl, FAITH_DISCONNECT_BAD_PROTOCOL,
                             FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0, buf);
  }

  return rc;
}

static faith_status_code_t
server_route_envelope(struct server_state_t *s, struct client_conn_t *cl_sender,
                  const faith_client_id_t *recipient_auth_id,
                  const faith_envelope_t  *envl) {
  if (!s || !cl_sender || cl_sender->closing || !recipient_auth_id || !envl)
    return FAITH_ERR_INVALID;

  if (cl_sender->state != CLIENT_OPEN)
    return FAITH_ERR_BAD_ENVELOPE;

  if (!cl_sender->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  if (envl->body_size > 0 && !envl->body)
    return FAITH_ERR_INVALID;

  struct client_route_device_t *recipient_devices = NULL;

  _FH_CHECK(routing_get_devices(&s->rt, recipient_auth_id, &recipient_devices));

  if (_fh_rc != FAITH_OK) {
    if (_fh_rc == FAITH_ERR_NOT_FOUND) {
      char recipient_auth_id_hex[33];

      _FH_CHECK_RETURN(
          faith_id128_to_hex(recipient_auth_id->bytes, recipient_auth_id_hex));

      nob_log(INFO,
              "[client=%" PRIu64 " fd=%i] Envelope %s: Recipient "
              "(auth_id: %s) is offline; Not routing envelope.",
              cl_sender->conn_id, cl_sender->fd,
              faith_envelope_name(envl->type), recipient_auth_id_hex);

      return FAITH_OK;
    }

    return _fh_rc;
  }

  char recipient_auth_id_hex[33];

  _FH_CHECK_RETURN(
      faith_id128_to_hex(recipient_auth_id->bytes, recipient_auth_id_hex));

  for (ptrdiff_t i = 0; i < hmlen(recipient_devices); i++) {
    if (!recipient_devices[i].value || !recipient_devices[i].value->conn) {
      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%i] Envelope %s: Routing table contains an invalid "
              "device entry for recipient (auth_id: %s).",
              cl_sender->conn_id, cl_sender->fd,
              faith_envelope_name(envl->type), recipient_auth_id_hex);
      continue;
    }

    struct client_conn_t *recipient = recipient_devices[i].value->conn;

    if (recipient->closing || recipient->state != CLIENT_OPEN)
      continue;

    char recipient_device_id_hex[33];

    _FH_CHECK_RETURN(faith_id128_to_hex(recipient->device_id.bytes,
                                        recipient_device_id_hex));

    if (!recipient->authorized) {
      nob_log(ERROR,
              "[client=%" PRIu64 " fd=%i] Envelope %s: Recipient device "
              "(auth_id: %s, device_id: %s) is not authorized; "
              "the envelope will not be routed to it.",
              cl_sender->conn_id, cl_sender->fd,
              faith_envelope_name(envl->type), recipient_auth_id_hex,
              recipient_device_id_hex);
      continue;
    }

    faith_envelope_t routing_envl = *envl;
    routing_envl.sender_id = cl_sender->auth_id;
    routing_envl.recipient_id = *recipient_auth_id;

    _FH_CHECK(server_send_envelope_or_close(s, recipient, &routing_envl));

    if (_fh_rc != FAITH_OK) {
      nob_log(
          ERROR,
          "[client=%" PRIu64 " fd=%i] Envelope %s: Failed to route envelope to "
          "recipient device (auth_id: %s, device_id: %s).",
          cl_sender->conn_id, cl_sender->fd, faith_envelope_name(envl->type),
          recipient_auth_id_hex, recipient_device_id_hex);
      continue;
    }

    nob_log(INFO,
            "[client=%" PRIu64
            " fd=%i] Envelope %s: Routed envelope to recipient "
            "device (auth_id: %s, device_id: %s).",
            cl_sender->conn_id, cl_sender->fd, faith_envelope_name(envl->type),
            recipient_auth_id_hex, recipient_device_id_hex);
  }

  return FAITH_OK;
}

static faith_status_code_t
server_cancel_pending_device_link(struct server_state_t *s,
                                  struct client_conn_t  *requesting_cl) {
  if (!s || !requesting_cl)
    return FAITH_ERR_INVALID;

  if (requesting_cl->state != CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE)
    return FAITH_OK;

  struct client_temporary_handshake_params_t *params =
      &requesting_cl->temp_handshake_params;

  struct client_route_device_t *devices = NULL;
  faith_status_code_t           rc =
      routing_get_devices(&s->rt, &params->sender_auth_id, &devices);

  if (rc == FAITH_ERR_NOT_FOUND)
    return FAITH_OK;

  if (rc != FAITH_OK)
    return rc;

  faith_status_code_t result = FAITH_OK;
  ptrdiff_t           n_devices = hmlen(devices);

  for (ptrdiff_t i = 0; i < n_devices; ++i) {
    if (!devices[i].value || !devices[i].value->conn) {
      nob_log(ERROR, "Routing table contains an invalid device entry");
      continue;
    }

    struct client_conn_t *authorized_cl = devices[i].value->conn;

    if (authorized_cl->pending_device_link_conn != requesting_cl)
      continue;

    if (authorized_cl->authorized && !authorized_cl->closing &&
        authorized_cl->state == CLIENT_OPEN) {
      faith_envelope_t envl = {0};
      envl.type = FAITH_ENVELOPE_DEVICE_LINK_CANCELLED;
      envl.recipient_id = authorized_cl->auth_id;
      envl.body = NULL;
      envl.body_size = 0;

      rc = server_send_envelope_or_close(s, authorized_cl, &envl);

      if (rc != FAITH_OK && result == FAITH_OK)
        result = rc;
    }

    free(authorized_cl->pending_device_link_req);
    authorized_cl->pending_device_link_req = NULL;
    authorized_cl->pending_device_link_conn = NULL;
  }

  return result;
}

static faith_status_code_t
server_accept_hello(struct server_state_t *s, struct client_conn_t *cl,
                    const faith_client_id_t *sender_id,
                    const faith_device_id_t *device_id) {
  if (!s || !cl || cl->closing || !device_id)
    return FAITH_ERR_INVALID;

  if (!cl->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  /* Send HELLO_OK evelope back to client */
  faith_envelope_t hello_ok_envl = {0};
  hello_ok_envl.type = FAITH_ENVELOPE_HELLO_OK;
  hello_ok_envl.recipient_id = *sender_id;

  _FH_CHECK_RETURN(server_send_envelope_or_close(s, cl, &hello_ok_envl));

  cl->auth_id = *sender_id;
  cl->device_id = *device_id;

  set_client_state(s, cl, CLIENT_OPEN);

  char auth_id_hex[33];
  char device_id_hex[33];

  faith_status_code_t _fh_result = FAITH_OK;

  _FH_CHECK_RETURN(faith_id128_to_hex(cl->auth_id.bytes, auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(cl->device_id.bytes, device_id_hex));

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server accepted HELLO (auth id: %s, device id: %s)",
          cl->conn_id, cl->fd, auth_id_hex, device_id_hex);

  return _fh_result;
}

static faith_status_code_t
server_handle_hello(struct server_state_t *s, struct client_conn_t *cl,
                    const faith_envelope_t *hello_envl) {
  // HELLO {
  // header: {
  // sender_id: auth_id
  // }
  //  body: {
  //    device_id,
  //    public_key,
  //    client_nonce
  //  }
  // }
  //
  if (!cl || cl->closing || !hello_envl)
    return FAITH_ERR_INVALID;
  if (hello_envl->type != FAITH_ENVELOPE_HELLO)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_WAIT_FOR_HELLO) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got invalid HELLO from client.",
            cl->conn_id, cl->fd);
    return FAITH_ERR_BAD_ENVELOPE;
  }

  /* Reading body from envelope */

  if (hello_envl->body_size != FAITH_ENVL_HELLO_BODY_SIZE || !hello_envl->body)
    return FAITH_ERR_BAD_FRAME;

  uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];

  /* 1. Deserialize device ID */
  size_t            offset = 0;
  faith_device_id_t device_id;
  memcpy(device_id.bytes, hello_envl->body, sizeof(device_id.bytes));

  offset += sizeof(device_id.bytes);

  /* 2. Deserialize public key */
  memcpy(public_key, hello_envl->body + offset, sizeof(public_key));

  offset += sizeof(public_key);

  /* 3. Deserialize nonce */
  uint64_t nonce = faith_read_u64_be(hello_envl->body + offset);

  uint64_t server_nonce;
  _FH_CHECK_RETURN(
      faith_random_bytes((uint8_t *)&server_nonce, sizeof(server_nonce)));

  /* Construct temporary handshake parameters for client identity evaluation */
  struct client_temporary_handshake_params_t *params =
      &cl->temp_handshake_params;

  params->sender_auth_id = hello_envl->sender_id;
  params->device_id = device_id;

  memcpy(params->public_key, public_key, sizeof(params->public_key));

  params->server_nonce = server_nonce;
  params->nonce = nonce;

  /* Send CHALLENGE envelope */

  faith_envelope_t challenge_envl;
  challenge_envl.type = FAITH_ENVELOPE_CHALLENGE;
  challenge_envl.recipient_id = hello_envl->sender_id;

  uint8_t body[FAITH_ENVL_HELLO_CHALLENGE_BODY_SIZE];
  _FH_CHECK_RETURN(faith_write_u64_be(body, server_nonce));

  challenge_envl.body = body;
  challenge_envl.body_size = (faith_body_size_t)sizeof(body);

  _FH_CHECK_RETURN(server_send_envelope_or_close(s, cl, &challenge_envl));

  set_client_state(s, cl, CLIENT_WAIT_FOR_CHALLENGE_RESPONSE);

  return FAITH_OK;
}

static faith_status_code_t
server_send_device_auth_pending(struct server_state_t *s,
                                struct client_conn_t  *recipient_cl) {
  if (!s || !recipient_cl || recipient_cl->closing)
    return FAITH_ERR_INVALID;

  faith_envelope_t device_auth_pending_envl = {0};
  device_auth_pending_envl.type = FAITH_ENVELOPE_DEVICE_AUTH_PENDING;
  _FH_CHECK_RETURN(server_send_envelope_or_close(s, recipient_cl,
                                                 &device_auth_pending_envl));

  return FAITH_OK;
}

static faith_status_code_t server_send_device_link_request(
    struct server_state_t *s, struct client_conn_t *recipient_cl,
    struct client_conn_t *request_cl, const faith_client_id_t *auth_id,
    const uint8_t public_key_new_device[FAITH_ED25519_PUBLIC_KEY_SIZE],
    const faith_device_id_t           *new_device_id,
    faith_envl_stc_device_link_req_t **o_req) {
  if (!s || !auth_id || !public_key_new_device || !new_device_id ||
      !recipient_cl) {
    return FAITH_ERR_INVALID;
  }
  if (recipient_cl->state != CLIENT_OPEN)
    return FAITH_ERR_INVALID;
  if (request_cl->closing || recipient_cl->closing)
    return FAITH_ERR_INVALID;

  if (!recipient_cl->authorized) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->device_id.bytes, device_id_hex));
    nob_log(ERROR,
            "Not sending device link request to "
            "device with device_id: %s (auth_id: %s). Client connection is "
            "not authorized.",
            device_id_hex, auth_id_hex);

    return FAITH_ERR_UNAUTHORIZED;
  }

  if (recipient_cl->pending_device_link_req != NULL) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->device_id.bytes, device_id_hex));
    nob_log(ERROR,
            "Not sending device link request to "
            "device with device_id: %s (auth_id: %s). Another device link "
            "request is already pending.",
            device_id_hex, auth_id_hex);

    server_disconnect_client(
        s, request_cl, FAITH_DISCONNECT_DEVICE_REJECTED,
        FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
        "Another device-link request is already pending on this account.");

    return FAITH_OK;
  }

  /* Allocate device link request */
  faith_envl_stc_device_link_req_t *req = calloc(1, sizeof(*req));
  if (!req)
    return FAITH_ERR_NOMEM;

  req->device_id_new = *new_device_id;

  memcpy(req->public_key_new_device, public_key_new_device,
         FAITH_ED25519_PUBLIC_KEY_SIZE);

  req->auth_id = *auth_id;

  _FH_CHECK_RETURN(faith_random_bytes(req->code, sizeof(req->code)));

  req->expires_at_ms =
      faith_now_ms() + FAITH_DEVICE_LINK_REQ_EXPIRATION_TIME_MS;

  /* Serialize device link reqest */

  uint8_t           body[FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE];
  faith_body_size_t body_size = 0;
  _FH_CHECK_RETURN(
      faith_encode_device_link_req_body(body, &body_size, sizeof(body), req));

  faith_envelope_t device_link_req_envl = {0};
  device_link_req_envl.body = body;
  device_link_req_envl.body_size = body_size;
  device_link_req_envl.recipient_id = *auth_id;
  device_link_req_envl.type = FAITH_ENVELOPE_DEVICE_LINK_REQUEST;

  _FH_CHECK_RETURN(
      server_send_envelope_or_close(s, recipient_cl, &device_link_req_envl));

  *o_req = req;

  return FAITH_OK;
}

static faith_status_code_t server_handle_newly_joined_device(
    struct server_state_t *s, struct client_conn_t *cl,
    const struct client_temporary_handshake_params_t *params) {
  if (!s || !cl || cl->closing || !params)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_WAIT_FOR_CHALLENGE_RESPONSE)
    return FAITH_ERR_BAD_ENVELOPE;

  if (s->cfg.verbose_logging) {
    char cl_auth_id_hex[33];
    char new_device_id_hex[33];
    _FH_CHECK_RETURN(
        faith_id128_to_hex(params->sender_auth_id.bytes, cl_auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(params->device_id.bytes, new_device_id_hex));

    nob_log(
        INFO,
        "[client=%" PRIu64
        " fd=%i] Handling newly joined device (device_id=%s) for auth_id=%s.",
        cl->conn_id, cl->fd, new_device_id_hex, cl_auth_id_hex);
  }

  struct client_route_device_t *authorized_devices = NULL;

  _FH_CHECK_RETURN(routing_get_devices(&s->rt, &params->sender_auth_id,
                                       &authorized_devices));

  ptrdiff_t n_authorized_devices = hmlen(authorized_devices);
  if (!authorized_devices || n_authorized_devices < 1) {
    return FAITH_ERR_INVALID;
  }

  /* Send device authorization request to every already registered device
   for that auth ID */
  for (ptrdiff_t i = 0; i < n_authorized_devices; i++) {
    if (!authorized_devices[i].value || !authorized_devices[i].value->conn) {
      nob_log(ERROR, "Routing table contains an invalid device entry");
      continue;
    }
    struct client_conn_t *authorized_cl = authorized_devices[i].value->conn;
    if (!authorized_cl->authorized || authorized_cl->closing ||
        authorized_cl->state != CLIENT_OPEN)
      continue;

    faith_envl_stc_device_link_req_t *req = NULL;
    _FH_CHECK(server_send_device_link_request(
        s, authorized_cl, cl, &params->sender_auth_id, params->public_key,
        &params->device_id, &req));

    if (_fh_rc != FAITH_OK || !req) {
      char sender_auth_id_hex[33];
      _FH_CHECK_RETURN(
          faith_id128_to_hex(params->sender_auth_id.bytes, sender_auth_id_hex));
      char device_id_hex[33];
      _FH_CHECK_RETURN(
          faith_id128_to_hex(params->device_id.bytes, device_id_hex));
      nob_log(ERROR,
              "Failed to send device link request to authorized "
              "device session with device_id: %s (auth_id: %s)",
              device_id_hex, sender_auth_id_hex);

      return _fh_rc;
    }

    /* Set pending device link request of receiving client connection. We also
     * store the client connection that sent the device link request. */
    authorized_cl->pending_device_link_req = req;
    authorized_cl->pending_device_link_conn = cl;
  }

  /* Send DEVICE_AUTH_PENDING to the connection that requested the
   * new device */
  _FH_CHECK_RETURN(server_send_device_auth_pending(s, cl));
  set_client_state(s, cl, CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE);

  return FAITH_OK;
}

static faith_status_code_t server_authorize_client(
    struct server_state_t *s, struct client_conn_t *cl,
    const faith_client_id_t *auth_id, const faith_device_id_t *device_id,
    uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE], int register_session) {
  if (!s || !cl || cl->closing || !auth_id || !device_id || !public_key)
    return FAITH_ERR_INVALID;

  cl->authorized = 1;

  /* Accept the hello */
  _FH_CHECK_RETURN(server_accept_hello(s, cl, auth_id, device_id));

  if (register_session) {
    /* register client session */
    _FH_CHECK_RETURN(routing_register_session(&s->rt, &cl->auth_id,
                                              &cl->device_id, cl, public_key));
  }

  char cl_auth_id_hex[33];
  char cl_device_id_hex[33];
  _FH_CHECK_RETURN(faith_id128_to_hex(cl->auth_id.bytes, cl_auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(cl->device_id.bytes, cl_device_id_hex));

  nob_log(INFO,
          "[client=%" PRIu64 " fd=%i] Client passed authorization for "
          "requested routing session. (auth_id=%s, device_id=%s)",
          cl->conn_id, cl->fd, cl_auth_id_hex, cl_device_id_hex);

  return FAITH_OK;
}

static faith_status_code_t server_handle_challenge_response(
    struct server_state_t *s, struct client_conn_t *cl,
    const faith_envelope_t *challenge_response_envl) {
  if (!s || !cl || cl->closing || !challenge_response_envl)
    return FAITH_ERR_INVALID;

  if (challenge_response_envl->type != FAITH_ENVELOPE_CHALLENGE_RESPONSE) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Invalid envelope type. Expected "
            "FAITH_ENVELOPE_CHALLENGE_RESPONSE, got %s",
            cl->conn_id, cl->fd,
            faith_envelope_name(challenge_response_envl->type));
    return FAITH_ERR_INVALID;
  }

  if (cl->state != CLIENT_WAIT_FOR_CHALLENGE_RESPONSE) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got invalid CHALLENGE_RESPONSE from client.",
            cl->conn_id, cl->fd);
    return FAITH_ERR_BAD_ENVELOPE;
  }

  struct client_temporary_handshake_params_t *params =
      &cl->temp_handshake_params;

  if (!faith_client_id_equal(challenge_response_envl->sender_id,
                             params->sender_auth_id)) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got CHALLENGE_RESPONSE from invalid client.",
            cl->conn_id, cl->fd);
    return FAITH_ERR_INVALID;
  }

  if (!challenge_response_envl->body ||
      challenge_response_envl->body_size != FAITH_ED25519_SIGNATURE_SIZE) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got invalid CHALLENGE_RESPONSE envelope contents.",
            cl->conn_id, cl->fd);
    return FAITH_ERR_INVALID;
  }

  uint8_t *verification_public_key = NULL;

  /* Looking for the mapped session of the auth ID, device ID pair */
  struct client_device_session_data_t *sess = NULL;
  faith_status_code_t                  sess_rc = routing_get_session(
      &s->rt, &params->sender_auth_id, &params->device_id, &sess);

  /* Failure finding session, different from FAITH_ERR_NOT_FOUND */
  if (sess_rc == FAITH_ERR_INVALID) {
    char sender_auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(
        faith_id128_to_hex(params->sender_auth_id.bytes, sender_auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(params->device_id.bytes, device_id_hex));
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Failed to get routing session (auth_id=%s, device_id=%s).",
            cl->conn_id, cl->fd, sender_auth_id_hex, device_id_hex);
    return sess_rc;
  }

  int handle_newly_joined_device = 0;

  if (sess_rc == FAITH_ERR_NOT_FOUND) {
    /* Session not registered yet, use sent public key for verification */
    verification_public_key = params->public_key;
  } else if (sess_rc == FAITH_OK) {
    /* Session already registered */
    if (!sess) {
      /* This means the auth ID is already registered but this is a new
       * device that wants to join. */
      handle_newly_joined_device = 1;
      verification_public_key = params->public_key;
    } else {
      /* Check if the sent public key and the public key of the registered
       * identity match */
      if (memcmp(params->public_key, sess->ident.public_key,
                 FAITH_ED25519_PUBLIC_KEY_SIZE) != 0) {

        char sender_auth_id_hex[33];
        char device_id_hex[33];
        _FH_CHECK_RETURN(faith_id128_to_hex(params->sender_auth_id.bytes,
                                            sender_auth_id_hex));
        _FH_CHECK_RETURN(
            faith_id128_to_hex(params->device_id.bytes, device_id_hex));
        nob_log(ERROR,
                "[client=%" PRIu64 " fd=%i] Rejected requested routing session "
                "(auth_id=%s, device_id=%s). "
                "Invalid public key sent.",
                cl->conn_id, cl->fd, sender_auth_id_hex, device_id_hex);
        return FAITH_ERR_UNAUTHORIZED;
      }

      /* Passed, fine. Use public key of the already verified session identity
       * for verification. We do not trust the sent public key. */
      verification_public_key = sess->ident.public_key;
    }
  } else {
    /* Propagate unexpected routing errors. */
    return sess_rc;
  }

  if (!verification_public_key) {
    return FAITH_ERR_INVALID;
  }

  uint8_t client_signature[FAITH_ED25519_SIGNATURE_SIZE] = {0};
  memcpy(client_signature, challenge_response_envl->body,
         sizeof(client_signature));

  /* Construct message buffer for signature generation
   *    Message Buffer {
   *      sender_auth_id,
   *      verification_public_key,
   *      client_nonce,
   *      server_nonce,
   *      device_id
   *    } */

  faith_signature_hello_handshake_t sign_msg = {0};

  sign_msg.auth_id = params->sender_auth_id;
  sign_msg.device_id = params->device_id;

  memcpy(sign_msg.public_key, verification_public_key,
         FAITH_ED25519_PUBLIC_KEY_SIZE);

  sign_msg.client_nonce = params->nonce;
  sign_msg.server_nonce = params->server_nonce;

  size_t msg_size = 0;
  uint8_t msg_buf[FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE];
  {
    _FH_CHECK(faith_gen_sign_buf_hello_handshake(msg_buf, &msg_size, sizeof(msg_buf),
                                                 &sign_msg));
    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%i] Failed to generate signing message buffer",
              cl->conn_id, cl->fd);

      return _fh_rc;
    }
  }

  /* Verify the signature */
  faith_status_code_t verification_rc = FAITH_ERR_UNAUTHORIZED;
  {
    _FH_CHECK(faith_verify_signature_raw_pubkey(
        verification_public_key, msg_buf, sizeof(msg_buf), client_signature,
        sizeof(client_signature)));

    verification_rc = _fh_rc;
  }

  int authorized = verification_rc == FAITH_OK;
  if (!authorized) {
    goto reject;
  }

  /* =============================== */
  /* Client passed authorization */
  /* =============================== */

  if (handle_newly_joined_device) {
    _FH_CHECK_RETURN(server_handle_newly_joined_device(s, cl, params));
    return FAITH_OK;
  }

  _FH_CHECK(server_authorize_client(s, cl, &params->sender_auth_id,
                                    &params->device_id, verification_public_key,
                                    sess == NULL));
  return _fh_rc;
reject: {

  /* =============================== */
  /* Client failed authorization */
  /* =============================== */

  char sender_auth_id_hex[33];
  char device_id_hex[33];
  _FH_CHECK_RETURN(
      faith_id128_to_hex(params->sender_auth_id.bytes, sender_auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(params->device_id.bytes, device_id_hex));
  nob_log(ERROR,
          "[client=%" PRIu64
          " fd=%i] Client failed authorization for requested routing session "
          "(auth_id=%s, device_id=%s). ",
          cl->conn_id, cl->fd, sender_auth_id_hex, device_id_hex);
  return FAITH_ERR_UNAUTHORIZED;
}
}

static faith_status_code_t
server_handle_device_link_response(struct server_state_t  *s,
                                   struct client_conn_t   *cl,
                                   const faith_envelope_t *response_envl) {
  if (!s || !cl || cl->closing || !response_envl)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_OPEN)
    return FAITH_ERR_BAD_ENVELOPE;

  faith_status_code_t _fh_result = FAITH_OK;
  int                 blame_responder = 0;

  faith_envl_stc_device_link_req_t *req = cl->pending_device_link_req;
  struct client_conn_t             *req_cl = cl->pending_device_link_conn;

  if (!req || !req_cl) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got %s but there is no "
            "device link request pending. Rejecting envelope.",
            cl->conn_id, cl->fd, faith_envelope_name(response_envl->type));
    return FAITH_ERR_INVALID;
  }

  if (response_envl->type != FAITH_ENVELOPE_DEVICE_AUTH_APPROVE &&
      response_envl->type != FAITH_ENVELOPE_DEVICE_AUTH_DENY) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Invalid envelope type. Expected "
            "FAITH_ENVELOPE_DEVICE_AUTH_APPROVE or "
            "FAITH_ENVELOPE_DEVICE_AUTH_DENY, got %s",
            cl->conn_id, cl->fd, faith_envelope_name(response_envl->type));
    return FAITH_ERR_INVALID;
  }

  if (!response_envl->body ||
      response_envl->body_size !=
          sizeof(faith_envl_cts_device_link_response_t)) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got invalid %s envelope contents.",
            cl->conn_id, cl->fd, faith_envelope_name(response_envl->type));
    return FAITH_ERR_INVALID;
  }

  if (!req_cl || req_cl->closing) {
    char req_auth_id_hex[33];
    char req_device_id_hex[33];
    _FH_CHECK_RETURN(faith_id128_to_hex(req->auth_id.bytes, req_auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(req->device_id_new.bytes, req_device_id_hex));

    nob_log(INFO,
            "[client=%" PRIu64 " fd=%i] Client that requested"
            "their device (device_id=%s) to be linked to auth_id=%s has "
            "already been closed.",
            cl->conn_id, cl->fd, req_auth_id_hex, req_device_id_hex);
    return FAITH_OK;
  }

  if (!cl->authorized) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(faith_id128_to_hex(cl->auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(faith_id128_to_hex(cl->device_id.bytes, device_id_hex));

    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got unauthorized %s"
            "from client. Client (auth_id=%s, device_id=%s) is not authorized.",
            cl->conn_id, cl->fd, faith_envelope_name(response_envl->type),
            auth_id_hex, device_id_hex);

    /* Return UNAUTHORIZED without rejecting/closing the client connection that
     * requested the device link. We are protecting the pending client
     * connection here. */
    return FAITH_ERR_UNAUTHORIZED;
  }

  if (cl->state != CLIENT_OPEN) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got invalid %s"
            "from client. Authorizing client is not in OPEN state.",
            cl->conn_id, cl->fd, faith_envelope_name(response_envl->type));

    /* Return INVALID without rejecting/closing the client connection that
     * requested the device link. We are protecting the pending client
     * connection here. */
    return FAITH_ERR_INVALID;
  }

  faith_envl_cts_device_link_response_t response = {0};
  _FH_CHECK_RETURN(faith_decode_device_link_response_body(
      response_envl->body, response_envl->body_size, &response));

  if (!faith_device_id_equal(response.device_id_new, req->device_id_new)) {
    nob_log(
        ERROR,
        "[client=%" PRIu64 " fd=%i] Server got %s but the sent"
        "device_id does not match the device_id that requested the approval.",
        cl->conn_id, cl->fd, faith_envelope_name(response_envl->type));

    /* Return INVALID without rejecting/closing the client connection that
     * requested the device link. */
    return FAITH_ERR_INVALID;
  }

  if (faith_now_ms() > cl->pending_device_link_req->expires_at_ms) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got %s but the "
            "link request has already expired.",
            cl->conn_id, cl->fd, faith_envelope_name(response_envl->type));

    /* Reject the requesting client conection if the request has expired but
     * keep the responding one open. */
    _FH_RETURN_DEFER(FAITH_ERR_EXPIRED);
  }

  /* Construct message buffer for signature generation
   *    Message Buffer {
   *      auth_id,
   *      device_id_new,
   *      public_key_new_device,
   *      code,
   *      expires_at_ms,
   *      device_id_approving (device ID of the responding device)
   *    } */

  faith_signature_device_link_response_t sign_msg = {0};
  sign_msg.auth_id = req->auth_id;
  sign_msg.device_id_new = req->device_id_new;

  memcpy(sign_msg.public_key_new_device, req->public_key_new_device,
         FAITH_ED25519_PUBLIC_KEY_SIZE);
  memcpy(sign_msg.code, req->code, sizeof(req->code));

  sign_msg.expires_at_ms = req->expires_at_ms;
  sign_msg.device_id_responding = cl->device_id;

  sign_msg.type = response_envl->type == FAITH_ENVELOPE_DEVICE_AUTH_APPROVE
                      ? FAITH_DEVICE_LINK_APPROVE
                      : FAITH_DEVICE_LINK_DENY;

  size_t msg_size = 0;
  uint8_t msg_buf[FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE];
  {
    blame_responder = 1;
    _FH_CHECK(faith_gen_sign_buf_device_link_response(msg_buf, &msg_size, sizeof(msg_buf),
                                                      &sign_msg));

    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR,
              "Failed to generate %s signing "
              "message buffer.",
              faith_envelope_name(response_envl->type));

      blame_responder = 1;
      _FH_RETURN_DEFER(_fh_rc);
    }
  }

  struct client_device_session_data_t *sess = NULL;
  {
    _FH_CHECK(routing_get_session(&s->rt, &cl->auth_id, &cl->device_id, &sess));
    /* We specifically need routing_get_session() to return FAITH_OK. This is
     * returned only if <cl->auth_id> is a registered client_route_user_t
     * and <cl->device_id> is a registered client_route_device_t of that user.
     * */
    if (_fh_rc != FAITH_OK) {
      _fh_result = _fh_rc;
      /* Reject the requesting client conection if the authorized connection
       * does not actually have an authorized session. */

      blame_responder = 1;
      _FH_RETURN_DEFER(FAITH_ERR_UNAUTHORIZED);
    }
  }

  /* This means cl->auth_id is registered but cl->device_id is not, effectively
   * telling us that the client connection is not yet authorized. Because we
   * checked cl->authorized above, this should never happen with correct
   * behaviour.*/
  if (!sess) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got %s but client connection that "
            "sent the envelope does not have registered session data. "
            "However, the client connection IS authorized, so there is "
            "probably a deeper issue.",
            cl->conn_id, cl->fd, faith_envelope_name(response_envl->type));
    return FAITH_ERR_INVALID;
  }

  /* Verify the signature */
  faith_status_code_t verification_rc = FAITH_ERR_UNAUTHORIZED;
  {
    _FH_CHECK(faith_verify_signature_raw_pubkey(
        sess->ident.public_key, msg_buf, sizeof(msg_buf),
        response.signature_response, sizeof(response.signature_response)));

    verification_rc = _fh_rc;
  }

  int authorized = verification_rc == FAITH_OK;
  if (!authorized) {
    _FH_RETURN_DEFER(verification_rc == FAITH_ERR_NOT_EQUAL
                         ? FAITH_ERR_UNAUTHORIZED
                         : verification_rc);
  }

  /* ======================================== */
  /* Client proved their legitimacy to us. */
  /* ======================================== */

  faith_status_code_t rc = FAITH_OK;

  switch (response_envl->type) {
  case FAITH_ENVELOPE_DEVICE_AUTH_DENY: {
    _FH_CHECK(server_disconnect_client(
        s, req_cl, FAITH_DISCONNECT_DEVICE_REJECTED,
        FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
        "The device has been rejected by the account owner."));
    if (_fh_rc != FAITH_OK)
      rc = _fh_rc;
    break;
  }
  case FAITH_ENVELOPE_DEVICE_AUTH_APPROVE:
    _FH_CHECK_DEFER(server_authorize_client(s, req_cl, &req->auth_id,
                                            &req->device_id_new,
                                            req->public_key_new_device, 1));
    break;
  default:
    return FAITH_ERR_UNREACHABLE;
  }

  struct client_route_device_t *authorized_devices = NULL;

  {
    _FH_CHECK(routing_get_devices(&s->rt, &cl->auth_id, &authorized_devices));
    if (_fh_rc != FAITH_OK)
      rc = _fh_rc;
  }

  ptrdiff_t n_authorized_devices = hmlen(authorized_devices);

  /* Route the response to all other authorized devices
    for that auth ID */
  for (ptrdiff_t i = 0; i < n_authorized_devices; i++) {
    if (!authorized_devices[i].value || !authorized_devices[i].value->conn) {
      nob_log(ERROR, "Routing table contains an invalid device entry");
      continue;
    }

    struct client_conn_t *authorized_cl = authorized_devices[i].value->conn;

    if (authorized_cl->pending_device_link_conn != req_cl)
      continue;

    if (authorized_cl->authorized && !authorized_cl->closing &&
        authorized_cl->state == CLIENT_OPEN) {
      faith_envelope_t ack_envl = {0};
      ack_envl.type = FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_ACK;
      ack_envl.recipient_id = authorized_cl->auth_id;

      _FH_CHECK(server_send_envelope_or_close(s, authorized_cl, &ack_envl));
    }

    free(authorized_cl->pending_device_link_req);
    authorized_cl->pending_device_link_req = NULL;
    authorized_cl->pending_device_link_conn = NULL;
  }

  return rc;

defer: {
  /* ================================================= */
  /* This path punishes the pending client that
   * requested to be linked to this auth_id. */
  /* ================================================= */

  /* Copies for logging */
  faith_client_id_t client_id = req->auth_id;
  faith_device_id_t device_id_new = req->device_id_new;

  /* Remove the pending device link request from <cl> */
  free(cl->pending_device_link_req);
  cl->pending_device_link_req = NULL;
  cl->pending_device_link_conn = NULL;

  /* Close the connection that requested the device link: <req_cl> */
  char buf[FAITH_MAX_CLIENT_DISCONNECT_MSG];
  snprintf(buf, sizeof(buf),
           "Client failed device-link authorization. (Error: %s)",
           faith_status_code_name(_fh_result));

  // Don't propagate status code
  _FH_CHECK(server_disconnect_client(s, req_cl, FAITH_DISCONNECT_AUTH_FAILED,
                                     FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
                                     buf));

  char auth_id_hex[33];
  char device_id_hex[33];
  _FH_CHECK_RETURN(faith_id128_to_hex(client_id.bytes, auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(device_id_new.bytes, device_id_hex));

  nob_log(ERROR,
          "[client=%" PRIu64 " fd=%i] Client connection"
          "(auth_id=%s, device_id=%s) failed authorization for device link "
          "request; Error %s. Device with device_id=%s will not be linked to "
          "auth_id=%s. "
          "Closing connection. ",
          req_cl->conn_id, req_cl->fd, auth_id_hex, device_id_hex,
          faith_status_code_name(_fh_result), device_id_hex, auth_id_hex);

  return blame_responder ? _fh_result : FAITH_OK;
}
}

static faith_status_code_t
server_handle_msg_request(struct server_state_t *s, struct client_conn_t *cl,
                          const faith_envelope_t *req_envl) {
  if (!s || !cl || cl->closing || !req_envl)
    return FAITH_ERR_INVALID;

  if (req_envl->type != FAITH_ENVELOPE_MSG_REQUEST)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_OPEN)
    return FAITH_ERR_BAD_ENVELOPE;

  if (!cl->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  faith_envl_cts_msg_request_t req = {0};
  _FH_CHECK_RETURN(
      faith_decode_msg_request_body(req_envl->body, req_envl->body_size, &req));

  if (!routing_auth_id_registered(&s->rt, &req.auth_id_recv)) {

    faith_envl_stc_msg_request_failed_t fail = {
        .cl_req_id = req.cl_req_id,
        .reason = FAITH_MSG_REQUEST_FAIL_USER_NOT_FOUND};

    faith_body_size_t body_size = 0;
    uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_FAILED_BODY_SIZE];
    _FH_CHECK_RETURN(faith_encode_msg_request_failed_body(body, &body_size,
                                                          sizeof(body), &fail));

    faith_envelope_t fail_envl = {0};
    fail_envl.type = FAITH_ENVELOPE_MSG_REQUEST_FAILED;
    fail_envl.recipient_id = cl->auth_id;

    fail_envl.body = body;
    fail_envl.body_size = body_size;

    _FH_CHECK_RETURN(server_send_envelope_or_close(s, cl, &fail_envl));

    return FAITH_OK;
  }

  faith_envl_stc_msg_request_ack_t ack = {0};
  ack.cl_req_id = req.cl_req_id;
  _FH_CHECK_RETURN(
      faith_random_bytes(ack.srv_req_id.bytes, sizeof(ack.srv_req_id.bytes)));

  struct server_msg_request_t server_req = {
      .auth_id_receiver = req.auth_id_recv,
      .auth_id_sender = cl->auth_id,
      .created_at_ms = faith_now_ms(),
      .expires_at_ms = faith_now_ms() + FAITH_MSG_REQUEST_EXPIRATION_TIME_MS,
  };

  _FH_CHECK_RETURN(
      storage_store_msg_request(&s->storage, &ack.srv_req_id, &server_req));

  faith_status_code_t _fh_result = FAITH_OK;

  /* send MSG_REQUEST_RECEIVED to all devices of the recipient account */
  {
    faith_envl_stc_msg_request_received_t received = {0};
    received.srv_req_id = ack.srv_req_id;
    received.auth_id_sender = cl->auth_id;

    faith_body_size_t body_size = 0;
    uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RECEIVED_BODY_SIZE];

    _FH_CHECK_DEFER(faith_encode_msg_request_received_body(
        body, &body_size, sizeof(body), &received));

    faith_envelope_t received_envl = {
        .type = FAITH_ENVELOPE_MSG_REQUEST_RECEIVED,
        .recipient_id = req.auth_id_recv,
        .body = body,
        .body_size = body_size,
    };

    _FH_CHECK_DEFER(
        server_route_envelope(s, cl, &req.auth_id_recv, &received_envl));
  }

  faith_body_size_t body_size = 0;
  uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_ACK_BODY_SIZE];

  _FH_CHECK_RETURN(
      faith_encode_msg_request_ack_body(body, &body_size, sizeof(body), &ack));

  faith_envelope_t ack_envl = {
      .type = FAITH_ENVELOPE_MSG_REQUEST_ACK,
      .recipient_id = cl->auth_id,
      .body = body,
      .body_size = body_size,
  };

  _FH_CHECK(server_send_envelope_or_close(s, cl, &ack_envl));

  return FAITH_OK;

defer: { _FH_CHECK(storage_remove_msg_request(&s->storage, &ack.srv_req_id)); }
  return _fh_result;
}

static faith_status_code_t server_send_msg_request_response_failed(
    struct server_state_t *s, struct client_conn_t *cl,
    const faith_envl_cts_msg_request_response_t *response,
    faith_msg_request_response_fail_reason_t     reason) {
  if (!s || !cl || cl->closing || !response)
    return FAITH_ERR_INVALID;

  faith_envl_stc_msg_request_response_failed_t failed = {
      .reason = reason,
      .cl_req_id = response->cl_req_id,
      .srv_req_id = response->srv_req_id,
  };

  faith_body_size_t body_size = 0;
  uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_FAILED_BODY_SIZE];

  _FH_CHECK_RETURN(faith_encode_msg_request_response_failed_body(
      body, &body_size, sizeof(body), &failed));

  faith_envelope_t failed_envl = {
      .type = FAITH_ENVELOPE_MSG_REQUEST_RESPONSE_FAILED,
      .recipient_id = cl->auth_id,
      .body = body,
      .body_size = body_size,
  };

  _FH_CHECK_RETURN(server_send_envelope_or_close(s, cl, &failed_envl));

  return FAITH_OK;
}

static faith_status_code_t server_send_msg_request_response_ack(
    struct server_state_t *s, struct client_conn_t *cl,
    const faith_envl_cts_msg_request_response_t *response) {
  if (!s || !cl || cl->closing || !response)
    return FAITH_ERR_INVALID;

  faith_envl_stc_msg_request_response_ack_t ack = {
      .cl_req_id = response->cl_req_id,
      .srv_req_id = response->srv_req_id,
  };

  faith_body_size_t body_size = 0;
  uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_ACK_BODY_SIZE];

  _FH_CHECK_RETURN(faith_encode_msg_request_response_ack_body(
      body, &body_size, sizeof(body), &ack));

  faith_envelope_t ack_envl = {
      .type = FAITH_ENVELOPE_MSG_REQUEST_RESPONSE_ACK,
      .recipient_id = cl->auth_id,
      .body = body,
      .body_size = body_size,
  };

  _FH_CHECK_RETURN(server_send_envelope_or_close(s, cl, &ack_envl));

  return FAITH_OK;
}

static faith_status_code_t
server_handle_msg_request_response(struct server_state_t *s, struct client_conn_t *cl,
                          const faith_envelope_t *response_envl) {
  if (!s || !cl || cl->closing || !response_envl)
    return FAITH_ERR_INVALID;

  if (response_envl->type != FAITH_ENVELOPE_MSG_REQUEST_RESPONSE)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_OPEN)
    return FAITH_ERR_BAD_ENVELOPE;

  if (!cl->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  /* Get session of the responding client connection */
  struct client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(
      routing_get_session(&s->rt, &cl->auth_id, &cl->device_id, &sess));

  if (!sess)
    return FAITH_ERR_INVALID;

  /* Decode MSG_REQUEST_RESPONSE body */
  faith_envl_cts_msg_request_response_t response = {0};
  _FH_CHECK_RETURN(faith_decode_msg_request_response_body(
      response_envl->body, response_envl->body_size, &response));

  /* Retrieve the message request that the client wants to respond to */
  struct server_msg_request_t *stored_req = NULL;
  faith_status_code_t          storage_rc =
      storage_get_msg_request(&s->storage, &response.srv_req_id, &stored_req);

  if(storage_rc == FAITH_ERR_NOT_FOUND) {
    _FH_CHECK(server_send_msg_request_response_failed(
        s, cl, &response, FAITH_MSG_REQUEST_RESPONSE_FAIL_REQUEST_NOT_FOUND));
    return _fh_rc;
  }

  if (storage_rc != FAITH_OK) {
    _FH_CHECK(server_send_msg_request_response_failed(
        s, cl, &response, FAITH_MSG_REQUEST_RESPONSE_FAIL_INTERNAL_ERROR));
    return storage_rc;
  }

  /* <auth_id_receiver> here refers to the original receiver of the message
   * request, not the receiver of this MSG_REQUEST_RESPONSE. */
  if (!faith_client_id_equal(stored_req->auth_id_receiver, cl->auth_id)) {
    _FH_CHECK_RETURN(server_send_msg_request_response_failed(
        s, cl, &response, FAITH_MSG_REQUEST_RESPONSE_FAIL_NOT_RECIPIENT));
    return FAITH_OK;
  }

  const uint64_t now_ms = faith_now_ms();

  if (stored_req->expires_at_ms <= now_ms) {
    _FH_CHECK(storage_remove_msg_request(&s->storage, &response.srv_req_id));

    if (_fh_rc != FAITH_OK && _fh_rc != FAITH_ERR_NOT_FOUND)
      return _fh_rc;

    _FH_CHECK_RETURN(server_send_msg_request_response_failed(
        s, cl, &response, FAITH_MSG_REQUEST_RESPONSE_FAIL_EXPIRED));

    return FAITH_OK;
  }

  faith_signature_msg_request_response_t sign_msg = {0};
  sign_msg.type = response.type;
  sign_msg.srv_req_id = response.srv_req_id;
  sign_msg.auth_id_recv = cl->auth_id;
  sign_msg.device_id_recv = cl->device_id;
  sign_msg.auth_id_req = stored_req->auth_id_sender;

  size_t msg_size = 0;
  uint8_t msg_buf[FAITH_SIGNATURE_MSG_REQUEST_RESPONSE_SIZE];
  {
    _FH_CHECK(faith_gen_sign_buf_msg_request_response(msg_buf, &msg_size, sizeof(msg_buf),
                                                 &sign_msg));
    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%i] Failed to generate signing message buffer",
              cl->conn_id, cl->fd);

      return _fh_rc;
    }
  }

  /* Verify the signature */
  {
    _FH_CHECK(faith_verify_signature_raw_pubkey(
        sess->ident.public_key, msg_buf, sizeof(msg_buf),
        response.signature_response, sizeof(response.signature_response)));

    if (_fh_rc == FAITH_ERR_NOT_EQUAL) {
      return FAITH_ERR_UNAUTHORIZED;
    }

    if (_fh_rc != FAITH_OK) {
      return _fh_rc;
    }
  }

  const faith_client_id_t auth_id_sender = stored_req->auth_id_sender;
  const faith_client_id_t auth_id_receiver = stored_req->auth_id_receiver;

  // TODO: Avoid race condition of multiple devices responding to a message
  // request and removing the request.
  faith_status_code_t remove_rc =
      storage_remove_msg_request(&s->storage, &response.srv_req_id);

  if (remove_rc == FAITH_ERR_NOT_FOUND) {
    return server_send_msg_request_response_failed(
        s, cl, &response, FAITH_MSG_REQUEST_RESPONSE_FAIL_ALREADY_RESPONDED);
  }

  if (remove_rc != FAITH_OK) {
    _FH_CHECK(server_send_msg_request_response_failed(
        s, cl, &response, FAITH_MSG_REQUEST_RESPONSE_FAIL_INTERNAL_ERROR));

    return remove_rc;
  }

  /* route MSG_REQUEST_RESPONDED to client that sent the request originally */
  {
    faith_envl_stc_msg_request_responded_t responded = {
        .srv_req_id = response.srv_req_id,
        .auth_id_responder = auth_id_receiver,
        .type = response.type,
    };

    faith_body_size_t body_size = 0;
    uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RESPONDED_BODY_SIZE];

    _FH_CHECK_RETURN(faith_encode_msg_request_responded_body(
        body, &body_size, sizeof(body), &responded));

    faith_envelope_t responded_envl = {
        .type = FAITH_ENVELOPE_MSG_REQUEST_RESPONDED,
        .recipient_id = auth_id_sender,
        .body = body,
        .body_size = body_size,
    };

    _FH_CHECK_RETURN(server_route_envelope(s, cl, &auth_id_sender, &responded_envl));
  }

  /* send MSG_REQUEST_RESPONSE_ACK back to client */
  _FH_CHECK_RETURN(server_send_msg_request_response_ack(s, cl, &response));

  return FAITH_OK;
}

static faith_status_code_t server_route_msg_envl(struct server_state_t *s,
                                                 struct client_conn_t  *cl,
                                                 faith_envelope_t      *envl) {
  if (!s || !cl || !envl)
    return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_MSG_SEND)
    return FAITH_ERR_INVALID;

  /* MSG_SEND specifically requires a nonempty body. */
  if (envl->body_size == 0 || !envl->body)
    return FAITH_ERR_INVALID;

  _FH_CHECK_RETURN(server_route_envelope(s, cl, &envl->recipient_id, envl));

  return FAITH_OK;
}

static faith_status_code_t server_handle_envelope(struct server_state_t *s,
                                                  struct client_conn_t  *cl,
                                                  faith_frame_t *frame) {

  if (!s || !frame || !cl)
    return FAITH_ERR_INVALID;

  faith_envelope_t envl = {0};

  faith_status_code_t _fh_result = FAITH_OK;

  _FH_CHECK_DEFER(
      faith_decode_envelope(frame->payload, frame->payload_size, &envl));

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server is handling envelope: type=%s body_size=%u",
          cl->conn_id, cl->fd, faith_envelope_name(envl.type), envl.body_size);

  switch (envl.type) {
  case FAITH_ENVELOPE_HELLO:
    _FH_CHECK_DEFER(server_handle_hello(s, cl, &envl));
    break;

  case FAITH_ENVELOPE_CHALLENGE_RESPONSE:
    _FH_CHECK_DEFER(server_handle_challenge_response(s, cl, &envl));
    break;

  case FAITH_ENVELOPE_MSG_SEND:
    _FH_CHECK_DEFER(server_route_msg_envl(s, cl, &envl));
    break;

  case FAITH_ENVELOPE_DEVICE_AUTH_APPROVE:
  case FAITH_ENVELOPE_DEVICE_AUTH_DENY:
    _FH_CHECK_DEFER(server_handle_device_link_response(s, cl, &envl));
    break;
  case FAITH_ENVELOPE_MSG_REQUEST: 
    _FH_CHECK_DEFER(server_handle_msg_request(s, cl, &envl));
    break;
  case FAITH_ENVELOPE_MSG_REQUEST_RESPONSE:  
    _FH_CHECK_DEFER(server_handle_msg_request_response(s, cl, &envl));
    break;
  default:
    _FH_RETURN_DEFER(FAITH_ERR_BAD_ENVELOPE);
  }

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server successfully handled envelope: type=%s body_size=%u",
          cl->conn_id, cl->fd, faith_envelope_name(envl.type), envl.body_size);

defer:
  if (envl.body != NULL)
    free(envl.body);

  return _fh_result;
}

static faith_status_code_t handle_frame(struct server_state_t *s,
                                        struct client_conn_t  *cl,
                                        faith_frame_t         *frame) {
  if (!s || !cl || !frame)
    return FAITH_ERR_INVALID;

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server got frame: msg_type=%s payload_size=%zu",
          cl->conn_id, cl->fd, faith_frame_msg_name(frame->msg_type),
          frame->payload_size);

  switch (frame->msg_type) {
  case FAITH_MSG_PING:
    _FH_CHECK_RETURN(server_send_pong(s, cl, frame));
    break;
  case FAITH_MSG_ENVL:
    _FH_CHECK_RETURN(server_handle_envelope(s, cl, frame));
    break;
  default:
    return FAITH_ERR_BAD_FRAME;
  }

  return FAITH_OK;
}

static enum read_frame_result_t
server_read_more_ssl_bytes(struct client_conn_t      *cl,
                           const struct server_cfg_t *cfg) {
  if (!cl || !cfg)
    return READ_FRAME_ERROR;

  uint8_t tmp[4096];

  int nread = SSL_read(cl->ssl, tmp, sizeof(tmp));

  if (nread > 0) {
    faith_status_code_t rc =
        server_enqueue_input_bytes(cl, tmp, (size_t)nread, cfg);

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

static faith_status_code_t try_parse_frame_from_buffer(const uint8_t *payload,
                                                       size_t payload_size,
                                                       faith_frame_t *out,
                                                       size_t *consumed_out) {
  if (!consumed_out || !out)
    return FAITH_ERR_INVALID;

  *consumed_out = 0;

  /* accepts and handles NULL <payload> and invalid/incomplete payload_size
   * writes <out> if successfully decoded a full frame */
  faith_status_code_t rc = faith_decode_frame(payload, payload_size, out);

  if (rc != FAITH_OK) {
    return rc;
  }

  *consumed_out = out->frame_size + FAITH_FRAME_LENGTH_SIZE;;

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
      enum read_frame_result_t rr = server_read_more_ssl_bytes(cl, cfg);

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
              " fd=%i] Error while reading incomplete frame. Error=%i",
              cl->conn_id, cl->fd, rr);

      return rr;
    }

    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Failed to read frame",
            cl->conn_id, cl->fd);

    return READ_FRAME_ERROR;
  }
}

static int drive_client_read(struct server_state_t *s, struct client_conn_t *cl,
                             const struct server_cfg_t *cfg) {
  if (!s || !cl || cl->closing || !cfg)
    return -1;
  faith_frame_t frame;

  enum read_frame_result_t rr = try_read_one_frame(cl, &frame, cfg);

  switch (rr) {
  case READ_FRAME_OK: {
    _FH_CHECK(handle_frame(s, cl, &frame));
    faith_frame_free(&frame);
    if (_fh_rc != FAITH_OK) {
      server_disconnect_client(
          s, cl, FAITH_DISCONNECT_BAD_PROTOCOL, FAITH_CLIENT_RECONNECT_ALLOWED,
          0, 0, "Server failed to properly handle client frame.");
      return -1;
    }

    if (cl->closing)
      return -1;

    if (cfg->verbose_logging) {
      nob_log(INFO, "[client=%" PRIu64 " fd=%i] Success handling client frame.",
              cl->conn_id, cl->fd);
    }

    uint32_t mask = cl->ev_mask | EPOLLIN;

    if (cl->out_buf && cl->out_off < cl->out_size) {
      mask |= EPOLLOUT;
    }

    if (modify_client_ev_mask(s->epoll_fd, cl, mask) < 0) {
      goto fail_ev_mask;
    }

    return 0;
  }
  case READ_FRAME_WANT_READ: {
    uint32_t mask = cl->ev_mask | EPOLLIN;

    if (cl->out_buf && cl->out_off < cl->out_size) {
      mask |= EPOLLOUT;
    }

    if (modify_client_ev_mask(s->epoll_fd, cl, mask) < 0) {
      goto fail_ev_mask;
    }
    return 0;
  }

  case READ_FRAME_CLOSED:
    server_disconnect_client(
        s, cl, FAITH_DISCONNECT_TEMPORARY_FAILURE,
        FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
        "Connection closed while reading incomplete frame.");
    return -1;
  default:
    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Failed to read client frame.",
            cl->conn_id, cl->fd);

    server_disconnect_client(s, cl, FAITH_DISCONNECT_BAD_PROTOCOL,
                             FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                             "Server failed to read client frame.");
    return -1;
  }

fail_ev_mask:
  nob_log(ERROR, "[client=%" PRIu64 " fd=%d] modify_client_ev_mask failed: %s",
          cl->conn_id, cl->fd, strerror(errno));

  server_disconnect_client(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                           FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                           "Failed to modify client event mask.");

  return -1;
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

    // create SSL object for client
    cl->ssl = SSL_new(s->ssl_ctx);
    if (!cl->ssl) {
      nob_log(ERROR, "SSL_new() failed for client connection (FD: %i)",
              client_fd);
      _FH_CHECK(close_client(s, &cl));
      continue;
    }

    // set file descriptor of ssl object
    SSL_set_fd(cl->ssl, cl->fd);
    SSL_set_accept_state(cl->ssl);

    struct epoll_event ev;
    memset(&ev, 0, sizeof(ev));

    ev.events = EPOLLIN | EPOLLRDHUP;
    ev.data.ptr = cl;
    cl->ev_mask = ev.events;

    if (epoll_ctl(s->epoll_fd, EPOLL_CTL_ADD, client_fd, &ev) < 0) {
      nob_log(ERROR, "epoll_ctl() failed for client (FD: %i): %s", client_fd,
              strerror(errno));
      _FH_CHECK(close_client(s, &cl));
      continue;
    }
  }
}

static faith_status_code_t drive_tls_handshake(struct server_state_t *s,
                                               struct client_conn_t  *cl) {
  int rc = SSL_accept(cl->ssl);

  if (rc == 1) {
    set_client_state(s, cl, CLIENT_WAIT_FOR_HELLO);

    if (modify_client_ev_mask(s->epoll_fd, cl, cl->ev_mask | EPOLLIN) < 0) {
      nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
      return FAITH_ERR_IO;
    }

    return FAITH_OK;
  }

  int err = SSL_get_error(cl->ssl, rc);

  if (err == SSL_ERROR_WANT_READ) {
    if (modify_client_ev_mask(s->epoll_fd, cl, cl->ev_mask | EPOLLIN) < 0) {
      nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
      return FAITH_ERR_IO;
    }

    return FAITH_OK;
  }

  if (err == SSL_ERROR_WANT_WRITE) {
    if (modify_client_ev_mask(s->epoll_fd, cl, cl->ev_mask | EPOLLOUT) < 0) {
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
    nob_log(INFO, "[client=%" PRIu64 " fd=%i] wrote %i bytes over the wire.",
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
  cl->out_cap = 0;
  cl->out_off = 0;

  if (modify_client_ev_mask(epoll_fd, cl,
                            EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP) < 0) {
    nob_log(ERROR, "modify_client_ev_mask failed: %s", strerror(errno));
    return FAITH_ERR_IO;
  }

  return FAITH_OK;
}

static int client_output_empty(const struct client_conn_t *cl) {
  if (!cl || !cl->ssl)
    return 0;

  return cl->out_size == 0 && BIO_ctrl_pending(SSL_get_wbio(cl->ssl)) == 0;
}

static void server_begin_shutdown(struct server_state_t *s) {
  for (struct client_conn_t *cl = s->clients; cl != NULL;) {
    struct client_conn_t *next = cl->next;

    faith_status_code_t rc = server_disconnect_client(
        s, cl, FAITH_DISCONNECT_SERVER_SHUTDOWN,
        FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0, "The server has shut down.");

    if (rc != FAITH_OK) {
      nob_log(ERROR, "Failed to send shutdown disconnect to client: %s",
              faith_status_code_name(rc));

      struct client_conn_t *to_close = cl;
      _FH_CHECK(close_client(s, &to_close));
    }

    cl = next;
  }
}

static bool server_has_clients(const struct server_state_t *s) {
  return s->clients != NULL;
}

int loop(struct server_state_t *s) {
  struct epoll_event events[MAX_EVENTS];

  bool     shutting_down = false;
  uint64_t shutdown_deadline_ms = 0;

  for (;;) {
    if (shutdown_requested && !shutting_down) {
      shutting_down = true;

      if (server_has_clients(s))
        nob_log(INFO, "Server shutdown requested; notifying clients.");

      if (s->listenfd >= 0) {
        epoll_ctl(s->epoll_fd, EPOLL_CTL_DEL, s->listenfd, NULL);
        close(s->listenfd);
        s->listenfd = -1;
      }

      server_begin_shutdown(s);

      shutdown_deadline_ms = faith_now_ms() + 3000;
    }

    if (shutting_down) {
      if (!server_has_clients(s))
        break;

      if (faith_now_ms() >= shutdown_deadline_ms) {
        nob_log(
            WARNING,
            "Shutdown flush deadline reached; forcing all clients to close.");

        while (s->clients != NULL) {
          struct client_conn_t *cl = s->clients;
          _FH_CHECK(close_client(s, &cl));
        }

        break;
      }
    }

    int timeout_ms = shutting_down ? 100 : -1;
    int n = epoll_wait(s->epoll_fd, events, MAX_EVENTS, timeout_ms);

    if (n < 0) {
      if (errno == EINTR)
        continue;

      nob_log(ERROR, "epoll_wait() failed: %s", strerror(errno));
      return 1;
    }

    for (int i = 0; i < n; ++i) {
      uint32_t revents = events[i].events;

      if (!shutting_down && events[i].data.fd == s->listenfd) {
        accept_clients(s);
        continue;
      }

      struct client_conn_t *cl = events[i].data.ptr;
      if (!cl)
        continue;

      if (revents & (EPOLLERR | EPOLLHUP | EPOLLRDHUP)) {
        _FH_CHECK(close_client(s, &cl));
        continue;
      }

      int dead = 0;

      if (cl->closing) {
        _FH_CHECK(close_client(s, &cl));
        continue;
      }

      if (shutting_down || cl->close_after_flush) {
        if ((revents & EPOLLOUT) &&
            flush_client_output(s->epoll_fd, cl) != FAITH_OK) {
          dead = 1;
        }
      } else if (cl->state == CLIENT_HANDSHAKE) {
        if (drive_tls_handshake(s, cl) < 0)
          dead = 1;
      } else if (cl->state == CLIENT_OPEN ||
                 cl->state == CLIENT_WAIT_FOR_HELLO ||
                 cl->state == CLIENT_WAIT_FOR_CHALLENGE_RESPONSE ||
                 cl->state == CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE) {
        if ((revents & EPOLLIN) && drive_client_read(s, cl, &s->cfg) < 0) {
          dead = 1;
        }
      }

      if (!dead && !cl->closing && (revents & EPOLLOUT) &&
          flush_client_output(s->epoll_fd, cl) != FAITH_OK) {
        dead = 1;
      }

      if (dead || cl->closing) {
        _FH_CHECK(close_client(s, &cl));
        continue;
      }

      if (cl->close_after_flush && client_output_empty(cl)) {
        _FH_CHECK(close_client(s, &cl));
        continue;
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

static void ignore_sigpipe(void) {
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));
  sa.sa_handler = SIG_IGN;

  sigemptyset(&sa.sa_mask);
  sa.sa_flags = 0;

  sigaction(SIGPIPE, &sa, NULL);
}

int main(int argc, char **argv) {
  ignore_sigpipe();

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

  /* User only wanted to print help message */
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
