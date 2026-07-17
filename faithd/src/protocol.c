#include "shared.h"

#include <openssl/rand.h>
#include <openssl/ssl.h>
#include <stdint.h>
#include <string.h>

inline uint16_t faith_version_pack(uint8_t major, uint8_t minor,
                                   uint8_t patch) {
  return ((uint16_t)(major & 0x1f) << 11) | ((uint16_t)(minor & 0x1f) << 6) |
         ((uint16_t)(patch & 0x3f));
}

inline uint8_t faith_version_major(uint16_t v) {
  return (uint8_t)((v >> 11) & 0x1f);
}

inline uint8_t faith_version_minor(uint16_t v) {
  return (uint8_t)((v >> 6) & 0x1f);
}

inline uint8_t faith_version_patch(uint16_t v) { return (uint8_t)(v & 0x3f); }

const char *faith_strerror(int code) { return strerror(code); }

faith_status_code_t faith_write_bytes_sync(SSL *ssl, const uint8_t *buf,
                                           size_t size) {
  if (!ssl || !buf)
    return FAITH_ERR_INVALID;

  size_t total = 0;

  while (total < size) {
    size_t written = 0;

    int ok = SSL_write_ex(ssl, buf + total, size - total, &written);
    if (ok != 1) {
      int err = SSL_get_error(ssl, ok);

      if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE)
        continue;

      if (err == SSL_ERROR_ZERO_RETURN)
        return FAITH_ERR_CLOSED;

      ERR_print_errors_fp(stderr);
      return FAITH_ERR_IO;
    }

    total += written;
  }

  return FAITH_OK;
}

faith_status_code_t faith_read_bytes_sync(SSL *ssl, uint8_t *buf, size_t size) {
  if (!ssl || !buf) {
    return FAITH_ERR_INVALID;
  }

  size_t total = 0;

  while (total < size) {
    size_t nread = 0;
    int    ok = SSL_read_ex(ssl, buf + total, size - total, &nread);

    if (ok == 1) {
      total += nread;
      continue;
    }

    int err = SSL_get_error(ssl, 0);

    switch (err) {
    case SSL_ERROR_WANT_READ:
    case SSL_ERROR_WANT_WRITE:
      continue;

    case SSL_ERROR_ZERO_RETURN:
      return FAITH_ERR_CLOSED;

    case SSL_ERROR_SYSCALL:
      return FAITH_ERR_CLOSED;

    case SSL_ERROR_SSL: {
      unsigned long e = ERR_peek_error();

      if (ERR_GET_REASON(e) == SSL_R_UNEXPECTED_EOF_WHILE_READING) {
        ERR_clear_error();
        return FAITH_ERR_CLOSED;
      }

      nob_log(ERROR,
              "SSL_read_ex failed: ssl_err=%d, nread=%zu, total=%zu, want=%zu, "
              "errno=%d (%s)\n",
              err, nread, total, size - total, errno, strerror(errno));
      ERR_print_errors_fp(stderr);
      return FAITH_ERR_SSL;
    }

    default: {

      nob_log(ERROR,
              "SSL_read_ex failed: ssl_err=%d, nread=%zu, total=%zu, want=%zu, "
              "errno=%d (%s)\n",
              err, nread, total, size - total, errno, strerror(errno));
      return FAITH_ERR_IO;
    }
    }
  }

  return FAITH_OK;
}

void faith_frame_free(faith_frame_t *f) {
  if (!f)
    return;

  free(f->payload);
  memset(f, 0, sizeof(*f));
}

