#include "protocol.h"

#include "../../third_party/nob.h"
#include "../../third_party/stb_ds.h"

#include "core.h"
#include "events.h"
#include "helpers.h"

#define _MODULE_NAME "codec/protocol"

faith_status_code_t faith_encode_frame(uint8_t *out_buf, size_t *out_size,
                                       size_t               buf_cap_in_bytes,
                                       const faith_frame_t *in) {
  size_t data_size = FAITH_FRAME_HEADER_SIZE + in->payload_size;
  FAITH_ENCODE_PROLOGUE(data_size);

  const faith_body_size_t frame_size =
      FAITH_FRAME_METADATA_SIZE + in->payload_size;

  if (in->payload_size > FAITH_MAX_PAYLOAD_SIZE)
    goto payload_too_large;

  if (frame_size > FAITH_MAX_FRAME_LEN)
    goto frame_too_large;

  size_t offset = 0;

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)frame_size);

  FAITH_ENCODE_U16_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint16_t)in->proto_ver);

  FAITH_ENCODE_U16_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint16_t)in->msg_type);

  if (in->payload_size > 0) {
    FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->payload,
                        in->payload_size);
  }

  FAITH_ENCODE_EPILOGUE(data_size, !=);

  return FAITH_OK;

frame_too_large:
  nob_log(ERROR,
          "[%s] Failed to encode frame; Frame is too large, "
          "frame_size=%u, MAX_FRAME_LEN=%i",
          _MODULE_NAME, (uint32_t)frame_size, (int32_t)FAITH_MAX_FRAME_LEN);
  return FAITH_ERR_TOO_LARGE;
payload_too_large:
  nob_log(ERROR,
          "[%s] Failed to encode frame; Payload is too large, "
          "payload_size=%u, MAX_PAYLOAD_SIZE=%i",

          _MODULE_NAME, (uint32_t)in->payload_size,
          (int32_t)FAITH_MAX_PAYLOAD_SIZE);
  return FAITH_ERR_TOO_LARGE;
}

faith_status_code_t faith_decode_frame(const uint8_t *payload,
                                       size_t         payload_size,
                                       faith_frame_t *out) {
  if (!payload) {
    if (payload_size == 0) {
      return FAITH_ERR_INCOMPLETE;
    }
    return FAITH_ERR_INVALID;
  }

  if (payload_size < FAITH_FRAME_HEADER_SIZE) {
    return FAITH_ERR_INCOMPLETE;
  }

  memset(out, 0, sizeof(*out));

  size_t offset = 0;

  uint32_t frame_size = 0;
  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, frame_size);

  if (frame_size < FAITH_FRAME_METADATA_SIZE)
    return FAITH_ERR_BAD_FRAME;

  const size_t frame_payload_size =
      (size_t)frame_size - FAITH_FRAME_METADATA_SIZE;

  if (frame_payload_size > FAITH_MAX_PAYLOAD_SIZE) {
    nob_log(ERROR,
            "[%s] Failed to parse frame from buffer; "
            "payload_size=%zu MAX_PAYLOAD_SIZE=%zu",
            _MODULE_NAME, payload_size, (size_t)FAITH_MAX_PAYLOAD_SIZE);

    return FAITH_ERR_TOO_LARGE;
  }

  if (frame_size > FAITH_MAX_FRAME_LEN) {
    nob_log(ERROR,
            "[%s] Failed to parse frame from buffer; Frame is too large, "
            "frame_size=%i MAX_FRAME_LEN=%i",
            _MODULE_NAME, (int32_t)frame_size, (int32_t)FAITH_MAX_FRAME_LEN);
    return FAITH_ERR_TOO_LARGE;
  }

  size_t total_frame_size = FAITH_FRAME_LENGTH_SIZE + frame_size;

  if (payload_size < total_frame_size) {
    return FAITH_ERR_INCOMPLETE;
  }

  uint16_t proto_ver = 0;
  FAITH_DECODE_U16_BE_RETURN(payload, payload_size, offset, proto_ver);

  uint16_t msg_type = 0;
  FAITH_DECODE_U16_BE_RETURN(payload, payload_size, offset, msg_type);

  if (proto_ver != FAITH_PROTO_VERSION)
    return FAITH_ERR_UNSUPPORTED_VER;

  out->proto_ver = proto_ver;
  out->msg_type = msg_type;
  out->frame_size = frame_size;
  out->payload_size = frame_size - FAITH_FRAME_METADATA_SIZE;

  faith_status_code_t _fh_result = FAITH_OK;

  if (out->payload_size > 0) {
    out->payload = malloc(out->payload_size);
    if (!out->payload)
      return FAITH_ERR_NOMEM;

    FAITH_DECODE_DEFER(payload, payload_size, offset, out->payload,
                       out->payload_size);
  }

  FAITH_DECODE_EPILOGUE_DEFER(FAITH_FRAME_HEADER_SIZE + out->payload_size, <);

  return FAITH_OK;
