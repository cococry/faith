#pragma once

#include <openssl/err.h>
#include <stdint.h>
#include <time.h>

#include "../third_party/nob.h"

#define FAITH_PROTO_VERSION    faith_version_pack(0, 0, 1)
#define FAITH_MAX_FRAME_LEN    256
#define FAITH_MAX_MSG_SIZE     65536
#define FAITH_MAX_PAYLOAD_SIZE 256

#define FAITH_CLIENT_ID_SIZE 16
#define FAITH_DEVICE_ID_SIZE 16

#define FAITH_ED25519_PUBLIC_KEY_SIZE  32
#define FAITH_ED25519_PRIVATE_KEY_SIZE 32
#define FAITH_ED25519_SIGNATURE_SIZE   64

#define FAITH_MAX_CLIENT_OUT_QUEUE (1024u * 1024u)
#define FAITH_MAX_CLIENT_IN_QUEUE  (1024u * 1024u)

#define FAITH_FRAME_LENGTH_SIZE sizeof(uint32_t)

#define FAITH_FRAME_METADATA_SIZE                                              \
  (sizeof(uint16_t) /* protocol version */ +                                   \
   sizeof(uint16_t) /* message type */)

#define FAITH_FRAME_HEADER_SIZE                                                \
  (FAITH_FRAME_LENGTH_SIZE + FAITH_FRAME_METADATA_SIZE)

#define FAITH_ENVL_HEADER_SIZE                                                 \
  (sizeof(uint32_t) /* envelope type */ +                                      \
   FAITH_CLIENT_ID_SIZE /* sender id     */ +                                  \
   FAITH_CLIENT_ID_SIZE /* recipient id  */ +                                  \
   sizeof(faith_body_size_t) /* body size */)

#define _FAITH_BODY_SIZE(X) ((faith_body_size_t)((X)))

#define FAITH_BODY_SIZE_T_MAX UINT32_MAX

#define FAITH_MSG_PONG_PAYLOAD_SIZE                                            \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* client-server nonce */ +                \
                   sizeof(uint64_t)) /* server sent at ms */

#define FAITH_ENVL_HELLO_BODY_SIZE                                             \
  _FAITH_BODY_SIZE(FAITH_DEVICE_ID_SIZE /* client device id */ +               \
                   FAITH_ED25519_PUBLIC_KEY_SIZE /* client public key */ +     \
                   sizeof(uint64_t) /* client nonce */)

#define FAITH_ENVL_HELLO_CHALLENGE_BODY_SIZE                                   \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* server nonce */)

#define FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE                          \
  _FAITH_BODY_SIZE(FAITH_ED25519_SIGNATURE_SIZE /* signature response */ +     \
                   FAITH_DEVICE_ID_SIZE /*device ID new*/)

#define FAITH_DEVICE_LINK_CODE_SIZE 16
#define FAITH_REQUEST_ID_SIZE       16

#define FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE                               \
  _FAITH_BODY_SIZE(FAITH_CLIENT_ID_SIZE /* auth ID */ +                        \
                   FAITH_ED25519_PUBLIC_KEY_SIZE /*public key new device */ +  \
                   FAITH_DEVICE_ID_SIZE /* device ID new */ +                  \
                   FAITH_DEVICE_LINK_CODE_SIZE /* code */ +                    \
                   sizeof(uint64_t) /* expires_at_ms*/)

#define FAITH_MAX_CLIENT_DISCONNECT_MSG 128

#define FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED                       \
  _FAITH_BODY_SIZE(sizeof(uint32_t) /* reason */ +                             \
                   sizeof(uint32_t) /* reconnect policy */ +                   \
                   sizeof(uint64_t) /* retry after milliseconds */ +           \
                   sizeof(uint64_t) /* ban expiration timestamp */ +           \
                   sizeof(uint16_t) /* message length */)

#define FAITH_ENVL_CTS_MSG_REQUEST_BODY_SIZE                                   \
  _FAITH_BODY_SIZE(FAITH_CLIENT_ID_SIZE /* auth ID receiving */ +              \
                   sizeof(uint64_t) /* client request ID */)

