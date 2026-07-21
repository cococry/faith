#define _GNU_SOURCE

#include "transport/tls.h"
#include "transport/conn.h"
#include "transport/frame.h"
#include "reactor/reactor.h"
#include "server/server.h"
#include "server/sess_registry.h"
#include "logging/logging.h"

#include <openssl/crypto.h>
#include <openssl/err.h>
#include <openssl/rand.h>
#include <openssl/ssl.h>

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
#include "../third_party/nob.h"

#include "protocol.h"

#include "../third_party/stb_ds.h"

#define PORT       4433
#define MAX_EVENTS 1024

#define _FH_FOR_EACH_AUTH_DEVICE(SERVER, AUTH_ID, RECIPIENT, STATUS_OUT, BODY) \
  do {                                                                         \
    struct server_state_t        *_fh_iter_server = (SERVER);                  \
    const faith_client_id_t      *_fh_iter_auth_id = (AUTH_ID);                \
    client_session_device_t *_fh_iter_devices = NULL;                     \
                                                                               \
    (STATUS_OUT) = sess_registry_get_devices(&_fh_iter_server->rt, _fh_iter_auth_id, \
                                       &_fh_iter_devices);                     \
                                                                               \
    if ((STATUS_OUT) == FAITH_ERR_NOT_FOUND) {                                 \
      (STATUS_OUT) = FAITH_OK;                                                 \
    } else if ((STATUS_OUT) == FAITH_OK) {                                     \
      ptrdiff_t _fh_iter_count = hmlen(_fh_iter_devices);                      \
                                                                               \
      for (ptrdiff_t _fh_iter_i = 0; _fh_iter_i < _fh_iter_count;              \
           ++_fh_iter_i) {                                                     \
        if (!_fh_iter_devices[_fh_iter_i].value ||                             \
            !_fh_iter_devices[_fh_iter_i].value->conn)                         \
          continue;                                                            \
                                                                               \
        struct client_conn_t *(RECIPIENT) =                                    \
            _fh_iter_devices[_fh_iter_i].value->conn;                          \
                                                                               \
        if ((RECIPIENT)->closing || !(RECIPIENT)->authorized ||                \
            (RECIPIENT)->state != CLIENT_OPEN)                                 \
          continue;                                                            \
                                                                               \
        BODY                                                                   \
      }                                                                        \
    }                                                                          \
  } while (0)

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



struct storage_state_t {
  struct stored_msg_request_t* msg_requests;
};

struct server_state_t {
  reactor_source_t  listen_source;
  reactor_context_t reactor;

  tls_context_t tls;

  atomic_uint_fast64_t  next_client_id;
  struct client_conn_t *clients;

  sess_registry_state_t rt;
  struct storage_state_t storage;
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

  if (g_verbose_logging) {
    nob_log(INFO, "[client=%" PRIu64 " fd=%i] Client changed state to %s",
            cl->conn.id, cl->conn.fd, client_state_name(state));
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
static faith_status_code_t storage_store_msg_request(
    struct storage_state_t            *st,
    const faith_request_id_t           *request_id,
    const struct server_msg_request_t  *request) {
  if (!st || !request_id || !request)
    return FAITH_ERR_INVALID;

  /* This also handles the extremely unlikely case of a randomly generated
   * server request ID colliding. */

  // TODO: handle colliding in caller
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

static faith_status_code_t
server_cancel_pending_device_link_request(struct server_state_t *s,
                                          struct client_conn_t  *requesting_cl);

void
server_remove_pending_device_link_request(struct client_conn_t *cl);

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

  faith_status_code_t result = FAITH_OK;
  if (cl->state == CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE && !shutdown_requested) {
    faith_status_code_t rc = server_cancel_pending_device_link_request(s, cl);

    if (rc != FAITH_OK) {
      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%d] Failed to cancel pending device-link request: %s",
              cl->conn.id, cl->conn.fd, faith_status_code_name(rc));

      if (result == FAITH_OK)
        result = rc;
    }
  }

  *cl_ptr = NULL;

  server_remove_client(s, cl);

  int device_link_req_pending =
      cl->authorized && (cl->pending_device_link_conn != NULL &&
                         cl->pending_device_link_req != NULL);