faith_status_code_t faith_read_frame_sync(SSL *ssl, faith_frame_t *out) {
  if (!ssl || !out)
    return FAITH_ERR_INVALID;

  uint8_t len_buf[4];
  uint8_t hdr_buf[4];

  memset(out, 0, sizeof(*out));

  faith_status_code_t read_rc =
      faith_read_bytes_sync(ssl, len_buf, sizeof(len_buf));

  if (read_rc == FAITH_ERR_SSL || read_rc == FAITH_ERR_INVALID ||
      read_rc == FAITH_ERR_IO)
    return read_rc;
  else if (read_rc == FAITH_ERR_CLOSED) {
    return FAITH_OK;
  }

  const uint32_t frame_size = faith_read_u32_be(len_buf);

  if (frame_size < FAITH_FRAME_METADATA_SIZE)
    return FAITH_ERR_BAD_FRAME;

  if (frame_size > FAITH_MAX_FRAME_LEN) {
    nob_log(ERROR,
            "Failed to read frame; Frame is too large, "
            "frame_size=%i MAX_FRAME_LEN=%i",
            (int32_t)frame_size, (int32_t)FAITH_MAX_FRAME_LEN);
    return FAITH_ERR_FRAME_TOO_LARGE;
  }

  _FH_CHECK_RETURN(faith_read_bytes_sync(ssl, hdr_buf, sizeof(hdr_buf)));

  out->frame_size = frame_size;
  out->proto_ver = faith_read_u16_be(hdr_buf);
  out->msg_type = faith_read_u16_be(hdr_buf + sizeof(uint16_t));
  out->payload_size = frame_size - FAITH_FRAME_METADATA_SIZE;

  if (out->proto_ver != FAITH_PROTO_VERSION)
    return FAITH_ERR_UNSUPPORTED_VER;

  if (out->payload_size > FAITH_MAX_PAYLOAD_SIZE) {
    nob_log(ERROR,
            "Failed to read frame; Frame is too large, "
            "frame_size=%zu MAX_FRAME_LEN=%i",
            out->payload_size, (int32_t)FAITH_MAX_FRAME_LEN);

    return FAITH_ERR_FRAME_TOO_LARGE;
  }

  if (out->payload_size == 0)
    return FAITH_OK;

  out->payload = malloc(out->payload_size);
  if (!out->payload)
    return FAITH_ERR_NOMEM;

  faith_status_code_t rc =
      faith_read_bytes_sync(ssl, out->payload, out->payload_size);

  if (rc != FAITH_OK) {
    faith_frame_free(out);
    return rc;
  }

  return FAITH_OK;
}

faith_status_code_t faith_write_frame_sync(SSL                   *ssl,
                                           faith_frame_msg_type_t type,
                                           const uint8_t         *payload,
                                           size_t payload_size) {
  if (!ssl)
    return FAITH_ERR_INVALID;

  uint8_t *data = NULL;
  size_t   data_size = 0;

  faith_status_code_t rc =
      faith_encode_frame(type, payload, payload_size, &data, &data_size);

  if (rc != FAITH_OK)
    return rc;

  rc = faith_write_bytes_sync(ssl, data, data_size);

  free(data);

  if (rc != FAITH_OK) {
    nob_log(ERROR, "faith_ssl_write_bytes failed: %s (%d)",
            faith_status_code_name(rc), (int)rc);
    return rc;
  }

  return FAITH_OK;
}