#define FAITH_ENVL_CTS_MSG_REQUEST_RESPONSE_BODY_SIZE                          \
  _FAITH_BODY_SIZE(FAITH_ED25519_SIGNATURE_SIZE /* signature response */ +     \
                   sizeof(uint64_t) /* client request ID */ +                  \
                   FAITH_REQUEST_ID_SIZE /* server request ID */ + \
                   sizeof(uint32_t) /* type */)

#define FAITH_ENVL_STC_MSG_REQUEST_FAILED_BODY_SIZE                            \
  _FAITH_BODY_SIZE(sizeof(uint32_t) /* failure reason */ +                     \
                   sizeof(uint64_t) /* client request ID */)

#define FAITH_ENVL_STC_MSG_REQUEST_RECEIVED_BODY_SIZE                          \
  _FAITH_BODY_SIZE(FAITH_CLIENT_ID_SIZE /* sender auth ID */ +                 \
                   FAITH_REQUEST_ID_SIZE /* server request ID */)

#define FAITH_ENVL_STC_MSG_REQUEST_ACK_BODY_SIZE                               \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* client request ID */ +                  \
                   FAITH_REQUEST_ID_SIZE /* server request ID */)

#define FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_MAX                         \
  _FAITH_BODY_SIZE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED +          \
                   FAITH_MAX_CLIENT_DISCONNECT_MSG)

#define FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE                              \
  (FAITH_CLIENT_ID_SIZE /* auth ID */ +                                        \
   FAITH_DEVICE_ID_SIZE /* new device ID */ +                                  \
   FAITH_ED25519_PUBLIC_KEY_SIZE /* new device public key */ +                 \
   FAITH_DEVICE_LINK_CODE_SIZE /* device-link verification code */ +           \
   sizeof(uint64_t) /* request expiration timestamp */ +                       \
   FAITH_DEVICE_ID_SIZE /* responding authorized device ID */ +                \
   sizeof(uint32_t) /* approve/deny response type */)

#define FAITH_SIGNATURE_MSG_REQUEST_RESPONSE_SIZE                              \
  (FAITH_CLIENT_ID_SIZE /* auth ID requesting */ +                             \
   FAITH_CLIENT_ID_SIZE /* auth ID receiving */ +                              \
   FAITH_DEVICE_ID_SIZE /* device id receiving */) +                           \
      FAITH_REQUEST_ID_SIZE /* server request ID */ +                          \
      sizeof(uint32_t) /* type */

#define FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE                                   \
  (FAITH_CLIENT_ID_SIZE /* auth ID */ +                                        \
   FAITH_DEVICE_ID_SIZE /* client device ID */ +                               \
   FAITH_ED25519_PUBLIC_KEY_SIZE /* client public key */ +                     \
   sizeof(uint64_t) /* client nonce */ + sizeof(uint64_t) /* server nonce */)

#define FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_ACK_BODY_SIZE                      \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* client request ID */ +                  \
                   FAITH_REQUEST_ID_SIZE /* server request ID */)

#define FAITH_ENVL_STC_MSG_REQUEST_RESPONSE_FAILED_BODY_SIZE                   \
  _FAITH_BODY_SIZE(sizeof(uint32_t) /* failure reason */ +                     \
                   sizeof(uint64_t) /* client request ID */ +                  \
                   FAITH_REQUEST_ID_SIZE /* server request ID */)

#define FAITH_ENVL_STC_MSG_REQUEST_RESPONDED_BODY_SIZE                         \
  _FAITH_BODY_SIZE(FAITH_REQUEST_ID_SIZE /* server request ID */ +             \
                   FAITH_CLIENT_ID_SIZE /* responder authentication ID */ +    \
                   sizeof(uint32_t) /* response type */)

#define _FH_CHECK_RETURN(expr)                                                 \
  do {                                                                         \
    faith_status_code_t _fh_rc = (expr);                                       \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(ERROR,                                                           \
              "_FH_CHECK_RETURN failed:\n"                                     \
              "  expression : %s\n"                                            \
              "  status     : %s (%d)\n"                                       \
              "  function   : %s\n"                                            \
              "  location   : %s:%d",                                          \
              #expr, faith_status_code_name(_fh_rc), (int)_fh_rc, __func__,    \
              __FILE__, __LINE__);                                             \
      return _fh_rc;                                                           \
    }                                                                          \
  } while (0)

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