  if (device_link_req_pending) {

    client_session_device_t *devices = NULL;
    faith_status_code_t      rc =
        sess_registry_get_devices(&s->rt, &cl->auth_id, &devices);
    if (rc != FAITH_OK || devices == NULL) {
      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%d] Failed to enumerate account devices: %s",
              cl->conn.id, cl->conn.fd, faith_status_code_name(rc));
      if (result == FAITH_OK)
        result = rc;
    }
    /* avoid client-to-client communication on server shutdown */
    else if (hmlen(devices) - 1 == 0 && !shutdown_requested) {
      rc = server_disconnect_client(s, cl->pending_device_link_conn,
                                    FAITH_DISCONNECT_TEMPORARY_FAILURE,
                                    FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                                    "All authorized devices of the account you "
                                    "are trying to link to have disconnected.");
      if (result == FAITH_OK)
        result = rc;
    }

    server_remove_pending_device_link_request(cl);
  }

  if (cl->authorized) {
    // TODO: persistent sessions
    faith_status_code_t rc =
        sess_registry_unregister_session(&s->rt, &cl->auth_id, &cl->device_id);

    if (rc != FAITH_OK) {
      nob_log(ERROR, "[client=%" PRIu64 "] routing unregister failed: %s (%d)",
              cl->conn.id, faith_status_code_name(rc), (int)rc);

      result = rc;
    }
  }

  {
    _FH_CHECK(reactor_remove(&s->reactor, &cl->reactor_source));
    if (_fh_rc != FAITH_OK) {
      result = _fh_rc;
    }
  }

  {
    faith_status_code_t rc = tls_shutdown(&cl->conn.tls);
    if (rc != FAITH_OK) {
      nob_log(ERROR, "[client=%" PRIu64 "] TLS shutdown failed: %s (%d)",
              cl->conn.id, faith_status_code_name(rc), (int)rc);
      result = rc;
    }
  }

  const int      log_fd = cl->conn.fd;
  const uint64_t log_conn_id = cl->conn.id;

  if (cl->conn.fd >= 0) {
    if (close(cl->conn.fd) < 0) {
      nob_log(ERROR, "[client=%" PRIu64 " fd=%d] close failed: %s", cl->conn.id,
              cl->conn.fd, strerror(errno));

      if (result == FAITH_OK)
        result = FAITH_ERR_IO;
    }

    cl->conn.fd = -1;
  }

  {
    _FH_CHECK(conn_queue_free(&cl->conn.out));
  }
  {
    _FH_CHECK(conn_queue_free(&cl->conn.in));
  }

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

  uint8_t           body[FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_MAX] = {0};
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

void server_destroy(struct server_state_t *s) {
  if (!s)
    return;

  nob_log(INFO, "Destroying server context...");

  while (s->clients) {
    struct client_conn_t *cl = s->clients;
    close_client(s, &cl);
  }

  _FH_CHECK_SCOPED(sess_registry_destroy(&s->rt));

  _FH_CHECK_SCOPED(reactor_destroy(&s->reactor));

  _FH_CHECK_SCOPED(tls_destroy(&s->tls));

  nob_log(INFO, "Destroyed server context.");
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
    _FH_CHECK(conn_queue_enqueue_bytes(&cl->conn.out, wire_data, wire_size));

    if (_fh_rc != FAITH_OK) {
      free(wire_data);
      return _fh_rc;
    }
  }

  free(wire_data);
  wire_data = NULL;

  if (!(cl->reactor_source.interests & REACTOR_WRITABLE)) {
    _FH_CHECK(reactor_modify_interests(&s->reactor, &cl->reactor_source,
                                       cl->reactor_source.interests |
                                           REACTOR_WRITABLE));
    if (_fh_rc != FAITH_OK) {
      server_disconnect_client(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                               FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                               "Failed to enable REACTOR_WRITABLE for the client.");
      return _fh_rc;
    }
  }

  nob_log(INFO, "[client=%" PRIu64 " fd=%d] queued frame: %s (%zu bytes)",
          cl->conn.id, cl->conn.fd, faith_frame_msg_name(msg_type), wire_size);

  return FAITH_OK;
}

