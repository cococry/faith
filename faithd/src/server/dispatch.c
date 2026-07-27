#include "dispatch.h"
#include "../core/core.h"

#include "client_io.h"
#include "server.h"

#include "../auth/device_link.h"
#include "../auth/handshake.h"

#include "../delivery/routing.h"

#include "../commands/dispatch.h"

faith_status_code_t server_dispatch_frame(server_state_t *s, client_conn_t *cl,
                                          faith_frame_t *frame) {
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

faith_status_code_t server_dispatch_envelope(server_state_t *s,
                                             client_conn_t  *cl,
                                             faith_frame_t  *frame) {
  if (!s || !frame || !cl)
    return FAITH_ERR_INVALID;

  faith_envelope_t envl = {0};

  faith_status_code_t _fh_result = FAITH_OK;

  _FH_CHECK_DEFER(
      faith_decode_envelope(frame->payload, frame->payload_size, &envl));

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server is handling envelope: type=%s body_size=%u",
          cl->conn.id, cl->conn.fd, faith_envelope_name(envl.type),
          envl.body_size);

  switch (envl.type) {
  case FAITH_ENVELOPE_HELLO:
    _FH_CHECK_DEFER(auth_handle_hello(s, cl, &envl));
    break;

  case FAITH_ENVELOPE_CHALLENGE_RESPONSE:
    _FH_CHECK_DEFER(auth_handle_challenge_response(s, cl, &envl));
    break;

  case FAITH_ENVELOPE_MSG_SEND:
    _FH_CHECK_DEFER(delivery_route_application_msg_envelope(s, cl, &envl));
    break;

  case FAITH_ENVELOPE_DEVICE_AUTH_APPROVE:
  case FAITH_ENVELOPE_DEVICE_AUTH_DENY:
    _FH_CHECK_DEFER(device_link_handle_device_response(s, cl, &envl));
    break;
  case FAITH_ENVELOPE_COMMAND:
    _FH_CHECK_DEFER(command_dispatch(s, cl, &envl));
    break;
  default:
    _FH_RETURN_DEFER(FAITH_ERR_BAD_ENVELOPE);
  }

  nob_log(INFO,
          "[client=%" PRIu64
          " fd=%i] Server successfully handled envelope: type=%s body_size=%u",
          cl->conn.id, cl->conn.fd, faith_envelope_name(envl.type),
          envl.body_size);

defer:
  if (envl.body != NULL)
    free(envl.body);

  return _fh_result;
}

faith_status_code_t server_handle_ping(server_state_t *s, client_conn_t *cl,
                                       faith_frame_t *frame) {
  if (!s || !cl || cl->closing || !frame)
    return FAITH_ERR_INVALID;

  faith_msg_ping_t ping = {0};

  /* 1. decode PING */
  _FH_CHECK_RETURN(
      faith_decode_ping(frame->payload, frame->payload_size, &ping));

  nob_log(INFO,
          "[client=%" PRIu64 " fd=%i] server got PING: nonce=%" PRIu64
          ", client_sent_at_ms=%lu",
          cl->conn.id, cl->conn.fd, ping.nonce, ping.client_sent_at_ms);

  /* 2. Send PONG over wire protocol */
  uint64_t server_sent_at_ms = faith_now_ms();

  faith_body_size_t payload_size = 0;
  uint8_t           payload[FAITH_MSG_PONG_PAYLOAD_SIZE] = {0};

  faith_msg_pong_t pong = {.server_sent_at_ms = server_sent_at_ms,
                           .nonce = ping.nonce};

  _FH_CHECK_RETURN(
      faith_encode_pong(payload, &payload_size, sizeof(payload), &pong));

  faith_frame_t pong_frame = {0};
  pong_frame.msg_type = FAITH_MSG_PONG;
  pong_frame.proto_ver = FAITH_PROTO_VERSION;
  pong_frame.payload = payload;
  pong_frame.payload_size = payload_size;

  _FH_CHECK_RETURN(server_queue_frame(s, cl, &pong_frame));

  nob_log(
      INFO,
      "[client=%" PRIu64
      " fd=%i] Server sent PONG to client. nonce=%lu, server_sent_at_ms=%lu",
      cl->conn.id, cl->conn.fd, ping.nonce, server_sent_at_ms);

  return FAITH_OK;
}