#define FAITH_DECODE_U16_BE_RETURN(payload, payload_size, offset, out)         \
  _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, uint16_t,    \
                              faith_read_u16_be)

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

#define FAITH_ENCODE_U16_BE_RETURN(buf, buf_cap, offset, value)                \
  _FAITH_ENCODE_INT_BE_RETURN(buf, buf_cap, offset, value, uint16_t,           \
                              faith_write_u16_be)

#define FAITH_ENCODE_U32_BE_RETURN(buf, buf_cap, offset, value)                \
  _FAITH_ENCODE_INT_BE_RETURN(buf, buf_cap, offset, value, uint32_t,           \
                              faith_write_u32_be)

#define FAITH_ENCODE_U64_BE_RETURN(buf, buf_cap, offset, value)                \
  _FAITH_ENCODE_INT_BE_RETURN(buf, buf_cap, offset, value, uint64_t,           \
                              faith_write_u64_be)

#define FAITH_ENVL_DECODE_PROLOGUE(envl_size, sign)                            \
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
    if ((size_t)payload_size sign _fh_expected_size) {                         \
      _FH_LOG_CODEC_FAILURE("Envelope decode",                                 \
                            "payload size does not match wire size",           \
                            "  payload size  : %zu\n"                          \
                            "  expected size : %zu",                           \
                            (size_t)payload_size, _fh_expected_size);          \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
  } while (0)

#define FAITH_ENVL_DECODE_EPILOGUE(envl_size, sign)                            \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
    const size_t _fh_offset = (size_t)offset;                                  \
    const size_t _fh_payload_size = (size_t)payload_size;                      \
                                                                               \
    if (_fh_offset sign _fh_expected_size ||                                   \
        _fh_offset sign _fh_payload_size) {                                    \
      _FH_LOG_CODEC_FAILURE("Envelope decode",                                 \
                            "final decode offset is inconsistent",             \
                            "  offset        : %zu\n"                          \
                            "  payload size  : %zu\n"                          \
                            "  expected size : %zu",                           \
                            _fh_offset, _fh_payload_size, _fh_expected_size);  \
      return FAITH_ERR_BAD_FRAME;                                              \
    }                                                                          \
  } while (0)

#define FAITH_ENVL_DECODE_EPILOGUE_DNY(envl_size, sign)                        \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
    const size_t _fh_offset = (size_t)offset;                                  \
    const size_t _fh_payload_size = (size_t)payload_size;                      \
                                                                               \
    if (_fh_offset sign _fh_expected_size) {                                   \
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

#define FAITH_ENVL_ENCODE_EPILOGUE(envl_size, sign)                            \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
    const size_t _fh_offset = (size_t)offset;                                  \
                                                                               \
    if (_fh_offset sign _fh_expected_size) {                                   \
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

#define _FH_CHECK_GOTO(expr, result, label)                                    \
  do {                                                                         \
    faith_status_code_t _fh_rc = (expr);                                       \
    result = _fh_rc;                                                           \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(ERROR,                                                           \
              "_FH_CHECK_GOTO failed:\n"                                       \
              "  expression : %s\n"                                            \
              "  status     : %s (%d)\n"                                       \
              "  function   : %s\n"                                            \
              "  location   : %s:%d",                                          \
              #expr, faith_status_code_name(_fh_rc), (int)_fh_rc, __func__,    \
              __FILE__, __LINE__);                                             \
      goto label;                                                              \
    }                                                                          \
  } while (0)

#define _FH_CHECK_DEFER(expr) _FH_CHECK_GOTO((expr), _fh_result, defer)

#define _FH_RETURN_DEFER(rc)                                                   \
  do {                                                                         \
    _fh_result = (rc);                                                         \
    goto defer;                                                                \
  } while (0)

#define _FH_CHECK(expr)                                                        \
  faith_status_code_t _fh_rc = (expr);                                         \
  do {                                                                         \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(ERROR,                                                           \
              "_FH_CHECK failed:\n"                                            \
              "  expression : %s\n"                                            \
              "  status     : %s (%d)\n"                                       \
              "  function   : %s\n"                                            \
              "  location   : %s:%d",                                          \
              #expr, faith_status_code_name(_fh_rc), (int)_fh_rc, __func__,    \
              __FILE__, __LINE__);                                             \
    }                                                                          \
  } while (0)

