#include "protocol.h"

#include "../../third_party/nob.h"

#include "helpers.h"

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
          "Failed to encode frame; Frame is too large, "
          "frame_size=%u, MAX_FRAME_LEN=%i",
          (uint32_t)frame_size, (int32_t)FAITH_MAX_FRAME_LEN);
  return FAITH_ERR_FRAME_TOO_LARGE;
payload_too_large:
  nob_log(ERROR,
          "Failed to encode frame; Payload is too large, "
          "payload_size=%u, MAX_PAYLOAD_SIZE=%i",
          (uint32_t)in->payload_size, (int32_t)FAITH_MAX_PAYLOAD_SIZE);
  return FAITH_ERR_FRAME_TOO_LARGE;
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
            "Failed to parse frame from buffer; "
            "payload_size=%zu MAX_PAYLOAD_SIZE=%zu",
            payload_size, (size_t)FAITH_MAX_PAYLOAD_SIZE);

    return FAITH_ERR_FRAME_TOO_LARGE;
  }

  if (frame_size > FAITH_MAX_FRAME_LEN) {
    nob_log(ERROR,
            "Failed to parse frame from buffer; Frame is too large, "
            "frame_size=%i MAX_FRAME_LEN=%i",
            (int32_t)frame_size, (int32_t)FAITH_MAX_FRAME_LEN);
    return FAITH_ERR_FRAME_TOO_LARGE;
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

  FAITH_DECODE_EPILOGUE_DNY(FAITH_FRAME_HEADER_SIZE, <);

  out->proto_ver = proto_ver;
  out->msg_type = msg_type;
  out->frame_size = frame_size;
  out->payload_size = frame_size - FAITH_FRAME_METADATA_SIZE;

  if (out->payload_size > 0) {
    out->payload = malloc(out->payload_size);
    if (!out->payload)
      return FAITH_ERR_NOMEM;

    memcpy(out->payload, payload + FAITH_FRAME_HEADER_SIZE, out->payload_size);
  }

  return FAITH_OK;
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

  FAITH_DECODE_EPILOGUE_DNY(FAITH_ENVL_HEADER_SIZE, <);

  if (out->body_size != payload_size - offset) {
    return FAITH_ERR_BAD_FRAME;
  }

  uint8_t *body = NULL;
  if (out->body_size != 0) {
    body = malloc(out->body_size);
    if (!body)
      return FAITH_ERR_NOMEM;

    memcpy(body, payload + offset, out->body_size);
    offset += out->body_size;
  }

  if (offset != payload_size) {
    free(body);
    return FAITH_ERR_BAD_FRAME;
  }

  out->body = body;

  return FAITH_OK;
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

  if (out->payload_size > 0) {
    out->payload = malloc(out->payload_size);
    if (!out->payload)
      return FAITH_ERR_NOMEM;

    memcpy(out->payload, payload + offset, out->payload_size);
  }

  FAITH_DECODE_EPILOGUE_DNY(FAITH_ENVL_CTS_COMMAND_BODY_SIZE_FIXED, <);

  return FAITH_OK;
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

  FAITH_DECODE_EPILOGUE_DNY(FAITH_ENVL_STC_COMMAND_RESULT_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_event_body(uint8_t           *out_buf,
                                            faith_body_size_t *out_size,
                                            size_t             buf_cap_in_bytes,
                                            const faith_envl_stc_event_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + in->data_size);

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

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + in->data_size, !=);

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
  if (out->data_size > 0) {
    out->data = malloc(out->data_size);
    if (!out->data)
      return FAITH_ERR_NOMEM;

    memcpy(out->data, payload + offset, out->data_size);
  }

  FAITH_DECODE_EPILOGUE_DNY(FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED, <);

  return FAITH_OK;
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
