#include "shared.h"

#include <openssl/rand.h>
#include <openssl/ssl.h>
#include <stdint.h>
#include <string.h>

#define _FH_LOG_CODEC_FAILURE(operation, reason, details_fmt, ...)             \
  nob_log(ERROR,                                                               \
          "%s failed:\n"                                                       \
          "  reason   : %s\n"                                                  \
          "  function : %s\n"                                                  \
          "  location : %s:%d\n" details_fmt,                                  \
          (operation), (reason), __func__, __FILE__, __LINE__, __VA_ARGS__)

#define FAITH_APPEND_RETURN(buf, cap, off, ptr, len)                           \
  do {                                                                         \
    const size_t _fh_cap = (size_t)(cap);                                      \
    const size_t _fh_off = (size_t)(off);                                      \
    const size_t _fh_len = (size_t)(len);                                      \
                                                                               \
    if (!(buf)) {                                                              \
      _FH_LOG_CODEC_FAILURE("Buffer append", "destination buffer is NULL",     \
                            "  buffer   : %s\n"                                \
                            "  source   : %s\n"                                \
                            "  offset   : %zu\n"                               \
                            "  length   : %zu\n"                               \
                            "  capacity : %zu",                                \
                            #buf, #ptr, _fh_off, _fh_len, _fh_cap);            \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    if (_fh_len > 0 && !(ptr)) {                                               \
      _FH_LOG_CODEC_FAILURE("Buffer append",                                   \
                            "source is NULL for a non-empty copy",             \
                            "  buffer   : %s\n"                                \
                            "  source   : %s\n"                                \
                            "  offset   : %zu\n"                               \
                            "  length   : %zu\n"                               \
                            "  capacity : %zu",                                \
                            #buf, #ptr, _fh_off, _fh_len, _fh_cap);            \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    if (_fh_off > _fh_cap) {                                                   \
      _FH_LOG_CODEC_FAILURE("Buffer append",                                   \
                            "offset exceeds destination capacity",             \
                            "  buffer   : %s\n"                                \
                            "  source   : %s\n"                                \
                            "  offset   : %zu\n"                               \
                            "  length   : %zu\n"                               \
                            "  capacity : %zu",                                \
                            #buf, #ptr, _fh_off, _fh_len, _fh_cap);            \
      return FAITH_ERR_OVERFLOW;                                               \
    }                                                                          \
                                                                               \
    if (_fh_len > _fh_cap - _fh_off) {                                         \
      _FH_LOG_CODEC_FAILURE(                                                   \
          "Buffer append", "insufficient destination capacity",                \
          "  buffer    : %s\n"                                                 \
          "  source    : %s\n"                                                 \
          "  offset    : %zu\n"                                                \
          "  length    : %zu\n"                                                \
          "  remaining : %zu\n"                                                \
          "  capacity  : %zu",                                                 \
          #buf, #ptr, _fh_off, _fh_len, _fh_cap - _fh_off, _fh_cap);           \
      return FAITH_ERR_OVERFLOW;                                               \
    }                                                                          \
                                                                               \
    if (_fh_len > 0)                                                           \
      memcpy((buf) + _fh_off, (ptr), _fh_len);                                 \
                                                                               \
    (off) += _fh_len;                                                          \
  } while (0)

#define FAITH_DECODE_RETURN(payload, payload_size, offset, dst, size)          \
  do {                                                                         \
    const size_t _fh_payload_size = (size_t)(payload_size);                    \
    const size_t _fh_offset = (size_t)(offset);                                \
    const size_t _fh_size = (size_t)(size);                                    \
                                                                               \
    if (!(payload)) {                                                          \
      _FH_LOG_CODEC_FAILURE("Buffer decode", "payload is NULL",                \
                            "  payload      : %s\n"                            \
                            "  destination  : %s\n"                            \
                            "  offset       : %zu\n"                           \
                            "  length       : %zu\n"                           \
                            "  payload size : %zu",                            \
                            #payload, #dst, _fh_offset, _fh_size,              \
                            _fh_payload_size);                                 \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    if (_fh_size > 0 && !(dst)) {                                              \
      _FH_LOG_CODEC_FAILURE(                                                   \
          "Buffer decode", "destination is NULL for a non-empty copy",         \
          "  payload      : %s\n"                                              \
          "  destination  : %s\n"                                              \
          "  offset       : %zu\n"                                             \
          "  length       : %zu\n"                                             \
          "  payload size : %zu",                                              \
          #payload, #dst, _fh_offset, _fh_size, _fh_payload_size);             \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    if (_fh_offset > _fh_payload_size) {                                       \
      _FH_LOG_CODEC_FAILURE("Buffer decode", "offset exceeds payload size",    \
                            "  payload      : %s\n"                            \
                            "  destination  : %s\n"                            \
                            "  offset       : %zu\n"                           \
                            "  length       : %zu\n"                           \
                            "  payload size : %zu",                            \
                            #payload, #dst, _fh_offset, _fh_size,              \
                            _fh_payload_size);                                 \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
                                                                               \
    if (_fh_size > _fh_payload_size - _fh_offset) {                            \
      _FH_LOG_CODEC_FAILURE("Buffer decode", "payload is truncated",           \
                            "  payload      : %s\n"                            \
                            "  destination  : %s\n"                            \
                            "  offset       : %zu\n"                           \
                            "  length       : %zu\n"                           \
                            "  remaining    : %zu\n"                           \
                            "  payload size : %zu",                            \
                            #payload, #dst, _fh_offset, _fh_size,              \
                            _fh_payload_size - _fh_offset, _fh_payload_size);  \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
                                                                               \
    if (_fh_size > 0)                                                          \
      memcpy((dst), (payload) + _fh_offset, _fh_size);                         \
                                                                               \
    (offset) += _fh_size;                                                      \
  } while (0)

#define _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, type,  \
                                    read_fn)                                   \
  do {                                                                         \
    const size_t _fh_payload_size = (size_t)(payload_size);                    \
    const size_t _fh_offset = (size_t)(offset);                                \
    const size_t _fh_int_size = sizeof(type);                                  \
                                                                               \
    if (!(payload)) {                                                          \
      _FH_LOG_CODEC_FAILURE("Integer decode", "payload is NULL",               \
                            "  reader       : %s\n"                            \
                            "  output       : %s\n"                            \
                            "  integer size : %zu\n"                           \
                            "  offset       : %zu\n"                           \
                            "  payload size : %zu",                            \
                            #read_fn, #out, _fh_int_size, _fh_offset,          \
                            _fh_payload_size);                                 \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    if (_fh_offset > _fh_payload_size) {                                       \
      _FH_LOG_CODEC_FAILURE("Integer decode", "offset exceeds payload size",   \
                            "  reader       : %s\n"                            \
                            "  output       : %s\n"                            \
                            "  integer size : %zu\n"                           \
                            "  offset       : %zu\n"                           \
                            "  payload size : %zu",                            \
                            #read_fn, #out, _fh_int_size, _fh_offset,          \
                            _fh_payload_size);                                 \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
                                                                               \
    if (_fh_int_size > _fh_payload_size - _fh_offset) {                        \
      _FH_LOG_CODEC_FAILURE("Integer decode", "payload is truncated",          \
                            "  reader       : %s\n"                            \
                            "  output       : %s\n"                            \
                            "  integer size : %zu\n"                           \
                            "  offset       : %zu\n"                           \
                            "  remaining    : %zu\n"                           \
                            "  payload size : %zu",                            \
                            #read_fn, #out, _fh_int_size, _fh_offset,          \
                            _fh_payload_size - _fh_offset, _fh_payload_size);  \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
                                                                               \
    (out) = read_fn((payload) + _fh_offset);                                   \
    (offset) += _fh_int_size;                                                  \
  } while (0)

#define FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out)         \
  _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, uint32_t,    \
                              faith_read_u32_be)

#define FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out)         \
  _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, uint64_t,    \
                              faith_read_u64_be)

#define _FAITH_ENCODE_INT_BE_RETURN(buf, buf_cap, offset, value, type,         \
                                    write_fn)                                  \
  do {                                                                         \
    const size_t _fh_cap = (size_t)(buf_cap);                                  \
    const size_t _fh_offset = (size_t)(offset);                                \
    const size_t _fh_int_size = sizeof(type);                                  \
                                                                               \
    if (!(buf)) {                                                              \
      _FH_LOG_CODEC_FAILURE("Integer encode", "destination buffer is NULL",    \
                            "  writer       : %s\n"                            \
                            "  value        : %s\n"                            \
                            "  integer size : %zu\n"                           \
                            "  offset       : %zu\n"                           \
                            "  capacity     : %zu",                            \
                            #write_fn, #value, _fh_int_size, _fh_offset,       \
                            _fh_cap);                                          \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    if (_fh_offset > _fh_cap) {                                                \
      _FH_LOG_CODEC_FAILURE(                                                   \
          "Integer encode", "offset exceeds destination capacity",             \
          "  writer       : %s\n"                                              \
          "  value        : %s\n"                                              \
          "  integer size : %zu\n"                                             \
          "  offset       : %zu\n"                                             \
          "  capacity     : %zu",                                              \
          #write_fn, #value, _fh_int_size, _fh_offset, _fh_cap);               \
      return FAITH_ERR_OVERFLOW;                                               \
    }                                                                          \
                                                                               \
    if (_fh_int_size > _fh_cap - _fh_offset) {                                 \
      _FH_LOG_CODEC_FAILURE("Integer encode",                                  \
                            "insufficient destination capacity",               \
                            "  writer       : %s\n"                            \
                            "  value        : %s\n"                            \
                            "  integer size : %zu\n"                           \
                            "  offset       : %zu\n"                           \
                            "  remaining    : %zu\n"                           \
                            "  capacity     : %zu",                            \
                            #write_fn, #value, _fh_int_size, _fh_offset,       \
                            _fh_cap - _fh_offset, _fh_cap);                    \
      return FAITH_ERR_OVERFLOW;                                               \
    }                                                                          \
                                                                               \
    _FH_CHECK_RETURN(write_fn((buf) + _fh_offset, (type)(value)));             \
    (offset) += _fh_int_size;                                                  \
  } while (0)

#define FAITH_ENCODE_U32_BE_RETURN(buf, buf_cap, offset, value)                \
  _FAITH_ENCODE_INT_BE_RETURN(buf, buf_cap, offset, value, uint32_t,           \
                              faith_write_u32_be)

#define FAITH_ENCODE_U64_BE_RETURN(buf, buf_cap, offset, value)                \
  _FAITH_ENCODE_INT_BE_RETURN(buf, buf_cap, offset, value, uint64_t,           \
                              faith_write_u64_be)

#define FAITH_ENVL_DECODE_PROLOGUE(envl_size)                                  \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
                                                                               \
    if (!out) {                                                                \
      _FH_LOG_CODEC_FAILURE("Envelope decode", "output pointer is NULL",       \
                            "  expected size : %zu", _fh_expected_size);       \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    memset(out, 0, sizeof(*out));                                              \
                                                                               \
    if (!payload) {                                                            \
      const faith_status_code_t _fh_rc =                                       \
          payload_size == 0 ? FAITH_ERR_BAD_FRAME : FAITH_ERR_INVALID;         \
                                                                               \
      _FH_LOG_CODEC_FAILURE("Envelope decode", "payload pointer is NULL",      \
                            "  payload       : payload\n"                      \
                            "  payload size  : %zu\n"                          \
                            "  expected size : %zu\n"                          \
                            "  status        : %s (%d)",                       \
                            (size_t)payload_size, _fh_expected_size,           \
                            faith_status_code_name(_fh_rc), (int)_fh_rc);      \
      return _fh_rc;                                                           \
    }                                                                          \
                                                                               \
    if ((size_t)payload_size != _fh_expected_size) {                           \
      _FH_LOG_CODEC_FAILURE("Envelope decode",                                 \
                            "payload size does not match wire size",           \
                            "  payload size  : %zu\n"                          \
                            "  expected size : %zu",                           \
                            (size_t)payload_size, _fh_expected_size);          \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
  } while (0)

#define FAITH_ENVL_DECODE_EPILOGUE(envl_size)                                  \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
    const size_t _fh_offset = (size_t)offset;                                  \
    const size_t _fh_payload_size = (size_t)payload_size;                      \
                                                                               \
    if (_fh_offset != _fh_expected_size || _fh_offset != _fh_payload_size) {   \
      _FH_LOG_CODEC_FAILURE("Envelope decode",                                 \
                            "final decode offset is inconsistent",             \
                            "  offset        : %zu\n"                          \
                            "  payload size  : %zu\n"                          \
                            "  expected size : %zu",                           \
                            _fh_offset, _fh_payload_size, _fh_expected_size);  \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
  } while (0)

#define FAITH_ENVL_ENCODE_PROLOGUE(envl_size)                                  \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
                                                                               \
    if (!out_buf || !out_size || !in) {                                        \
      _FH_LOG_CODEC_FAILURE("Envelope encode", "required argument is NULL",    \
                            "  out_buf       : %p\n"                           \
                            "  out_size      : %p\n"                           \
                            "  input         : %p\n"                           \
                            "  expected size : %zu",                           \
                            (void *)out_buf, (void *)out_size,                 \
                            (const void *)in, _fh_expected_size);              \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    *out_size = 0;                                                             \
                                                                               \
    if ((size_t)buf_cap_in_bytes < _fh_expected_size) {                        \
      _FH_LOG_CODEC_FAILURE("Envelope encode",                                 \
                            "insufficient destination capacity",               \
                            "  capacity      : %zu\n"                          \
                            "  required size : %zu",                           \
                            (size_t)buf_cap_in_bytes, _fh_expected_size);      \
      return FAITH_ERR_OVERFLOW;                                               \
    }                                                                          \
  } while (0)

#define FAITH_ENVL_ENCODE_EPILOGUE(envl_size)                                  \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
    const size_t _fh_offset = (size_t)offset;                                  \
                                                                               \
    if (_fh_offset != _fh_expected_size) {                                     \
      _FH_LOG_CODEC_FAILURE("Envelope encode",                                 \
                            "final encoded size is inconsistent",              \
                            "  encoded size  : %zu\n"                          \
                            "  expected size : %zu",                           \
                            _fh_offset, _fh_expected_size);                    \
      return FAITH_ERR_INVALID;                                                \
    }                                                                          \
                                                                               \
    *out_size = (faith_body_size_t)_fh_offset;                                 \
  } while (0)

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

    nob_log(ERROR,
            "SSL_read_ex failed: ssl_err=%d, nread=%zu, total=%zu, want=%zu, "
            "errno=%d (%s)\n",
            err, nread, total, size - total, errno, strerror(errno));

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

      ERR_print_errors_fp(stderr);
      return FAITH_ERR_SSL;
    }

    default:
      return FAITH_ERR_IO;
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

  _FH_CHECK_RETURN(faith_read_bytes_sync(ssl, len_buf, sizeof(len_buf)));

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

  if (payload_size > FAITH_MAX_PAYLOAD_SIZE)
    return FAITH_ERR_FRAME_TOO_LARGE;

  if (payload_size > SIZE_MAX - FAITH_FRAME_METADATA_SIZE)
    return FAITH_ERR_FRAME_TOO_LARGE;

  const size_t frame_size = FAITH_FRAME_METADATA_SIZE + payload_size;

  if (frame_size > UINT32_MAX || frame_size > FAITH_MAX_FRAME_LEN)
    return FAITH_ERR_FRAME_TOO_LARGE;

  if (frame_size > SIZE_MAX - FAITH_FRAME_LENGTH_SIZE)
    return FAITH_ERR_FRAME_TOO_LARGE;

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

const char *faith_event_name(faith_event_type_t ev) {
  switch (ev) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_EVENT_TYPES(X)
#undef X
  default:
    return "FAITH_EVENT_UNKNOWN";
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

faith_status_code_t faith_encode_envelope(uint8_t *out_buf, size_t *out_size,
                                          size_t buf_cap_in_bytes,
                                          const faith_envelope_t *env) {
  if (!out_buf || !out_size || !env)
    return FAITH_ERR_INVALID;

  *out_size = 0;

  if (env->body_size > 0 && !env->body)
    return FAITH_ERR_INVALID;

  const size_t env_size = FAITH_ENVL_HEADER_SIZE + (size_t)env->body_size;

  if (buf_cap_in_bytes < env_size)
    return FAITH_ERR_OVERFLOW;

  size_t offset = 0;

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset, env->type);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, env->sender_id.bytes,
                      sizeof(env->sender_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      env->recipient_id.bytes, sizeof(env->recipient_id.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset, env->body_size);

  if (env->body_size > 0) {
    FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, env->body,
                        env->body_size);
  }

  if (offset != env_size)
    return FAITH_ERR_INVALID;

  *out_size = offset;
  return FAITH_OK;
}

faith_status_code_t faith_decode_envelope(const uint8_t    *payload,
                                          size_t            payload_size,
                                          faith_envelope_t *o_envl) {
  if (!o_envl)
    return FAITH_ERR_INVALID;

  memset(o_envl, 0, sizeof(*o_envl));

  if (!payload)
    return payload_size == 0 ? FAITH_ERR_BAD_FRAME : FAITH_ERR_INVALID;

  if (payload_size < FAITH_ENVL_HEADER_SIZE) {
    return FAITH_ERR_BAD_FRAME;
  }

  size_t offset = 0;

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, o_envl->type);

  FAITH_DECODE_RETURN(payload, payload_size, offset, o_envl->sender_id.bytes,
                      sizeof(o_envl->sender_id.bytes));

  FAITH_DECODE_RETURN(payload, payload_size, offset, o_envl->recipient_id.bytes,
                      sizeof(o_envl->recipient_id.bytes));

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, o_envl->body_size);

  if (o_envl->body_size != payload_size - offset) {
    return FAITH_ERR_BAD_FRAME;
  }

  uint8_t *body = NULL;
  if (o_envl->body_size != 0) {
    body = malloc(o_envl->body_size);
    if (!body)
      return FAITH_ERR_NOMEM;

    memcpy(body, payload + offset, o_envl->body_size);
    offset += o_envl->body_size;
  }

  if (offset != payload_size) {
    free(body);
    return FAITH_ERR_BAD_FRAME;
  }

  o_envl->body = body;

  return FAITH_OK;
}

faith_status_code_t
faith_encode_device_link_req_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_device_link_req_t *in) {

  FAITH_ENVL_ENCODE_PROLOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE);

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

  FAITH_ENVL_ENCODE_EPILOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_device_link_req_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_device_link_req_t *out) {
  FAITH_ENVL_DECODE_PROLOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE);

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

  FAITH_ENVL_DECODE_EPILOGUE(FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE);

  return FAITH_OK;
}

faith_status_code_t faith_encode_device_link_response_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_cts_device_link_response_t *in) {
  FAITH_ENVL_ENCODE_PROLOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->signature_response,
                      sizeof(in->signature_response));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_new.bytes, sizeof(in->device_id_new.bytes));

  FAITH_ENVL_ENCODE_EPILOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE);

  return FAITH_OK;
}