typedef struct {
  uint32_t frame_size;
  uint16_t proto_ver;
  uint16_t msg_type;

  void  *payload;
  size_t payload_size;
} faith_frame_t;

#define FAITH_STATUS_CODES(X)                                                  \
  X(FAITH_OK, 0)                                                               \
  X(FAITH_ERR_INVALID, 1)                                                      \
  X(FAITH_ERR_ALREADY_STARTED, 2)                                              \
  X(FAITH_ERR_THREAD, 3)                                                       \
  X(FAITH_ERR_OVERFLOW, 4)                                                     \
  X(FAITH_ERR_UNDERFLOW, 5)                                                    \
  X(FAITH_ERR_IO, 6)                                                           \
  X(FAITH_ERR_FRAME_TOO_LARGE, 7)                                              \
  X(FAITH_ERR_BAD_FRAME, 8)                                                    \
  X(FAITH_ERR_CLOSED, 9)                                                       \
  X(FAITH_ERR_UNSUPPORTED_VER, 10)                                             \
  X(FAITH_ERR_NOMEM, 11)                                                       \
  X(FAITH_ERR_INCOMPLETE, 12)                                                  \
  X(FAITH_ERR_UNAUTHORIZED, 13)                                                \
  X(FAITH_ERR_NOT_FOUND, 14)                                                   \
  X(FAITH_ERR_SSL, 15)                                                         \
  X(FAITH_ERR_CRYPTO, 16)                                                      \
  X(FAITH_ERR_NOT_EQUAL, 17)                                                   \
  X(FAITH_ERR_NOT_CONNECTED, 18)                                               \
  X(FAITH_ERR_EXPIRED, 19)                                                     \
  X(FAITH_ERR_UNREACHABLE, 20)                                                 \
  X(FAITH_ERR_BAD_ENVELOPE, 21)                                                \
  X(FAITH_ERR_NOT_STARTED, 22)                                                 \
  X(FAITH_ERR_ALREADY_CONNECTED, 23)                                           \
  X(FAITH_ERR_ALREADY_EXISTS, 24)

typedef enum {
#define X(name, value) name = value,
  FAITH_STATUS_CODES(X)
#undef X
} faith_status_code_t;

#define FAITH_MSG_TYPES(X)                                                     \
  X(FAITH_MSG_PING, 0)                                                         \
  X(FAITH_MSG_PONG, 1)                                                         \
  X(FAITH_MSG_ENVL, 2)

typedef enum {
#define X(name, value) name = value,
  FAITH_MSG_TYPES(X)
#undef X
} faith_frame_msg_type_t;

#define FAITH_ENVELOPE_TYPES(X)                                                \
  X(FAITH_ENVELOPE_HELLO, 0)                                                   \
  X(FAITH_ENVELOPE_HELLO_OK, 1)                                                \
  X(FAITH_ENVELOPE_MSG_SEND, 2)                                                \
  X(FAITH_ENVELOPE_MSG_DELIVER, 3)                                             \
  X(FAITH_ENVELOPE_MSG_ACK, 4)                                                 \
  X(FAITH_ENVELOPE_MSG_ERR, 5)                                                 \
  X(FAITH_ENVELOPE_CHALLENGE, 6)                                               \
  X(FAITH_ENVELOPE_CHALLENGE_RESPONSE, 7)                                      \
  X(FAITH_ENVELOPE_DEVICE_LINK_REQUEST, 8)                                     \
  X(FAITH_ENVELOPE_DEVICE_AUTH_PENDING, 9)                                     \
  X(FAITH_ENVELOPE_DEVICE_AUTH_APPROVE, 10)                                    \
  X(FAITH_ENVELOPE_DEVICE_AUTH_DENY, 11)                                       \
  X(FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_ACK, 12)                               \
  X(FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_FAILED, 13)                            \
  X(FAITH_ENVELOPE_DEVICE_LINK_CANCELLED, 14)                                  \
  X(FAITH_ENVELOPE_CLIENT_DISCONNECT, 15)                                      \
  X(FAITH_ENVELOPE_MSG_REQUEST, 16)                                            \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONSE, 17)                                   \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONSE_ACK, 18)                               \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONSE_FAILED, 19)                            \
  X(FAITH_ENVELOPE_MSG_REQUEST_ACK, 20)                                        \
  X(FAITH_ENVELOPE_MSG_REQUEST_FAILED, 21)                                     \
  X(FAITH_ENVELOPE_MSG_REQUEST_RECEIVED, 22)                                   \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONDED, 23)