static faith_status_code_t server_send_pong(struct server_state_t *s,
                                            struct client_conn_t  *cl,
                                            faith_frame_t         *frame) {
  if (!s || !cl || cl->closing || !frame)
    return FAITH_ERR_INVALID;
  
  faith_msg_ping_t ping = {0};

  /* 1. decode PING */
  _FH_CHECK_RETURN(faith_decode_ping(frame->payload, frame->payload_size,
                                      &ping));

  nob_log(INFO,
          "[client=%" PRIu64 " fd=%i] server got PING: nonce=%" PRIu64
          ", client_sent_at_ms=%lu",
          cl->conn.id, cl->conn.fd, ping.nonce, ping.client_sent_at_ms);

  /* 2. Send PONG over wire protocol */
  uint64_t server_sent_at_ms = faith_now_ms();

  faith_body_size_t payload_size = 0;
  uint8_t payload[FAITH_MSG_PONG_PAYLOAD_SIZE] = {0};

  faith_msg_pong_t pong = {
    .server_sent_at_ms = server_sent_at_ms,
    .nonce = ping.nonce
  };

  _FH_CHECK_RETURN(
      faith_encode_pong(payload, &payload_size, sizeof(payload), &pong));

  _FH_CHECK_RETURN(
      server_send_over_wire(s, cl, payload, sizeof(payload), FAITH_MSG_PONG));

  nob_log(
      INFO,
      "[client=%" PRIu64
      " fd=%i] Server sent PONG to client. nonce=%lu, server_sent_at_ms=%lu",
      cl->conn.id, cl->conn.fd, ping.nonce, server_sent_at_ms);

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
          cl->conn.id, cl->conn.fd, faith_envelope_name(envl->type),
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
            cl->conn.id, cl->conn.fd, faith_envelope_name(envl->type),
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

  client_session_user_t *recipient_user = NULL;
  _FH_CHECK_RETURN(sess_registry_get_user_from_auth_id(
      &s->rt, recipient_auth_id, &recipient_user));
  if (!recipient_user) {

    char recipient_auth_id_hex[33];

    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_auth_id->bytes, recipient_auth_id_hex));

    nob_log(INFO,
            "[client=%" PRIu64 " fd=%i] Envelope %s: Recipient "
            "(auth_id: %s) is offline; Not routing envelope.",
            cl_sender->conn.id, cl_sender->conn.fd, faith_envelope_name(envl->type),
            recipient_auth_id_hex);

    return FAITH_OK;
  }

  char recipient_auth_id_hex[33];

  _FH_CHECK_RETURN(
      faith_id128_to_hex(recipient_auth_id->bytes, recipient_auth_id_hex));

  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE(s, recipient_auth_id, recipient, device_loop_rc, {
    faith_envelope_t routing_envl = *envl;
    routing_envl.sender_id = cl_sender->auth_id;
    routing_envl.recipient_id = *recipient_auth_id;
    _FH_CHECK(server_send_envelope_or_close(s, recipient, &routing_envl));

    char recipient_device_id_hex[33];
    _FH_CHECK_RETURN(faith_id128_to_hex(recipient->device_id.bytes,
                                        recipient_device_id_hex));

    if (_fh_rc != FAITH_OK) {
      nob_log(
          ERROR,
          "[client=%" PRIu64 " fd=%i] Envelope %s: Failed to route envelope to "
          "recipient device (auth_id: %s, device_id: %s).",
          cl_sender->conn.id, cl_sender->conn.fd, faith_envelope_name(envl->type),
          recipient_auth_id_hex, recipient_device_id_hex);
      continue;
    }

    nob_log(INFO,
            "[client=%" PRIu64
            " fd=%i] Envelope %s: Routed envelope to recipient "
            "device (auth_id: %s, device_id: %s).",
            cl_sender->conn.id, cl_sender->conn.fd, faith_envelope_name(envl->type),
            recipient_auth_id_hex, recipient_device_id_hex);
  });

  return FAITH_OK;
}

void
server_remove_pending_device_link_request(struct client_conn_t *cl) {
  if (!cl || !cl->pending_device_link_req)
    return;

  free(cl->pending_device_link_req);
  cl->pending_device_link_req = NULL;
  cl->pending_device_link_conn = NULL;
}