defer:
  free(out->payload);
  out->payload = NULL;
  return _fh_result;
}

void faith_free_frame(faith_frame_t *frame) {
  if (!frame)
    return;

  free(frame->payload);
  memset(frame, 0, sizeof(*frame));
}

faith_status_code_t faith_encode_ping(uint8_t                *out_buf,
                                      faith_body_size_t      *out_size,
                                      size_t                  buf_cap_in_bytes,
                                      const faith_msg_ping_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_MSG_PING_PAYLOAD_SIZE);

  size_t offset = 0;

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->nonce);
  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->client_sent_at_ms);

  FAITH_ENCODE_EPILOGUE(FAITH_MSG_PING_PAYLOAD_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_ping(const uint8_t    *payload,
                                      size_t            payload_size,
                                      faith_msg_ping_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_MSG_PING_PAYLOAD_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->nonce);
  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset,
                             out->client_sent_at_ms);

  FAITH_DECODE_EPILOGUE(FAITH_MSG_PING_PAYLOAD_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_pong(uint8_t                *out_buf,
                                      faith_body_size_t      *out_size,
                                      size_t                  buf_cap_in_bytes,
                                      const faith_msg_pong_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_MSG_PING_PAYLOAD_SIZE);

  size_t offset = 0;

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->nonce);
  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->server_sent_at_ms);

  FAITH_ENCODE_EPILOGUE(FAITH_MSG_PING_PAYLOAD_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_pong(const uint8_t    *payload,
                                      size_t            payload_size,
                                      faith_msg_pong_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_MSG_PING_PAYLOAD_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->nonce);
  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset,
                             out->server_sent_at_ms);

  FAITH_DECODE_EPILOGUE(FAITH_MSG_PING_PAYLOAD_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_envelope(uint8_t *out_buf, size_t *out_size,
                                          size_t buf_cap_in_bytes,
                                          const faith_envelope_t *in) {
  if (!out_buf || !out_size || !in)
    return FAITH_ERR_INVALID;

  *out_size = 0;

  if (in->body_size > 0 && !in->body)
    return FAITH_ERR_INVALID;

  const size_t in_size = FAITH_ENVL_HEADER_SIZE + (size_t)in->body_size;

  if (buf_cap_in_bytes < in_size)
    return FAITH_ERR_OVERFLOW;

  size_t offset = 0;

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->type);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->sender_id.bytes,
                      sizeof(in->sender_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->recipient_id.bytes,
                      sizeof(in->recipient_id.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->body_size);

  if (in->body_size > 0) {
    FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->body,
                        in->body_size);
  }

  if (offset != in_size)
    return FAITH_ERR_INVALID;

  *out_size = offset;
  return FAITH_OK;
}