#define FAITH_CLIENT_DISCONNECT_REASONS(X)                                     \
  X(FAITH_DISCONNECT_REASON_NONE, 0)                                           \
  X(FAITH_DISCONNECT_SERVER_SHUTDOWN, 1)                                       \
  X(FAITH_DISCONNECT_SERVER_BUSY, 2)                                           \
  X(FAITH_DISCONNECT_RATE_LIMITED, 3)                                          \
  X(FAITH_DISCONNECT_TEMPORARY_FAILURE, 4)                                     \
  X(FAITH_DISCONNECT_BAD_PROTOCOL, 5)                                          \
  X(FAITH_DISCONNECT_UNSUPPORTED_VERSION, 6)                                   \
  X(FAITH_DISCONNECT_AUTH_FAILED, 7)                                           \
  X(FAITH_DISCONNECT_DEVICE_REJECTED, 8)                                       \
  X(FAITH_DISCONNECT_DUPLICATE_SESSION, 9)                                     \
  X(FAITH_DISCONNECT_IDENTITY_BANNED, 10)                                      \
  X(FAITH_DISCONNECT_DEVICE_BANNED, 11)                                        \
  X(FAITH_DISCONNECT_IP_BANNED, 12)                                            \
  X(FAITH_DISCONNECT_ABUSE, 13)                                                \
  X(FAITH_DISCONNECT_MEMORY_LIMIT, 14)                                         \
  X(FAITH_DISCONNECT_INTERNAL_ERROR, 15)

typedef enum {
#define X(name, value) name = value,
  FAITH_CLIENT_DISCONNECT_REASONS(X)
#undef X
} faith_client_disconnect_reason_t;

#define FAITH_CLIENT_DISCONNECT_POLICIES(X)                                    \
  X(FAITH_CLIENT_RECONNECT_ALLOWED, 0)                                         \
  X(FAITH_CLIENT_RECONNECT_FORBIDDEN, 1)

typedef enum {
#define X(name, value) name = value,
  FAITH_CLIENT_DISCONNECT_POLICIES(X)
#undef X
} faith_client_reconnect_policy_t;

#define FAITH_MSG_REQUEST_FAIL_REASONS(X)                                      \
  X(FAITH_MSG_REQUEST_FAIL_INVALID, 0)                                         \
  X(FAITH_MSG_REQUEST_FAIL_USER_NOT_FOUND, 1)

typedef enum {
#define X(name, value) name = value,
  FAITH_MSG_REQUEST_FAIL_REASONS(X)
#undef X
} faith_msg_request_fail_reason_t;

#define FAITH_MSG_REQUEST_RESPONSE_FAIL_REASONS(X)                             \
  X(FAITH_MSG_REQUEST_RESPONSE_FAIL_INVALID, 0)                                \
  X(FAITH_MSG_REQUEST_RESPONSE_FAIL_INTERNAL_ERROR, 1)                         \
  X(FAITH_MSG_REQUEST_RESPONSE_FAIL_REQUEST_NOT_FOUND, 2)                      \
  X(FAITH_MSG_REQUEST_RESPONSE_FAIL_NOT_RECIPIENT, 3)                          \
  X(FAITH_MSG_REQUEST_RESPONSE_FAIL_ALREADY_RESPONDED, 4)                      \
  X(FAITH_MSG_REQUEST_RESPONSE_FAIL_EXPIRED, 5)