static faith_status_code_t
server_cancel_pending_device_link_request(struct server_state_t *s,
                                  struct client_conn_t  *requesting_cl) {
  if (!s || !requesting_cl)
    return FAITH_ERR_INVALID;

  if (requesting_cl->state != CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE)
    return FAITH_OK;

  client_temporary_handshake_params_t *params =
      &requesting_cl->temp_handshake_params;

  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE(
      s, &params->sender_auth_id, authorized_cl, device_loop_rc, {
        if (authorized_cl->pending_device_link_conn != requesting_cl)
          continue;

        faith_envelope_t envl = {0};
        envl.type = FAITH_ENVELOPE_DEVICE_LINK_CANCELLED;
        envl.recipient_id = authorized_cl->auth_id;
        _FH_CHECK(server_send_envelope_or_close(s, authorized_cl, &envl));
        if (_fh_rc != FAITH_OK && device_loop_rc == FAITH_OK)
          device_loop_rc = _fh_rc;
        server_remove_pending_device_link_request(authorized_cl);
      });

  return device_loop_rc;
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
          cl->conn.id, cl->conn.fd, auth_id_hex, device_id_hex);

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
            cl->conn.id, cl->conn.fd);
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
  client_temporary_handshake_params_t *params = &cl->temp_handshake_params;

  params->sender_auth_id = hello_envl->sender_id;
  params->device_id = device_id;

  memcpy(params->public_key, public_key, sizeof(params->public_key));

  params->server_nonce = server_nonce;
  params->nonce = nonce;

  /* Send CHALLENGE envelope */

  faith_envelope_t challenge_envl;
  challenge_envl.type = FAITH_ENVELOPE_CHALLENGE;
  challenge_envl.recipient_id = hello_envl->sender_id;

  uint8_t body[FAITH_ENVL_HELLO_CHALLENGE_BODY_SIZE] = {0};
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
    return FAITH_ERR_BAD_ENVELOPE;
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

  uint8_t           body[FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE] = {0};
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
    const client_temporary_handshake_params_t *params) {
  if (!s || !cl || cl->closing || !params)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_WAIT_FOR_CHALLENGE_RESPONSE)
    return FAITH_ERR_BAD_ENVELOPE;

  if (g_verbose_logging) {
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
        cl->conn.id, cl->conn.fd, new_device_id_hex, cl_auth_id_hex);
  }

  if (!sess_registry_auth_id_registered(&s->rt, &params->sender_auth_id)) {
    server_disconnect_client(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                             FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
                             "The specified auth ID is in an invalid internal "
                             "state.");
    return FAITH_ERR_NOT_FOUND;
  }

  /* Send device authorization request to every already registered device
   for that auth ID */
  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE(
      s, &params->sender_auth_id, authorized_cl, device_loop_rc, {
        faith_envl_stc_device_link_req_t *req = NULL;
        _FH_CHECK(server_send_device_link_request(
            s, authorized_cl, cl, &params->sender_auth_id, params->public_key,
            &params->device_id, &req));

        if (_fh_rc != FAITH_OK || !req) {
          char sender_auth_id_hex[33];
          _FH_CHECK_RETURN(faith_id128_to_hex(params->sender_auth_id.bytes,
                                              sender_auth_id_hex));
          char device_id_hex[33];
          _FH_CHECK_RETURN(
              faith_id128_to_hex(params->device_id.bytes, device_id_hex));
          nob_log(ERROR,
                  "Failed to send device link request to authorized "
                  "device session with device_id: %s (auth_id: %s)",
                  device_id_hex, sender_auth_id_hex);

          return _fh_rc;
        }

        /* Set pending device link request of receiving client connection. We
         * also store the client connection that sent the device link request.
         */
        authorized_cl->pending_device_link_req = req;
        authorized_cl->pending_device_link_conn = cl;
      });

  /* Send DEVICE_AUTH_PENDING to the connection that requested the
   * new device */
  _FH_CHECK_RETURN(server_send_device_auth_pending(s, cl));
  /* temporarily assign <cl->auth_id> for disconnection purposes later. this
   * does not mean that the client is authorized. */
  cl->auth_id = params->sender_auth_id;
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
    _FH_CHECK_RETURN(sess_registry_register_session(&s->rt, &cl->auth_id,
                                              &cl->device_id, cl, public_key));
  }

  char cl_auth_id_hex[33];
  char cl_device_id_hex[33];
  _FH_CHECK_RETURN(faith_id128_to_hex(cl->auth_id.bytes, cl_auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(cl->device_id.bytes, cl_device_id_hex));

  nob_log(INFO,
          "[client=%" PRIu64 " fd=%i] Client passed authorization for "
          "requested routing session. (auth_id=%s, device_id=%s)",
          cl->conn.id, cl->conn.fd, cl_auth_id_hex, cl_device_id_hex);

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
            cl->conn.id, cl->conn.fd,
            faith_envelope_name(challenge_response_envl->type));
    return FAITH_ERR_INVALID;
  }

  if (cl->state != CLIENT_WAIT_FOR_CHALLENGE_RESPONSE) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got invalid CHALLENGE_RESPONSE from client.",
            cl->conn.id, cl->conn.fd);
    return FAITH_ERR_BAD_ENVELOPE;
  }

  client_temporary_handshake_params_t *params =
      &cl->temp_handshake_params;

  if (!faith_client_id_equal(challenge_response_envl->sender_id,
                             params->sender_auth_id)) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got CHALLENGE_RESPONSE from invalid client.",
            cl->conn.id, cl->conn.fd);
    return FAITH_ERR_INVALID;
  }

  if (!challenge_response_envl->body ||
      challenge_response_envl->body_size != FAITH_ED25519_SIGNATURE_SIZE) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got invalid CHALLENGE_RESPONSE envelope contents.",
            cl->conn.id, cl->conn.fd);
    return FAITH_ERR_INVALID;
  }

  uint8_t *verification_public_key = NULL;

  /* Looking for the mapped session of the auth ID, device ID pair */
  client_device_session_data_t *sess = NULL;
  faith_status_code_t                  sess_rc = sess_registry_get_session(
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
            cl->conn.id, cl->conn.fd, sender_auth_id_hex, device_id_hex);
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
                cl->conn.id, cl->conn.fd, sender_auth_id_hex, device_id_hex);
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
              cl->conn.id, cl->conn.fd);

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
          cl->conn.id, cl->conn.fd, sender_auth_id_hex, device_id_hex);
  return FAITH_ERR_UNAUTHORIZED;
}
}