faith_status_code_t faith_decode_envelope(const uint8_t    *payload,
                                          size_t            payload_size,
                                          faith_envelope_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_ENVL_HEADER_SIZE, <);

  size_t offset = 0;

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->type);

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->sender_id.bytes,
                      sizeof(out->sender_id.bytes));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->recipient_id.bytes,
                      sizeof(out->recipient_id.bytes));

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->body_size);

  if (out->body_size != payload_size - offset) {
    return FAITH_ERR_BAD_FRAME;
  }

  faith_status_code_t _fh_result = FAITH_OK;

  if (out->body_size != 0) {
    out->body = malloc(out->body_size);
    if (!out->body)
      return FAITH_ERR_NOMEM;

    FAITH_DECODE_DEFER(payload, payload_size, offset, out->body,
                       out->body_size);
  }

  FAITH_DECODE_EPILOGUE_DEFER(FAITH_ENVL_HEADER_SIZE + out->body_size, !=);

  return FAITH_OK;
defer:
  free(out->body);
  out->body = NULL;
  return _fh_result;
}

faith_status_code_t
faith_encode_device_link_req_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_device_link_req_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id.bytes,
                      sizeof(in->auth_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->public_key_new_device,
                      sizeof(in->public_key_new_device));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_new.bytes, sizeof(in->device_id_new.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->code,
                      sizeof(in->code));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->expires_at_ms);

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_device_link_req_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_device_link_req_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->auth_id.bytes,
                      sizeof(out->auth_id.bytes));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->public_key_new_device,
                      sizeof(out->public_key_new_device));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->device_id_new.bytes,
                      sizeof(out->device_id_new.bytes));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->code,
                      sizeof(out->code));

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->expires_at_ms);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_device_link_response_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_cts_device_link_response_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->signature_response,
                      sizeof(in->signature_response));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_new.bytes, sizeof(in->device_id_new.bytes));

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_device_link_response_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_cts_device_link_response_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->signature_response,
                      sizeof(out->signature_response));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->device_id_new.bytes,
                      sizeof(out->device_id_new.bytes));

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_client_disconnect_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_client_disconnect_t *in) {

  const size_t msg_len = strnlen(in->msg, sizeof(in->msg));

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED +
                        msg_len);

  size_t offset = 0;

  switch (in->reconnect_policy) {
#define X(name, value) case name:
    FAITH_CLIENT_DISCONNECT_POLICIES(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->reconnect_policy);

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->reason);

  switch (in->reason) {
#define X(name, value) case name:
    FAITH_CLIENT_DISCONNECT_REASONS(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->retry_after_ms);

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->banned_until_ms);

  FAITH_ENCODE_U16_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint16_t)msg_len);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->msg, msg_len);

  FAITH_ENCODE_EPILOGUE(
      FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED + msg_len, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_client_disconnect_body(const uint8_t    *payload,
                                    faith_body_size_t payload_size,
                                    faith_envl_stc_client_disconnect_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED, <);

  size_t offset = 0;

  switch (out->reconnect_policy) {
#define X(name, value) case name:
    FAITH_CLIENT_DISCONNECT_POLICIES(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset,
                             out->reconnect_policy);

  switch (out->reason) {
#define X(name, value) case name:
    FAITH_CLIENT_DISCONNECT_REASONS(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }
  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->reason);

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset,
                             out->retry_after_ms);

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset,
                             out->banned_until_ms);

  FAITH_DECODE_U16_BE_RETURN(payload, payload_size, offset, out->msg_size);

  if (out->msg_size > FAITH_MAX_CLIENT_DISCONNECT_MSG)
    return FAITH_ERR_BAD_FRAME;

  if ((size_t)out->msg_size != (size_t)payload_size - offset)
    return FAITH_ERR_BAD_FRAME;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->msg, out->msg_size);

  out->msg[out->msg_size] = '\0';

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED, <);

  return FAITH_OK;
}