faith_status_code_t faith_encode_frame(faith_frame_msg_type_t type,
                                       const uint8_t         *payload,
                                       size_t payload_size, uint8_t **out_data,
                                       size_t *out_size) {
  if (!out_data || !out_size)
    return FAITH_ERR_INVALID;

  *out_data = NULL;
  *out_size = 0;

  if (payload_size > 0 && !payload)
    return FAITH_ERR_INVALID;

  const size_t frame_size = FAITH_FRAME_METADATA_SIZE + payload_size;

  if (payload_size > FAITH_MAX_PAYLOAD_SIZE)
    goto payload_too_large;

  if (payload_size > SIZE_MAX - FAITH_FRAME_METADATA_SIZE)
    goto frame_too_large;

  if (frame_size > UINT32_MAX || frame_size > FAITH_MAX_FRAME_LEN)
    goto frame_too_large;

  if (frame_size > SIZE_MAX - FAITH_FRAME_LENGTH_SIZE)
    goto frame_too_large;

  const size_t data_size = FAITH_FRAME_LENGTH_SIZE + frame_size;

  uint8_t *data = malloc(data_size);
  if (!data)
    return FAITH_ERR_NOMEM;

  size_t offset = 0;

  faith_status_code_t rc =
      faith_write_u32_be(data + offset, (uint32_t)frame_size);
  if (rc != FAITH_OK)
    goto fail;
  offset += sizeof(uint32_t);

  rc = faith_write_u16_be(data + offset, FAITH_PROTO_VERSION);
  if (rc != FAITH_OK)
    goto fail;
  offset += sizeof(uint16_t);

  rc = faith_write_u16_be(data + offset, (uint16_t)type);
  if (rc != FAITH_OK)
    goto fail;
  offset += sizeof(uint16_t);

  if (payload_size > 0) {
    memcpy(data + offset, payload, payload_size);
    offset += payload_size;
  }

  if (offset != data_size) {
    rc = FAITH_ERR_INVALID;
    goto fail;
  }

  *out_data = data;
  *out_size = data_size;

  return FAITH_OK;

fail:
  free(data);
  return rc;
frame_too_large:
  nob_log(ERROR,
          "Failed to encode frame; Frame is too large, "
          "frame_size=%zu, MAX_FRAME_LEN=%i",
          frame_size, (int32_t)FAITH_MAX_FRAME_LEN);
  return FAITH_ERR_FRAME_TOO_LARGE;
payload_too_large:
  nob_log(ERROR,
          "Failed to encode frame; Payload is too large, "
          "payload_size=%zu, MAX_PAYLOAD_SIZE=%i",
          payload_size, (int32_t)FAITH_MAX_PAYLOAD_SIZE);
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

uint64_t faith_now_ms() {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);

  uint64_t total_ms =
      (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
  return total_ms;
}

inline faith_status_code_t faith_write_u64_be(uint8_t *out_buf, uint64_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline faith_status_code_t faith_write_u32_be(uint8_t *out_buf, uint32_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline faith_status_code_t faith_write_u16_be(uint8_t *out_buf, uint16_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline uint16_t faith_read_u16_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint16_t)p[0] << 8) | ((uint16_t)p[1]);
}

inline uint32_t faith_read_u32_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
         ((uint32_t)p[2] << 8) | ((uint32_t)p[3]);
}

inline uint64_t faith_read_u64_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint64_t)p[0] << 56) | ((uint64_t)p[1] << 48) |
         ((uint64_t)p[2] << 40) | ((uint64_t)p[3] << 32) |
         ((uint64_t)p[4] << 24) | ((uint64_t)p[5] << 16) |
         ((uint64_t)p[6] << 8) | ((uint64_t)p[7]);
}

const char *faith_status_code_name(faith_status_code_t code) {
  switch (code) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_STATUS_CODES(X)
#undef X
  default:
    return "FAITH_ERR_UNKNOWN";
  }
}

const char *faith_frame_msg_name(faith_frame_msg_type_t msg) {
  switch (msg) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_MSG_TYPES(X)
#undef X
  default:
    return "FAITH_MSG_UNKNOWN";
  }
}

const char *faith_envelope_name(faith_envelope_type_t env) {
  switch (env) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_ENVELOPE_TYPES(X)
#undef X
  default:
    return "FAITH_ENVELOPE_UNKNOWN";
  }
}

const char *
faith_client_reconnect_policy_name(faith_client_reconnect_policy_t policy) {
  switch (policy) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_CLIENT_DISCONNECT_POLICIES(X)
#undef X
  default:
    return "FAITH_CLIENT_RECONNECT_POLICY_UNKNOWN";
  }
}

const char *
faith_client_disconnect_reason_name(faith_client_disconnect_reason_t reason) {
  switch (reason) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_CLIENT_DISCONNECT_REASONS(X)
#undef X
  default:
    return "FAITH_CLIENT_DISCONNECT_REASON_UNKNOWN";
  }
}