static faith_status_code_t
server_send_device_auth_response_failed(struct server_state_t *s,
                                        struct client_conn_t  *cl) {
  if (!s || !cl || cl->closing)
    return FAITH_ERR_INVALID;

  faith_envelope_t failed_envl = {
      .type = FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_FAILED,
      .recipient_id = cl->auth_id,
      .body = NULL,
      .body_size = 0,
  };

  return server_send_envelope(s, cl, &failed_envl);
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
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));
    return FAITH_ERR_INVALID;
  }

  if (response_envl->type != FAITH_ENVELOPE_DEVICE_AUTH_APPROVE &&
      response_envl->type != FAITH_ENVELOPE_DEVICE_AUTH_DENY) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Invalid envelope type. Expected "
            "FAITH_ENVELOPE_DEVICE_AUTH_APPROVE or "
            "FAITH_ENVELOPE_DEVICE_AUTH_DENY, got %s",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    return FAITH_ERR_INVALID;
  }

  if (!response_envl->body ||
      response_envl->body_size !=
          FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got invalid %s envelope contents.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    _FH_CHECK_RETURN(server_send_device_auth_response_failed(s, cl));

    return FAITH_ERR_INVALID;
  }

  if (!cl->authorized) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(faith_id128_to_hex(cl->auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(faith_id128_to_hex(cl->device_id.bytes, device_id_hex));

    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got unauthorized %s"
            "from client. Client (auth_id=%s, device_id=%s) is not authorized.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type),
            auth_id_hex, device_id_hex);

    /* Return UNAUTHORIZED without rejecting/closing the client connection that
     * requested the device link. We are protecting the pending client
     * connection here. */
    return FAITH_ERR_UNAUTHORIZED;
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
            cl->conn.id, cl->conn.fd, req_auth_id_hex, req_device_id_hex);

    _FH_CHECK_RETURN(server_send_device_auth_response_failed(s, cl));

    return FAITH_OK;
  }

  faith_envl_cts_device_link_response_t response = {0};
  _FH_CHECK_RETURN(faith_decode_device_link_response_body(
      response_envl->body, response_envl->body_size, &response));

  if (!faith_device_id_equal(response.device_id_new, req->device_id_new)) {
    nob_log(
        ERROR,
        "[client=%" PRIu64 " fd=%i] Server got %s but the sent"
        "device_id does not match the device_id that requested the approval.",
        cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    _FH_CHECK_RETURN(server_send_device_auth_response_failed(s, cl));

    /* Return INVALID without rejecting/closing the client connection that
     * requested the device link. */
    return FAITH_ERR_INVALID;
  }

  if (faith_now_ms() > cl->pending_device_link_req->expires_at_ms) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got %s but the "
            "link request has already expired.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

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

  client_device_session_data_t *sess = NULL;
  {
    _FH_CHECK(sess_registry_get_session(&s->rt, &cl->auth_id, &cl->device_id, &sess));
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
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    _FH_CHECK_RETURN(server_send_device_auth_response_failed(s, cl));

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

  faith_envelope_t ack_envl = {0};
  ack_envl.type = FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_ACK;
  _FH_CHECK_RETURN(server_route_envelope(s, cl, &cl->auth_id, &ack_envl));

  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE(s, &cl->auth_id, recipient, device_loop_rc, {
    server_remove_pending_device_link_request(cl);
  });

  return device_loop_rc == FAITH_OK ? rc : device_loop_rc;

defer: {
  /* ================================================= */
  /* This path PUNISHES the client that requested the 
   * conversation. They will be executed. */
  /* ================================================= */

  /* Copies for logging */
  faith_client_id_t client_id = req->auth_id;
  faith_device_id_t device_id_new = req->device_id_new;

  /* Remove the pending device link request from <cl> */
  
  /* Don't propagate status code */
  {
    server_remove_pending_device_link_request(cl);
  }

  /* Close the connection that requested the device link: <req_cl> */
  char buf[FAITH_MAX_CLIENT_DISCONNECT_MSG];
  snprintf(buf, sizeof(buf),
           "Client failed device-link authorization. (Error: %s)",
           faith_status_code_name(_fh_result));

  /* Don't propagate status code */
  {
    _FH_CHECK(server_disconnect_client(s, req_cl, FAITH_DISCONNECT_AUTH_FAILED,
                                       FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
                                       buf));
  }

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
          req_cl->conn.id, req_cl->conn.fd, auth_id_hex, device_id_hex,
          faith_status_code_name(_fh_result), device_id_hex, auth_id_hex);

  /* Don't propagate status code */
  {
    _FH_CHECK(server_send_device_auth_response_failed(s, cl));
  }

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

  if (!sess_registry_auth_id_registered(&s->rt, &req.auth_id_recv)) {

    faith_envl_stc_msg_request_failed_t fail = {
        .cl_req_id = req.cl_req_id,
        .reason = FAITH_MSG_REQUEST_FAIL_USER_NOT_FOUND};

    faith_body_size_t body_size = 0;
    uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_FAILED_BODY_SIZE] = {0};
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
    uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RECEIVED_BODY_SIZE] = {0};

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
  uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_ACK_BODY_SIZE] = {0};

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
  uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_FAILED_BODY_SIZE] = {0};

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
  uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_ACK_BODY_SIZE] = {0};

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
  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(
      sess_registry_get_session(&s->rt, &cl->auth_id, &cl->device_id, &sess));

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
              cl->conn.id, cl->conn.fd);

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
    uint8_t           body[FAITH_ENVL_STC_MSG_REQUEST_RESPONDED_BODY_SIZE] = {0};

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
          cl->conn.id, cl->conn.fd, faith_envelope_name(envl.type), envl.body_size);

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
          cl->conn.id, cl->conn.fd, faith_envelope_name(envl.type), envl.body_size);

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
          cl->conn.id, cl->conn.fd, faith_frame_msg_name(frame->msg_type),
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