faith_status_code_t faith_encode_hello_body(uint8_t           *out_buf,
                                            faith_body_size_t *out_size,
                                            size_t             buf_cap_in_bytes,
                                            const faith_envl_cts_hello_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_CTS_HELLO_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->device_id.bytes,
                      sizeof(in->device_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->public_key,
                      sizeof(in->public_key));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->client_nonce);

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_CTS_HELLO_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_hello_body(const uint8_t    *payload,
                                            faith_body_size_t payload_size,
                                            faith_envl_cts_hello_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_CTS_HELLO_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->device_id.bytes,
                      sizeof(out->device_id.bytes));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->public_key,
                      sizeof(out->public_key));

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->client_nonce);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_CTS_HELLO_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_encode_hello_challenge_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_hello_challenge_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_HELLO_CHALLENGE_BODY_SIZE);

  size_t offset = 0;

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->server_nonce);

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_HELLO_CHALLENGE_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_hello_challenge_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_hello_challenge_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_HELLO_CHALLENGE_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->server_nonce);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_HELLO_CHALLENGE_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_encode_command_body(uint8_t *out_buf, faith_body_size_t *out_size,
                          size_t                          buf_cap_in_bytes,
                          const faith_envl_cts_command_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_CTS_COMMAND_BODY_SIZE_FIXED +
                        in->payload_size);

  if (in->payload_size > FAITH_COMMAND_PAYLOAD_SIZE_MAX) {
    return FAITH_ERR_OVERFLOW;
  }

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->cmd_id.bytes,
                      sizeof(in->cmd_id.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->payload_size);

  if (in->payload_size > 0) {
    FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->payload,
                        in->payload_size);
  }

  FAITH_ENCODE_EPILOGUE(
      FAITH_ENVL_CTS_COMMAND_BODY_SIZE_FIXED + in->payload_size, !=);

  return FAITH_OK;
}
faith_status_code_t faith_decode_command_body(const uint8_t    *payload,
                                              faith_body_size_t payload_size,
                                              faith_envl_cts_command_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_CTS_COMMAND_BODY_SIZE_FIXED, <);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->cmd_id.bytes,
                      sizeof(out->cmd_id.bytes));

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->type);

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->payload_size);

  if (out->payload_size > FAITH_COMMAND_PAYLOAD_SIZE_MAX)
    return FAITH_ERR_BAD_ENVELOPE;

  faith_status_code_t _fh_result = FAITH_OK;

  if (out->payload_size > 0) {
    out->payload = malloc(out->payload_size);
    if (!out->payload)
      return FAITH_ERR_NOMEM;

    FAITH_DECODE_DEFER(payload, payload_size, offset, out->payload,
                       out->payload_size);
  }

  FAITH_DECODE_EPILOGUE_DEFER(
      FAITH_ENVL_CTS_COMMAND_BODY_SIZE_FIXED + out->payload_size, !=);

  return FAITH_OK;

defer:
  free(out->payload);
  out->payload = NULL;
  return _fh_result;
}

faith_status_code_t
faith_encode_command_result_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                 size_t buf_cap_in_bytes,
                                 const faith_envl_stc_command_result_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_COMMAND_RESULT_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->cmd_id.bytes,
                      sizeof(in->cmd_id.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->err);

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->result);

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_COMMAND_RESULT_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_command_result_body(const uint8_t                   *payload,
                                 faith_body_size_t                payload_size,
                                 faith_envl_stc_command_result_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_COMMAND_RESULT_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->cmd_id.bytes,
                      sizeof(out->cmd_id.bytes));

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->type);

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->err);

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->result);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_COMMAND_RESULT_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_event_body(uint8_t           *out_buf,
                                            faith_body_size_t *out_size,
                                            size_t             buf_cap_in_bytes,
                                            const faith_envl_stc_event_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + in->data_size);

  if (in->data_size != 0 && !in->data) {
    return FAITH_ERR_INVALID;
  }

  size_t offset = 0;

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->seq_num);
  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);
  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->data_size);

  if (in->data_size > 0) {
    FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->data,
                        in->data_size);
  }

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + in->data_size,
                        !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_event_body(const uint8_t    *payload,
                                            faith_body_size_t payload_size,
                                            faith_envl_stc_event_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED, <);

  size_t offset = 0;

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->seq_num);
  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->type);
  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->data_size);

  faith_status_code_t _fh_result = FAITH_OK;

  if (out->data_size > 0) {
    out->data = malloc(out->data_size);
    if (!out->data)
      return FAITH_ERR_NOMEM;

    FAITH_DECODE_DEFER(payload, payload_size, offset, out->data,
                       out->data_size);
  }

  FAITH_DECODE_EPILOGUE_DEFER(
      FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + out->data_size, !=);

  return FAITH_OK;
