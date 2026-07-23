#include "client_io.h"
#include "client_lifecycle.h"
#include <openssl/ssl.h>

#include "../logging/logging.h"
#include "../transport/frame.h"

#include "../core/envelopes.h"

#include "dispatch.h"

static faith_status_code_t
handle_client_frame(server_state_t *s, client_conn_t *cl, faith_frame_t *frame);

static faith_status_code_t handle_client_frame(server_state_t *s,
                                               client_conn_t  *cl,
                                               faith_frame_t  *frame) {
  if (!s || !cl || !frame)
    return FAITH_ERR_INVALID;

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server got frame: msg_type=%s payload_size=%u",
          cl->conn.id, cl->conn.fd, faith_frame_msg_name(frame->msg_type),
          frame->payload_size);

  switch (frame->msg_type) {
  case FAITH_MSG_PING:
    _FH_CHECK_RETURN(server_handle_ping(s, cl, frame));
    break;
  case FAITH_MSG_ENVL:
    _FH_CHECK_RETURN(server_dispatch_envelope(s, cl, frame));
    break;
  default:
    return FAITH_ERR_BAD_FRAME;
  }

  return FAITH_OK;
}

faith_status_code_t server_queue_frame(server_state_t *s, client_conn_t *cl,
                                       const faith_frame_t *frame) {
  if (!cl || cl->closing || !s || !frame ||
      (!frame->payload && frame->payload_size != 0))
    return FAITH_ERR_INVALID;

  const size_t wire_size = FAITH_FRAME_HEADER_SIZE + frame->payload_size;
  uint8_t     *wire_data = malloc(wire_size);
  if (!wire_data) {
    return FAITH_ERR_NOMEM;
  }

  faith_status_code_t _fh_result = FAITH_OK;

  size_t wire_size_returned = 0;
  // Allocates memory on pointer passed to <out_data>, which is <wire_data> here
  _FH_CHECK_DEFER(
      faith_encode_frame(wire_data, &wire_size_returned, wire_size, frame));

  if (wire_size_returned != wire_size) {
    _FH_RETURN_DEFER(FAITH_ERR_BAD_FRAME);
  }

  _FH_CHECK_DEFER(
      conn_queue_enqueue_bytes(&cl->conn.out, wire_data, wire_size));

  free(wire_data);

  if (!(cl->reactor_source.interests & REACTOR_WRITABLE)) {
    _FH_CHECK(reactor_modify_interests(&s->reactor, &cl->reactor_source,
                                       cl->reactor_source.interests |
                                           REACTOR_WRITABLE));
    if (_fh_rc != FAITH_OK) {
      server_client_queue_disconnect(
          s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
          FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
          "Failed to enable REACTOR_WRITABLE for the client.");
      return _fh_rc;
    }
  }

  nob_log(INFO, "[client=%" PRIu64 " fd=%d] queued frame: %s (%zu bytes)",
          cl->conn.id, cl->conn.fd, faith_frame_msg_name(frame->msg_type),
          wire_size);

  return FAITH_OK;
defer:
  free(wire_data);
  return _fh_result;
}

faith_status_code_t server_queue_envelope(server_state_t *s, client_conn_t *cl,
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

  faith_frame_t frame = {0};
  frame.msg_type = FAITH_MSG_ENVL;
  frame.proto_ver = FAITH_PROTO_VERSION;
  frame.payload = payload;
  frame.payload_size = payload_size;

  _FH_CHECK_DEFER(server_queue_frame(s, cl, &frame));

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server enqueued envelope: type=%s body_size=%u",
          cl->conn.id, cl->conn.fd, faith_envelope_name(envl->type),
          envl->body_size);

defer:
  free(payload);
  return _fh_result;
}

faith_status_code_t
server_queue_envelope_or_mark_dead(server_state_t *s, client_conn_t *cl,
                                   const faith_envelope_t *envl) {
  _FH_CHECK(server_queue_envelope(s, cl, envl));
  if (_fh_rc == FAITH_OK)
    return FAITH_OK;

  nob_log(ERROR, "[client=%" PRIu64 " fd=%d] Failed to queue %s: %s",
          cl->conn.id, cl->conn.fd, faith_envelope_name(envl->type),
          faith_status_code_name(_fh_rc));

  cl->closing = true;
  return _fh_rc;
}

faith_status_code_t server_flush_client_output(server_state_t       *s,
                                               struct client_conn_t *cl) {
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

faith_status_code_t server_drive_tls_handshake(server_state_t *s,
                                               client_conn_t  *cl) {
  if (!s || !cl || cl->closing)
    return FAITH_ERR_INVALID;

  int err = tls_accept(&cl->conn.tls);

  if (err == INT_MAX) {
    return FAITH_ERR_INVALID;
  }

  reactor_events_t desired_interests;

  switch (err) {
  case SSL_ERROR_NONE:
    server_set_client_state(s, cl, CLIENT_WAIT_FOR_HELLO);
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

  _FH_CHECK_RETURN(reactor_modify_interests(&s->reactor, &cl->reactor_source,
                                            desired_interests));

  return FAITH_OK;
}

faith_status_code_t server_drive_client_read(server_state_t *s,
                                             client_conn_t  *cl) {
  if (!s || !cl || cl->closing)
    return -1;
  faith_frame_t frame;

  transport_result_t res = frame_try_full_read(&cl->conn, &frame);

  switch (res) {
  case TRANSPORT_RES_COMPLETE: {

    _FH_CHECK(handle_client_frame(s, cl, &frame));
    faith_free_frame(&frame);

    if (_fh_rc != FAITH_OK) {
      server_client_queue_disconnect(
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
    server_client_queue_disconnect(
        s, cl, FAITH_DISCONNECT_TEMPORARY_FAILURE,
        FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
        "Connection closed while reading incomplete frame.");
    return -1;
  default:
    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Failed to read client frame.",
            cl->conn.id, cl->conn.fd);

    server_client_queue_disconnect(s, cl, FAITH_DISCONNECT_BAD_PROTOCOL,
                                   FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                                   "Server failed to read client frame.");
    return -1;
  }

fail_ev_mask:
  nob_log(ERROR,
          "[client=%" PRIu64 " fd=%d] reactor_modify_interests failed: %s",
          cl->conn.id, cl->conn.fd, strerror(errno));

  server_client_queue_disconnect(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                                 FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                                 "Failed to modify client event mask.");

  return -1;
}