typedef enum {
#define X(name, value) name = value,
  FAITH_MSG_REQUEST_RESPONSE_FAIL_REASONS(X)
#undef X
} faith_msg_request_response_fail_reason_t;

#define FAITH_MSG_REQUEST_RESPONSE_TYPES(X)                                  \
  X(FAITH_MSG_REQUEST_RESPONSE_ACCEPT, 0)                                    \
  X(FAITH_MSG_REQUEST_RESPONSE_DENY, 1)

typedef enum {
#define X(name, value) name = value,
  FAITH_MSG_REQUEST_RESPONSE_TYPES(X)
#undef X
} faith_msg_request_response_type_t;

#define FAITH_DEVICE_LINK_REQ_EXPIRATION_TIME_MS 1000 * 60 /* 60 seconds */
#define FAITH_MSG_REQUEST_EXPIRATION_TIME_MS                                   \
  (1000 * 60 * 60 * 6) /* 6 hours                                              \
                        */
typedef enum {
#define X(name, value) name = value,
  FAITH_ENVELOPE_TYPES(X)
#undef X
} faith_envelope_type_t;

typedef struct {
  uint8_t bytes[FAITH_CLIENT_ID_SIZE];
} faith_client_id_t;

typedef struct {
  uint8_t bytes[FAITH_DEVICE_ID_SIZE];
} faith_device_id_t;

typedef struct {
  uint8_t bytes[FAITH_REQUEST_ID_SIZE];
} faith_request_id_t;

typedef uint32_t faith_body_size_t;

typedef enum {
  FAITH_DEVICE_LINK_DENY = 0,
  FAITH_DEVICE_LINK_APPROVE = 1,
} faith_device_link_response_type_t;

typedef struct {
  faith_envelope_type_t type;

  faith_client_id_t sender_id;
  faith_client_id_t recipient_id;

  faith_body_size_t body_size;

  uint8_t *body;
} faith_envelope_t;

typedef struct {
  // Auth ID of the session that wants to link a new device
  faith_client_id_t auth_id;
  uint8_t           public_key_new_device[FAITH_ED25519_PUBLIC_KEY_SIZE];
  faith_device_id_t device_id_new;

  // 128 bit randomly generated verification code
  uint8_t code[FAITH_DEVICE_LINK_CODE_SIZE];

  uint64_t expires_at_ms;
} faith_envl_stc_device_link_req_t;

typedef struct {
  // 64 byte cryptographic response signature
  uint8_t signature_response[FAITH_ED25519_SIGNATURE_SIZE];
  // device_id of the new device to be linked
  faith_device_id_t device_id_new;

} faith_envl_cts_device_link_response_t;

typedef struct {
  faith_client_reconnect_policy_t  reconnect_policy;
  faith_client_disconnect_reason_t reason;

  uint64_t retry_after_ms;
  uint64_t banned_until_ms;

  uint16_t msg_size;
  char     msg[FAITH_MAX_CLIENT_DISCONNECT_MSG + 1];
} faith_envl_stc_client_disconnect_t;

typedef struct {
  faith_client_id_t auth_id_recv;
  uint64_t          cl_req_id;
} faith_envl_cts_msg_request_t;

typedef struct {
  uint8_t signature_response[FAITH_ED25519_SIGNATURE_SIZE];

  uint64_t cl_req_id;

  faith_request_id_t srv_req_id;

  faith_msg_request_response_type_t type;
} faith_envl_cts_msg_request_response_t;

typedef struct {
  uint64_t           cl_req_id;
  faith_request_id_t srv_req_id;
} faith_envl_stc_msg_request_response_ack_t;

typedef struct {
  faith_msg_request_response_fail_reason_t reason;
  uint64_t                                 cl_req_id;
  faith_request_id_t                       srv_req_id;
} faith_envl_stc_msg_request_response_failed_t;

typedef struct {
  faith_msg_request_fail_reason_t reason;
  uint64_t                        cl_req_id;
} faith_envl_stc_msg_request_failed_t;

typedef struct {
  uint64_t           cl_req_id;
  faith_request_id_t srv_req_id;
} faith_envl_stc_msg_request_ack_t;