const char *
faith_msg_request_fail_reason_name(faith_msg_request_fail_reason_t reason) {
  switch (reason) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_MSG_REQUEST_FAIL_REASONS(X)
#undef X
  default:
    return "FAITH_MSG_REQUEST_FAIL_REASON_UNKNOWN";
  }
}
const char *faith_msg_request_response_fail_reason_name(
    faith_msg_request_response_fail_reason_t reason) {
  switch (reason) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_MSG_REQUEST_RESPONSE_FAIL_REASONS(X)
#undef X
  default:
    return "FAITH_MSG_REQUEST_RESPONSE_FAIL_REASON_UNKNOWN";
  }
}
const char *
faith_msg_request_response_type_name(faith_msg_request_response_type_t type) {
  switch (type) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_MSG_REQUEST_RESPONSE_TYPES(X)
#undef X
  default:
    return "FAITH_MSG_REQUEST_RESPONSE_TYPE_UNKNOWN";
  }
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

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED,
                             <);

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

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED,
                             <);

  return FAITH_OK;
}

faith_status_code_t
faith_encode_msg_request_body(uint8_t *out_buf, faith_body_size_t *out_size,
                              size_t buf_cap_in_bytes,
                              const faith_envl_cts_msg_request_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_CTS_MSG_REQUEST_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id_recv.bytes,
                      sizeof(in->auth_id_recv.bytes));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->cl_req_id);

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_CTS_MSG_REQUEST_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_msg_request_body(const uint8_t                *payload,
                              faith_body_size_t             payload_size,
                              faith_envl_cts_msg_request_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_CTS_MSG_REQUEST_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->auth_id_recv.bytes,
                      sizeof(out->auth_id_recv.bytes));

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->cl_req_id);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_CTS_MSG_REQUEST_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_msg_request_response_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_cts_msg_request_response_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_CTS_MSG_REQUEST_RESPONSE_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->signature_response,
                      sizeof(in->signature_response));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->cl_req_id);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->srv_req_id.bytes,
                      sizeof(in->srv_req_id.bytes));

  if (in->type != FAITH_MSG_REQUEST_RESPONSE_ACCEPT &&
      in->type != FAITH_MSG_REQUEST_RESPONSE_DENY)
    return FAITH_ERR_BAD_FRAME;

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_CTS_MSG_REQUEST_RESPONSE_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_msg_request_response_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_cts_msg_request_response_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_CTS_MSG_REQUEST_RESPONSE_BODY_SIZE, !=);

  size_t offset = 0;
  FAITH_DECODE_RETURN(payload, payload_size, offset, out->signature_response,
                      sizeof(out->signature_response));

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->cl_req_id);

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->srv_req_id.bytes,
                      sizeof(out->srv_req_id.bytes));

  if (out->type != FAITH_MSG_REQUEST_RESPONSE_ACCEPT &&
      out->type != FAITH_MSG_REQUEST_RESPONSE_DENY)
    return FAITH_ERR_BAD_FRAME;

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->type);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_CTS_MSG_REQUEST_RESPONSE_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_msg_request_failed_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_failed_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_FAILED_BODY_SIZE);

  size_t offset = 0;

  switch (in->reason) {
#define X(name, value) case name:
    FAITH_MSG_REQUEST_FAIL_REASONS(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->reason);

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->cl_req_id);

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_FAILED_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_msg_request_failed_body(const uint8_t    *payload,
                                     faith_body_size_t payload_size,
                                     faith_envl_stc_msg_request_failed_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_FAILED_BODY_SIZE, !=);

  size_t offset = 0;

  switch (out->reason) {
#define X(name, value) case name:
    FAITH_MSG_REQUEST_FAIL_REASONS(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->reason);

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->cl_req_id);

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_FAILED_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_encode_msg_request_ack_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_msg_request_ack_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_ACK_BODY_SIZE);

  size_t offset = 0;

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->cl_req_id);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->srv_req_id.bytes,
                      sizeof(in->srv_req_id.bytes));

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_ACK_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_msg_request_ack_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_msg_request_ack_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_ACK_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->cl_req_id);

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->srv_req_id.bytes,
                      sizeof(out->srv_req_id.bytes));

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_ACK_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_msg_request_received_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_received_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_RECEIVED_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->auth_id_sender.bytes,
                      sizeof(in->auth_id_sender.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->srv_req_id.bytes,
                      sizeof(in->srv_req_id.bytes));

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_RECEIVED_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_msg_request_received_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_received_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_RECEIVED_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->auth_id_sender.bytes,
                      sizeof(out->auth_id_sender.bytes));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->srv_req_id.bytes,
                      sizeof(out->srv_req_id.bytes));

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_RECEIVED_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_msg_request_response_ack_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_response_ack_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_ACK_BODY_SIZE);

  size_t offset = 0;

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->cl_req_id);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->srv_req_id.bytes,
                      sizeof(in->srv_req_id.bytes));

  FAITH_ENCODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_ACK_BODY_SIZE,
                             !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_msg_request_response_ack_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_response_ack_t *out) {

  FAITH_DECODE_PROLOGUE(FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_ACK_BODY_SIZE,
                             !=);

  size_t offset = 0;

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->cl_req_id);

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->srv_req_id.bytes,
                      sizeof(out->srv_req_id.bytes));

  FAITH_DECODE_EPILOGUE(FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_ACK_BODY_SIZE,
                             !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_msg_request_response_failed_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_response_failed_t *in) {

  FAITH_ENCODE_PROLOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_FAILED_BODY_SIZE);

  size_t offset = 0;

  switch (in->reason) {
#define X(name, value) case name:
    FAITH_MSG_REQUEST_RESPONSE_FAIL_REASONS(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->reason);

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset, in->cl_req_id);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->srv_req_id.bytes,
                      sizeof(in->srv_req_id.bytes));

  FAITH_ENCODE_EPILOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_FAILED_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_msg_request_response_failed_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_response_failed_t *out) {

  FAITH_DECODE_PROLOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_FAILED_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->reason);

  switch (out->reason) {
#define X(name, value) case name:
    FAITH_MSG_REQUEST_RESPONSE_FAIL_REASONS(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out->cl_req_id);

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->srv_req_id.bytes,
                      sizeof(out->srv_req_id.bytes));

  FAITH_DECODE_EPILOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_FAILED_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_encode_msg_request_responded_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_responded_t *in) {

  FAITH_ENCODE_PROLOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONDED_BODY_SIZE);

  switch (in->type) {
#define X(name, value)                                                        \
  case name:
    FAITH_MSG_REQUEST_RESPONSE_TYPES(X)
#undef X
    break;

  default:
    return FAITH_ERR_INVALID;
  }

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->srv_req_id.bytes,
                      sizeof(in->srv_req_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->auth_id_responder.bytes,
                      sizeof(in->auth_id_responder.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);

  FAITH_ENCODE_EPILOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONDED_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_msg_request_responded_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_responded_t *out) {

  FAITH_DECODE_PROLOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONDED_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset,
                      out->srv_req_id.bytes,
                      sizeof(out->srv_req_id.bytes));

  FAITH_DECODE_RETURN(payload, payload_size, offset,
                      out->auth_id_responder.bytes,
                      sizeof(out->auth_id_responder.bytes));

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->type);

  switch (out->type) {
#define X(name, value)                                                        \
  case name:
    FAITH_MSG_REQUEST_RESPONSE_TYPES(X)
#undef X
    break;

  default:
    return FAITH_ERR_BAD_FRAME;
  }

  FAITH_DECODE_EPILOGUE(
      FAITH_ENVL_STC_MSG_REQUEST_RESPONDED_BODY_SIZE, !=);

  return FAITH_OK;
}