faith_status_code_t faith_decode_device_link_response_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_cts_device_link_response_t *out) {

  FAITH_ENVL_DECODE_PROLOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->signature_response,
                      sizeof(out->signature_response));

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->device_id_new.bytes,
                      sizeof(out->device_id_new.bytes));

  FAITH_ENVL_DECODE_EPILOGUE(FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE);

  return FAITH_OK;
}

faith_status_code_t
faith_encode_client_disconnect(uint8_t *out_buf, faith_body_size_t *out_size,
                               size_t buf_cap_in_bytes,
                               const faith_envl_stc_client_disconnect_t *in) {

  FAITH_ENVL_ENCODE_PROLOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE);

  size_t offset = 0;

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->reconnect_policy);

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->reason);

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->retry_after_ms);

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->banned_until_ms);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->msg,
                      sizeof(in->msg));

  FAITH_ENVL_ENCODE_EPILOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_client_disconnect(const uint8_t                      *payload,
                               faith_body_size_t                   payload_size,
                               faith_envl_stc_client_disconnect_t *out) {

  FAITH_ENVL_DECODE_PROLOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE);

  size_t offset = 0;

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset,
                             out->reconnect_policy);

  FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out->reason);

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset,
                             out->retry_after_ms);

  FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset,
                             out->banned_until_ms);

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->msg,
                      sizeof(out->msg));

  FAITH_ENVL_DECODE_EPILOGUE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE);

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
    uint8_t *o_buf, size_t buf_cap_in_bytes,
    const faith_signature_hello_handshake_t *src) {
  if (!src || !o_buf)
    return FAITH_ERR_INVALID;

  if (buf_cap_in_bytes < FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE)
    return FAITH_ERR_INVALID;

  size_t offset = 0;

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset, src->auth_id.bytes,
                      sizeof(src->auth_id.bytes));

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset, src->device_id.bytes,
                      sizeof(src->device_id.bytes));

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset, src->public_key,
                      sizeof(src->public_key));

  _FH_CHECK_RETURN(faith_write_u64_be(o_buf + offset, src->client_nonce));
  offset += sizeof(src->client_nonce);

  _FH_CHECK_RETURN(faith_write_u64_be(o_buf + offset, src->server_nonce));
  offset += sizeof(src->server_nonce);

  return FAITH_OK;
}