typedef struct {
  faith_client_id_t  auth_id_sender;
  faith_request_id_t srv_req_id;
} faith_envl_stc_msg_request_received_t;

typedef struct {
  faith_request_id_t                srv_req_id;
  faith_client_id_t                 auth_id_responder;
  faith_msg_request_response_type_t type;
} faith_envl_stc_msg_request_responded_t;

typedef struct {
  faith_client_id_t auth_id;
  faith_device_id_t device_id;

  uint8_t  public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  uint64_t client_nonce;
  uint64_t server_nonce;

} faith_signature_hello_handshake_t;

typedef struct {
  faith_client_id_t auth_id;
  faith_device_id_t device_id_new;
  uint8_t           public_key_new_device[FAITH_ED25519_PUBLIC_KEY_SIZE];

  uint8_t  code[FAITH_DEVICE_LINK_CODE_SIZE];
  uint64_t expires_at_ms;

  faith_device_id_t                 device_id_responding;
  faith_device_link_response_type_t type;

} faith_signature_device_link_response_t;

typedef struct {
  faith_client_id_t auth_id_req;
  faith_client_id_t auth_id_recv;

  faith_device_id_t device_id_recv;

  faith_request_id_t srv_req_id;

  faith_msg_request_response_type_t type;
} faith_signature_msg_request_response_t;

typedef struct {
  faith_request_id_t srv_req_id;
  faith_client_id_t  auth_id_req;
} faith_msg_request_t;

uint16_t faith_version_pack(uint8_t major, uint8_t minor, uint8_t patch);
uint8_t  faith_version_major(uint16_t v);
uint8_t  faith_version_minor(uint16_t v);
uint8_t  faith_version_patch(uint16_t v);

const char *faith_strerror(int code);

faith_status_code_t faith_write_bytes_sync(SSL *ssl, const uint8_t *buf,
                                           size_t size);
faith_status_code_t faith_read_bytes_sync(SSL *ssl, uint8_t *buf, size_t size);

void faith_frame_free(faith_frame_t *f);

faith_status_code_t faith_read_frame_sync(SSL *ssl, faith_frame_t *out);
faith_status_code_t faith_write_frame_sync(SSL                   *ssl,
                                           faith_frame_msg_type_t type,
                                           const uint8_t         *payload,
                                           size_t                 payload_size);

faith_status_code_t faith_encode_frame(faith_frame_msg_type_t type,
                                       const uint8_t         *payload,
                                       size_t payload_size, uint8_t **out_data,
                                       size_t *out_size);

faith_status_code_t faith_decode_frame(const uint8_t *payload,
                                       size_t payload_size, faith_frame_t *out);

uint64_t faith_now_ms(void);

faith_status_code_t faith_write_u64_be(uint8_t *out_buf, uint64_t val);
faith_status_code_t faith_write_u32_be(uint8_t *out_buf, uint32_t val);
faith_status_code_t faith_write_u16_be(uint8_t *out_buf, uint16_t val);

uint16_t faith_read_u16_be(const uint8_t *p);
uint32_t faith_read_u32_be(const uint8_t *p);
uint64_t faith_read_u64_be(const uint8_t *p);

const char *faith_status_code_name(faith_status_code_t code);
const char *faith_frame_msg_name(faith_frame_msg_type_t msg);
const char *faith_envelope_name(faith_envelope_type_t env);
const char *
faith_client_reconnect_policy_name(faith_client_reconnect_policy_t policy);
const char *
faith_client_disconnect_reason_name(faith_client_disconnect_reason_t reason);
const char *
faith_msg_request_fail_reason_name(faith_msg_request_fail_reason_t reason);
const char *faith_msg_request_response_fail_reason_name(
    faith_msg_request_response_fail_reason_t reason);
const char *
faith_msg_request_response_type_name(
    faith_msg_request_response_type_t type);

faith_status_code_t faith_encode_envelope(uint8_t *out_buf, size_t *out_size,
                                          size_t buf_cap_in_bytes,
                                          const faith_envelope_t *env);

faith_status_code_t faith_decode_envelope(const uint8_t    *payload,
                                          size_t            payload_size,
                                          faith_envelope_t *out);

faith_status_code_t
faith_encode_device_link_req_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_device_link_req_t *in);