void faith_log_handler(Nob_Log_Level level, const char *fmt, va_list args) {
  if (level < nob_minimal_log_level)
    return;

  const char *level_name = NULL;

  switch (level) {
  case NOB_INFO:
    level_name = "INFO";
    break;
  case NOB_WARNING:
    level_name = "WARNING";
    break;
  case NOB_ERROR:
    level_name = "ERROR";
    break;
  case NOB_NO_LOGS:
    return;
  default:
    NOB_UNREACHABLE("Nob_Log_Level");
  }

  struct timeval tv;
  gettimeofday(&tv, NULL);

  time_t    now = tv.tv_sec;
  struct tm tm_utc;

#if defined(_WIN32)
  gmtime_s(&tm_utc, &now);
#else
  gmtime_r(&now, &tm_utc);
#endif

  char timestamp[32];

  strftime(timestamp, sizeof(timestamp), "%Y-%m-%dT%H:%M:%S", &tm_utc);

  fprintf(stderr, "%s.%03ldZ [%s]%*s ", timestamp, tv.tv_usec / 1000,
          level_name, (int)(7 - strlen(level_name)), "");

  vfprintf(stderr, fmt, args);
  fprintf(stderr, "\n");
}

int faith_client_id_equal(faith_client_id_t a, faith_client_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
}