defer:
  free(out->data);
  out->data = NULL;
  return _fh_result;
}

faith_status_code_t
faith_codec_event_batch_data_size(faith_envl_stc_event_t *events,
                                  uint16_t                n_events,
                                  faith_body_size_t      *o_size) {
  if (!events || !o_size)
    return FAITH_ERR_INVALID;

  *o_size = 0;

  if (n_events > FAITH_EVENT_BATCH_MAX_EVENTS) {
    nob_log(ERROR,
            "[%s] Invalid EVENT_BATCH body: event count %" PRIu16
            " exceeds maximum %u.",
            _MODULE_NAME, n_events, FAITH_EVENT_BATCH_MAX_EVENTS);
    return FAITH_ERR_INVALID;
  }

  size_t total_data_size = 0;

  if (n_events != 0) {
    for (uint16_t i = 0; i < n_events; i++) {
      size_t body_size = (size_t)FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED +
                         (size_t)events[i].data_size;

      if (body_size > UINT16_MAX) {
        nob_log(ERROR,
                "[%s] Cannot encode EVENT_BATCH: event %" PRIu16
                " body size (%zu bytes) exceeds UINT16_MAX.",
                _MODULE_NAME, i, body_size);
        return FAITH_ERR_TOO_LARGE;
      }

      size_t encoded_elem_size = sizeof(uint16_t) + body_size;

      if (encoded_elem_size > UINT32_MAX - total_data_size) {
        nob_log(ERROR,
                "[%s] Cannot encode EVENT_BATCH: adding event %" PRIu16
                " would make the total event data exceed UINT32_MAX.",
                _MODULE_NAME, i);
        return FAITH_ERR_TOO_LARGE;
      }

      total_data_size += encoded_elem_size;
    }
  }

  if (total_data_size > SIZE_MAX - FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED)
    return FAITH_ERR_TOO_LARGE;

  *o_size = total_data_size;

  return FAITH_OK;
}