faith_status_code_t
faith_decode_device_link_req_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_device_link_req_t *out);

faith_status_code_t faith_encode_device_link_response_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_cts_device_link_response_t *in);

faith_status_code_t faith_decode_device_link_response_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_cts_device_link_response_t *out);

faith_status_code_t faith_encode_client_disconnect_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_client_disconnect_t *in);

faith_status_code_t
faith_decode_client_disconnect_body(const uint8_t    *payload,
                                    faith_body_size_t payload_size,
                                    faith_envl_stc_client_disconnect_t *out);

faith_status_code_t
faith_encode_msg_request_body(uint8_t *out_buf, faith_body_size_t *out_size,
                              size_t buf_cap_in_bytes,
                              const faith_envl_cts_msg_request_t *in);

faith_status_code_t
faith_decode_msg_request_body(const uint8_t                *payload,
                              faith_body_size_t             payload_size,
                              faith_envl_cts_msg_request_t *out);

faith_status_code_t faith_encode_msg_request_response_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_cts_msg_request_response_t *in);

faith_status_code_t
faith_decode_msg_request_response_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_cts_msg_request_response_t *out);

faith_status_code_t faith_encode_msg_request_failed_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_failed_t *in);

faith_status_code_t
faith_decode_msg_request_failed_body(const uint8_t    *payload,
                                     faith_body_size_t payload_size,
                                     faith_envl_stc_msg_request_failed_t *out);

faith_status_code_t
faith_encode_msg_request_ack_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_msg_request_ack_t *in);

faith_status_code_t
faith_decode_msg_request_ack_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_msg_request_ack_t *out);

faith_status_code_t faith_encode_msg_request_received_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_received_t *in);

faith_status_code_t faith_decode_msg_request_received_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_received_t *out);

faith_status_code_t faith_encode_msg_request_response_ack_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_response_ack_t *in);

faith_status_code_t faith_decode_msg_request_response_ack_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_response_ack_t *out);

faith_status_code_t faith_encode_msg_request_response_failed_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_response_failed_t *in);

faith_status_code_t faith_decode_msg_request_response_failed_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_response_failed_t *out);

faith_status_code_t faith_encode_msg_request_responded_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_msg_request_responded_t *in);

faith_status_code_t faith_decode_msg_request_responded_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_stc_msg_request_responded_t *out);

void faith_log_handler(Nob_Log_Level level, const char *fmt, va_list args);

int faith_client_id_equal(faith_client_id_t a, faith_client_id_t b);
int faith_device_id_equal(faith_device_id_t a, faith_device_id_t b);

faith_status_code_t faith_id128_to_hex(const uint8_t bytes[16], char out[33]);

// OpenSSL wrapper
faith_status_code_t faith_random_bytes(uint8_t *o_buf, int num);

// OpenSSL wrapper
faith_status_code_t
faith_gen_ed25519_keypair(void   *handle,
                          uint8_t private_key[FAITH_ED25519_PRIVATE_KEY_SIZE],
                          uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]);

faith_status_code_t
faith_gen_sign_buf_hello_handshake(uint8_t *out_buf, size_t *out_size,
                                   size_t buf_cap_in_bytes,
                                   const faith_signature_hello_handshake_t *n);

faith_status_code_t faith_gen_sign_buf_device_link_response(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_device_link_response_t *in);

faith_status_code_t faith_gen_sign_buf_msg_request_response(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_msg_request_response_t *in);

faith_status_code_t faith_gen_signature(EVP_PKEY *keypair, uint8_t *o_signature,
                                        size_t        *o_signature_size,
                                        const uint8_t *msg_input,
                                        size_t         msg_size);

faith_status_code_t faith_verify_signature(EVP_PKEY      *public_key,
                                           const uint8_t *msg_input,
                                           size_t         msg_size,
                                           const uint8_t *signature,
                                           const size_t   signature_size);

faith_status_code_t faith_verify_signature_raw_pubkey(
    const uint8_t  public_key[FAITH_ED25519_PUBLIC_KEY_SIZE],
    const uint8_t *msg_input, size_t msg_size, const uint8_t *signature,
    size_t signature_size);