static void accept_clients(struct server_state_t *s) {
  while (1) {

    struct sockaddr_storage addr;
    socklen_t               addr_size = sizeof addr;

    int client_fd = accept4(s->listen_source.fd, (struct sockaddr *)&addr,
                            &addr_size, SOCK_NONBLOCK | SOCK_CLOEXEC);

    if (client_fd < 0) {
      // no clients left to accept
      if (errno == EAGAIN || errno == EWOULDBLOCK)
        return;

      if (errno == EINTR)
        continue;

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

    {
      _FH_CHECK(conn_init(&cl->conn));
      if (_fh_rc != FAITH_OK) {
        _FH_CHECK(close_client(s, &cl));
        continue;
      }
    }

    cl->conn.id = atomic_fetch_add(&s->next_client_id, 1);
    cl->conn.fd = client_fd;

    cl->reactor_source = (reactor_source_t){.fd = client_fd,
                                            .interests = REACTOR_READABLE,
                                            .user_data = cl,
                                            .type = REACTOR_SOURCE_CLIENT};
    {
      _FH_CHECK(tls_new_with_fd(&s->tls, cl->conn.fd, &cl->conn.tls));
      if (_fh_rc != FAITH_OK) {
        _FH_CHECK(close_client(s, &cl));
        continue;
      }
    }

    {
      _FH_CHECK(reactor_add(&s->reactor, &cl->reactor_source));
      if (_fh_rc != FAITH_OK) {
        _FH_CHECK(close_client(s, &cl));
        continue;
      }
    }

    /* Add client to linked list of clients */
    server_add_client(s, cl);
    set_client_state(s, cl, CLIENT_HANDSHAKE);

    nob_log(INFO, "accepted new client id=%" PRIu64 " fd=%i", cl->conn.id,
            client_fd);
  }
}

static faith_status_code_t drive_tls_handshake(struct server_state_t *s,
                                               struct client_conn_t  *cl) {
  if (!s || !cl || cl->closing)
    return FAITH_ERR_INVALID;

  int err = tls_accept(&cl->conn.tls);

  if (err == INT_MAX) {
    return FAITH_ERR_INVALID;
  }

  reactor_events_t desired_interests;

  switch (err) {
  case SSL_ERROR_NONE:
    set_client_state(s, cl, CLIENT_WAIT_FOR_HELLO);
    desired_interests = REACTOR_READABLE;
    break;
  case SSL_ERROR_WANT_READ:
    desired_interests = REACTOR_READABLE;
    break;
  case SSL_ERROR_WANT_WRITE:
    desired_interests = REACTOR_WRITABLE;
    break;
  default:
    nob_log(ERROR, "TLS handshake failed: ssl_error=%d", err);
    ERR_print_errors_fp(stderr);
    return FAITH_ERR_IO;
  }

  if (desired_interests == cl->reactor_source.interests)
    return FAITH_OK;

  _FH_CHECK_RETURN(
      reactor_modify_interests(&s->reactor, &cl->reactor_source, desired_interests));

  return FAITH_OK;
}

static void server_begin_shutdown(struct server_state_t *s) {
  struct client_conn_t *cl = s->clients;
  while (cl != NULL) {
    struct client_conn_t *next = cl->next;

    _FH_CHECK(server_disconnect_client(s, cl, FAITH_DISCONNECT_SERVER_SHUTDOWN,
                                       FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
                                       "The server has shut down"));
    cl = next;
  }
}

static bool server_has_clients(const struct server_state_t *s) {
  return s->clients != NULL;
}

static faith_status_code_t
server_flush_client_output(struct server_state_t *s, struct client_conn_t *cl) {
  if (!cl || !s)
    return FAITH_ERR_INVALID;

  transport_result_t res = 0;
  _FH_CHECK_RETURN(conn_flush_output(&cl->conn, &res));

  reactor_events_t desired_interests;

  switch (res) {
  case TRANSPORT_RES_WANT_READ:
    desired_interests = REACTOR_READABLE;
    break;
  case TRANSPORT_RES_WANT_WRITE:
    desired_interests = REACTOR_READABLE | REACTOR_WRITABLE;
    break;
  case TRANSPORT_RES_COMPLETE:
    desired_interests = REACTOR_READABLE;
    break;
  case TRANSPORT_RES_CLOSED:
    nob_log(ERROR, "Cannot flush client output, got TRANSPORT_RES_CLOSED; "
                   "Client connection is already closed.");
    return FAITH_ERR_CLOSED;
  case TRANSPORT_RES_ERROR:
    return FAITH_ERR_IO;
  default:
    nob_log(ERROR, "Got invalid transport result from conn_flush_output (%u).",
            (uint32_t)res);
    return FAITH_ERR_INVALID;
  }

  if (desired_interests == cl->reactor_source.interests)
    return FAITH_OK;

  _FH_CHECK_RETURN(reactor_modify_interests(&s->reactor, &cl->reactor_source,
                                            desired_interests));

  return FAITH_OK;
}

static int drive_client_read(struct server_state_t *s, struct client_conn_t *cl
                             ) {
  if (!s || !cl || cl->closing) 
    return -1;
  faith_frame_t frame;

  transport_result_t res = frame_try_full_read(&cl->conn, &frame);

  switch (res) {
  case TRANSPORT_RES_COMPLETE: {
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

    if (g_verbose_logging) {
      nob_log(INFO, "[client=%" PRIu64 " fd=%i] Success handling client frame.",
              cl->conn.id, cl->conn.fd);
    }

    reactor_events_t desired_interests =
        cl->reactor_source.interests | REACTOR_READABLE;

    if (cl->conn.out.buf && cl->conn.out.off < cl->conn.out.size) {
      desired_interests |= REACTOR_WRITABLE;
    }

    if (reactor_modify_interests(&s->reactor, &cl->reactor_source,
                                 desired_interests) != FAITH_OK) {
      goto fail_ev_mask;
    }

    return 0;
  }
  case TRANSPORT_RES_WANT_READ: {
    reactor_events_t desired_interests =
        cl->reactor_source.interests | REACTOR_READABLE;

    if (cl->conn.out.buf && cl->conn.out.off < cl->conn.out.size) {
      desired_interests |= REACTOR_WRITABLE;
    }

    if (reactor_modify_interests(&s->reactor, &cl->reactor_source,
                                 desired_interests) != FAITH_OK) {
      goto fail_ev_mask;
    }
    return 0;
  }

  case TRANSPORT_RES_CLOSED:
    server_disconnect_client(
        s, cl, FAITH_DISCONNECT_TEMPORARY_FAILURE,
        FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
        "Connection closed while reading incomplete frame.");
    return -1;
  default:
    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Failed to read client frame.",
            cl->conn.id, cl->conn.fd);

    server_disconnect_client(s, cl, FAITH_DISCONNECT_BAD_PROTOCOL,
                             FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                             "Server failed to read client frame.");
    return -1;
  }

fail_ev_mask:
  nob_log(ERROR, "[client=%" PRIu64 " fd=%d] modify_client_ev_mask failed: %s",
          cl->conn.id, cl->conn.fd, strerror(errno));

  server_disconnect_client(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                           FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                           "Failed to modify client event mask.");

  return -1;
}