faith_status_code_t
faith_encode_event_batch_body(uint8_t *out_buf, faith_body_size_t *out_size,
                              size_t buf_cap_in_bytes,
                              const faith_envl_stc_event_batch_t *in) {
  if (!in || (!in->events && in->n_events != 0))
    return FAITH_ERR_INVALID;

  size_t encoded_size =
      FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED + in->events_data_size;

  FAITH_ENCODE_PROLOGUE(encoded_size);

  size_t offset = 0;

  FAITH_ENCODE_U16_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->n_events);
  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->events_data_size);

  for (uint16_t i = 0; i < in->n_events; i++) {
    size_t body_size = (size_t)FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED +
                       (size_t)in->events[i].data_size;

    /* encode element size */
    FAITH_ENCODE_U16_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                               (uint16_t)body_size);

    faith_body_size_t returned_body_size = 0;
    /* encode element body*/
    _FH_CHECK_RETURN(
        faith_encode_event_body(out_buf + offset, &returned_body_size,
                                buf_cap_in_bytes - offset, &in->events[i]));

    if ((size_t)returned_body_size != body_size) {
      nob_log(ERROR,
              "[%s] EVENT encoder returned an unexpected body size: "
              "expected %zu bytes, got %" PRIu32 " bytes.",
              _MODULE_NAME, body_size, returned_body_size);
      return FAITH_ERR_INVALID;
    }
    offset += body_size;
  }

  FAITH_ENCODE_EPILOGUE(encoded_size, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_event_batch_body(const uint8_t                *payload,
                              faith_body_size_t             payload_size,
                              faith_envl_stc_event_batch_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED, <);

  out->n_events = 0;
  out->events_data_size = 0;
  out->events = NULL;

  size_t offset = 0;

  FAITH_DECODE_U16_BE_RETURN(payload, payload_size, offset, out->n_events);

  if (out->n_events > FAITH_EVENT_BATCH_MAX_EVENTS) {
    nob_log(ERROR,
            "[%s] Invalid EVENT_BATCH body: event count %" PRIu16
            " exceeds maximum %u.",
            _MODULE_NAME, out->n_events, FAITH_EVENT_BATCH_MAX_EVENTS);
    return FAITH_ERR_BAD_ENVELOPE;
  }

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset,
                             out->events_data_size);

  size_t decoded_size = (size_t)FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED +
                        (size_t)out->events_data_size;

  if ((size_t)payload_size < decoded_size) {
    nob_log(ERROR,
            "[%s] Invalid EVENT_BATCH body: declared events data size (%" PRIu32
            " bytes) exceeds remaining payload size (%" PRIu32 " bytes).",
            _MODULE_NAME, out->events_data_size,
            payload_size - FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED);

    return FAITH_ERR_OVERFLOW;
  }

  size_t events_end = offset + (size_t)out->events_data_size;

  if (events_end != (size_t)payload_size) {
    nob_log(ERROR,
            "[%s] Invalid EVENT_BATCH body: payload has %zu trailing bytes.",
            _MODULE_NAME, (size_t)payload_size - events_end);
    return FAITH_ERR_BAD_ENVELOPE;
  }

  faith_status_code_t _fh_result = FAITH_OK;

  if (out->n_events != 0) {
    arrsetlen(out->events, out->n_events);
  }

  uint16_t decoded_events = 0;
  for (uint16_t i = 0; i < out->n_events; i++) {
    /* decode element size */
    uint16_t elem_size = 0;
    FAITH_DECODE_U16_BE_DEFER(payload, payload_size, offset, elem_size);

    if (elem_size < FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED) {
      nob_log(ERROR,
              "[%s] Invalid EVENT_BATCH body: event %" PRIu16
              " declares an invalid body size of %" PRIu16
              " bytes. The minimum required size for an event is: %" PRIu32
              " bytes.",
              _MODULE_NAME, i, elem_size, FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED);
      _FH_RETURN_DEFER(FAITH_ERR_BAD_ENVELOPE);
    }

    if (offset > payload_size || payload_size - offset < (size_t)elem_size) {
      nob_log(ERROR,
              "[%s] Invalid EVENT_BATCH body: event %" PRIu16
              " declares %" PRIu16 " bytes, but only %zu bytes remain.",
              _MODULE_NAME, i, elem_size, events_end - offset);
      _FH_RETURN_DEFER(FAITH_ERR_BAD_FRAME);
    }

    /* decode element body */
    _FH_CHECK_DEFER(faith_decode_event_body(
        payload + offset, (faith_body_size_t)elem_size, &out->events[i]));

    decoded_events++;

    offset += (size_t)elem_size;
  }

  FAITH_DECODE_EPILOGUE_DEFER(decoded_size, !=);

  return FAITH_OK;

defer:
  for (size_t i = 0; i < decoded_events; i++) {
    free(out->events[i].data);
    out->events[i].data = NULL;
  }
  arrfree(out->events);
  out->events_data_size = 0;
  out->n_events = 0;
  out->events = NULL;

  return _fh_result;
}

faith_status_code_t
faith_encode_event_ack_body(uint8_t *out_buf, faith_body_size_t *out_size,
                            size_t                            buf_cap_in_bytes,
                            const faith_envl_cts_event_ack_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_CTS_EVENT_ACK_BODY_SIZE);
  size_t offset = 0;

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->seq_num);
  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_CTS_EVENT_ACK_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_event_ack_body(const uint8_t              *payload,
                            faith_body_size_t           payload_size,
                            faith_envl_cts_event_ack_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_CTS_EVENT_ACK_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->seq_num);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_CTS_EVENT_ACK_BODY_SIZE, !=);

  return FAITH_OK;
}