int faith_device_id_equal(faith_device_id_t a, faith_device_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
}

faith_status_code_t faith_id128_to_hex(const uint8_t bytes[16], char out[33]) {
  if (!out || !bytes)
    return FAITH_ERR_INVALID;

  for (size_t i = 0; i < 16; ++i) {
    int written = snprintf(out + (i * 2), 3, "%02x", bytes[i]);

    if (written != 2)
      return FAITH_ERR_INVALID;
  }

  return FAITH_OK;
}

faith_status_code_t faith_random_bytes(uint8_t *o_buf, int num) {
  if (RAND_bytes(o_buf, num) != 1) {
    nob_log(ERROR, "Failed to generate auth_id with OpenSSL RAND_bytes()");
    return FAITH_ERR_SSL;
  }
  return FAITH_OK;
}

faith_status_code_t
faith_gen_ed25519_keypair(void   *handle,
                          uint8_t private_key[FAITH_ED25519_PRIVATE_KEY_SIZE],
                          uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]) {
  if (!handle || !private_key || !public_key)
    return FAITH_ERR_INVALID;

  faith_status_code_t status = FAITH_OK;

  EVP_PKEY    **out_keypair = (EVP_PKEY **)handle;
  EVP_PKEY     *keypair = NULL;
  EVP_PKEY_CTX *ctx = NULL;

  ctx = EVP_PKEY_CTX_new_id(EVP_PKEY_ED25519, NULL);
  if (!ctx) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  if (EVP_PKEY_keygen_init(ctx) != 1) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  if (EVP_PKEY_keygen(ctx, &keypair) != 1) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  {
    size_t priv_len = 0;

    if (EVP_PKEY_get_raw_private_key(keypair, NULL, &priv_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }

    if (priv_len != FAITH_ED25519_PRIVATE_KEY_SIZE) {
      nob_log(ERROR, "Generated private key has unexpected byte size: %zu",
              priv_len);
      status = FAITH_ERR_INVALID;
      goto cleanup;
    }

    if (EVP_PKEY_get_raw_private_key(keypair, private_key, &priv_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }
  }

  {
    size_t pub_len = 0;

    if (EVP_PKEY_get_raw_public_key(keypair, NULL, &pub_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }

    if (pub_len != FAITH_ED25519_PUBLIC_KEY_SIZE) {
      nob_log(ERROR, "Generated public key has unexpected byte size: %zu",
              pub_len);
      status = FAITH_ERR_INVALID;
      goto cleanup;
    }

    if (EVP_PKEY_get_raw_public_key(keypair, public_key, &pub_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }
  }

  *out_keypair = keypair;
  keypair = NULL;

cleanup:
  EVP_PKEY_free(keypair);
  EVP_PKEY_CTX_free(ctx);
  return status;
}

faith_status_code_t faith_gen_sign_buf_hello_handshake(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_hello_handshake_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id.bytes,
                      sizeof(in->auth_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->device_id.bytes,
                      sizeof(in->device_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->public_key,
                      sizeof(in->public_key));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->client_nonce);

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->server_nonce);

  FAITH_ENCODE_EPILOGUE(FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_gen_sign_buf_device_link_response(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_device_link_response_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id.bytes,
                      sizeof(in->auth_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_new.bytes, sizeof(in->device_id_new.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->public_key_new_device,
                      sizeof(in->public_key_new_device));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->code,
                      sizeof(in->code));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->expires_at_ms);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_responding.bytes,
                      sizeof(in->device_id_responding.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);

  return offset == FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE
             ? FAITH_OK
             : FAITH_ERR_INVALID;
}

faith_status_code_t faith_gen_sign_buf_msg_request_response(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_msg_request_response_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_SIGNATURE_MSG_REQUEST_RESPONSE_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id_req.bytes,
                      sizeof(in->auth_id_req.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id_recv.bytes,
                      sizeof(in->auth_id_recv.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_recv.bytes,
                      sizeof(in->device_id_recv.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->srv_req_id.bytes,
                      sizeof(in->srv_req_id.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);

  FAITH_ENCODE_EPILOGUE(FAITH_SIGNATURE_MSG_REQUEST_RESPONSE_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_gen_signature(EVP_PKEY *keypair, uint8_t *o_signature,
                                        size_t        *o_signature_size,
                                        const uint8_t *msg_input,
                                        size_t         msg_size) {
  if (!keypair || !o_signature || !o_signature_size ||
      (!msg_input && msg_size != 0)) {
    return FAITH_ERR_INVALID;
  }

  EVP_MD_CTX *ctx = EVP_MD_CTX_new();
  if (!ctx)
    return FAITH_ERR_NOMEM;

  faith_status_code_t result = FAITH_ERR_CRYPTO;

  if (EVP_DigestSignInit(ctx, NULL, NULL, NULL, keypair) != 1)
    goto defer;

  size_t required_size = 0;

  if (EVP_DigestSign(ctx, NULL, &required_size, msg_input, msg_size) != 1) {
    goto defer;
  }

  if (required_size != FAITH_ED25519_SIGNATURE_SIZE)
    goto defer;

  size_t produced_size = required_size;

  if (EVP_DigestSign(ctx, o_signature, &produced_size, msg_input, msg_size) !=
      1) {
    goto defer;
  }

  if (produced_size != FAITH_ED25519_SIGNATURE_SIZE)
    goto defer;

  *o_signature_size = produced_size;
  result = FAITH_OK;

defer:
  EVP_MD_CTX_free(ctx);
  return result;
}

faith_status_code_t faith_verify_signature(EVP_PKEY      *public_key,
                                           const uint8_t *msg_input,
                                           size_t         msg_size,
                                           const uint8_t *signature,
                                           size_t         signature_size) {
  if (!public_key || (!msg_input && msg_size != 0) || !signature ||
      signature_size != FAITH_ED25519_SIGNATURE_SIZE) {
    return FAITH_ERR_INVALID;
  }

  EVP_MD_CTX *ctx = EVP_MD_CTX_new();
  if (!ctx)
    return FAITH_ERR_NOMEM;

  faith_status_code_t _fh_result = FAITH_ERR_CRYPTO;

  if (EVP_DigestVerifyInit(ctx, NULL, NULL, NULL, public_key) != 1)
    _FH_RETURN_DEFER(_fh_result);

  int rc =
      EVP_DigestVerify(ctx, signature, signature_size, msg_input, msg_size);

  if (rc == 1)
    _fh_result = FAITH_OK;
  else if (rc == 0)
    _fh_result = FAITH_ERR_NOT_EQUAL;
  else
    _fh_result = FAITH_ERR_CRYPTO;

defer:
  EVP_MD_CTX_free(ctx);
  return _fh_result;
}

faith_status_code_t faith_verify_signature_raw_pubkey(
    const uint8_t  public_key[FAITH_ED25519_PUBLIC_KEY_SIZE],
    const uint8_t *msg_input, size_t msg_size, const uint8_t *signature,
    size_t signature_size) {
  if (!public_key || (!msg_input && msg_size != 0) || !signature ||
      signature_size != FAITH_ED25519_SIGNATURE_SIZE) {
    return FAITH_ERR_INVALID;
  }

  EVP_PKEY *pkey = EVP_PKEY_new_raw_public_key(
      EVP_PKEY_ED25519, NULL, public_key, FAITH_ED25519_PUBLIC_KEY_SIZE);

  if (!pkey)
    return FAITH_ERR_CRYPTO;

  faith_status_code_t rc = faith_verify_signature(pkey, msg_input, msg_size,
                                                  signature, signature_size);

  EVP_PKEY_free(pkey);
  return rc;
}