static void handle_client_event(struct server_state_t *s,
                                struct client_conn_t  *cl,
                                reactor_events_t events, bool shutting_down) {
  if (!s || !cl)
    return;

  if (events & (REACTOR_CLOSED | REACTOR_ERROR)) {
    _FH_CHECK(close_client(s, &cl));
    return;
  }

  if (cl->closing) {
    _FH_CHECK(close_client(s, &cl));
    return;
  }

  bool dead = false;

  if (shutting_down || cl->close_after_flush) {
    if (conn_output_empty(&cl->conn)) {
      _FH_CHECK(close_client(s, &cl));
      return;
    }

    if (events & (REACTOR_READABLE | REACTOR_WRITABLE)) {
      if (server_flush_client_output(s, cl) != FAITH_OK)
        dead = true;
    }
  } else if (cl->state == CLIENT_HANDSHAKE) {
    if (events & (REACTOR_READABLE | REACTOR_WRITABLE)) {
      if (drive_tls_handshake(s, cl) != FAITH_OK)
        dead = true;
    }
  } else if (cl->state == CLIENT_OPEN || cl->state == CLIENT_WAIT_FOR_HELLO ||
             cl->state == CLIENT_WAIT_FOR_CHALLENGE_RESPONSE ||
             cl->state == CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE) {
    if ((events & REACTOR_READABLE) &&
        drive_client_read(s, cl) != FAITH_OK) {
      dead = true;
    }

    if (!dead && !cl->closing && (events & REACTOR_WRITABLE) &&
        server_flush_client_output(s, cl) != FAITH_OK) {
      dead = true;
    }
  }

  if (dead || cl->closing) {
    _FH_CHECK(close_client(s, &cl));
    return;
  }

  if (cl->close_after_flush && conn_output_empty(&cl->conn)) {
    _FH_CHECK(close_client(s, &cl));
  }
}