faith_status_code_t faith_gen_sign_buf_device_link_response(
    uint8_t *o_buf, size_t buf_cap_in_bytes,
    const faith_signature_device_link_response_t *src) {
  if (!src || !o_buf)
    return FAITH_ERR_INVALID;

  if (buf_cap_in_bytes < FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE)
    return FAITH_ERR_OVERFLOW;

  size_t offset = 0;

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset, src->auth_id.bytes,
                      sizeof(src->auth_id.bytes));

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset, src->device_id_new.bytes,
                      sizeof(src->device_id_new.bytes));

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset,
                      src->public_key_new_device,
                      sizeof(src->public_key_new_device));

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset, src->code,
                      sizeof(src->code));

  _FH_CHECK_RETURN(faith_write_u64_be(o_buf + offset, src->expires_at_ms));
  offset += sizeof(uint64_t);

  FAITH_APPEND_RETURN(o_buf, buf_cap_in_bytes, offset,
                      src->device_id_responding.bytes,
                      sizeof(src->device_id_responding.bytes));

  _FH_CHECK_RETURN(faith_write_u32_be(o_buf + offset, (uint32_t)src->type));
  offset += sizeof(uint32_t);

  return offset == FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE
             ? FAITH_OK
             : FAITH_ERR_INVALID;
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