int loop(struct server_state_t *s) {
  bool     shutting_down = false;
  uint64_t shutdown_deadline_ms = 0;

  static reactor_event_data_t events[REACTOR_MAX_EVENTS];

  for (;;) {
    if (shutdown_requested && !shutting_down) {
      shutting_down = true;

      nob_log(INFO, "Server shutdown requested; notifying clients.");

      _FH_CHECK_SCOPED(reactor_remove(&s->reactor, &s->listen_source));

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

    size_t n_events = 0;
    _FH_CHECK(reactor_wait(&s->reactor, timeout_ms, events, &n_events));

    for (size_t i = 0; i < n_events; ++i) {
      reactor_source_t *src = events[i].src;

      if (!src) {
        nob_log(ERROR, "reactor_wait() returned an event with no source");
        continue;
      }

      switch (src->type) {
      case REACTOR_SOURCE_LISTENER:
        if (!shutting_down) {
          accept_clients(s);
        }
        break;
      case REACTOR_SOURCE_CLIENT:
        if (!src->user_data) {
          nob_log(ERROR, "client reactor source has no user_data");
          continue;
        }
        handle_client_event(s, (struct client_conn_t *)src->user_data,
                            events[i].events, shutting_down);
        break;
      default:
        nob_log(ERROR, "Unknown reactor source type: %u", src->type);
        break;
      }

    }
  }

  return 0;
}

static int init_listener(void) {
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
      g_verbose_logging = 1;
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

static int server_init(struct server_state_t *s) {
  if (!s)
    return 1;

  if (install_signal_handlers() != 0)
    exit(1);

  const tls_config_t cfg = (tls_config_t){
      .options = SSL_OP_IGNORE_UNEXPECTED_EOF | SSL_OP_NO_RENEGOTIATION |
                 SSL_OP_SERVER_PREFERENCE,
      .chain_file = "chain.pem",
      .pkey_file = "pkey.pem",
      .timeout = 3600,
      .cache_size = 1024,
      .verification_mode = SSL_VERIFY_NONE,
  };

  {
    _FH_CHECK(tls_init(&cfg, &s->tls));
    if (_fh_rc != FAITH_OK) {
      server_destroy(s);
      return 1;
    }
  }

  {
    _FH_CHECK(reactor_init(&s->reactor));
    if (_fh_rc != FAITH_OK) {
      server_destroy(s);
      return 1;
    }
  }

  int listen_fd = init_listener();
  if (listen_fd < 0) {
    server_destroy(s);
    return 1;
  }

  s->listen_source = (reactor_source_t){.type = REACTOR_SOURCE_LISTENER,
                                        .fd = listen_fd,
                                        .user_data = NULL,
                                        .interests = REACTOR_READABLE};

  {
    _FH_CHECK(reactor_add(&s->reactor, &s->listen_source));
    if (_fh_rc != FAITH_OK) {
      server_destroy(s);
      return 1;
    }
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
  tls_init_global();

  ignore_sigpipe();

  nob_set_log_handler(faith_log_handler);

  struct server_state_t s = {.next_client_id = 1};

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
