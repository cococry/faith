#pragma once

#include "../core/core.h"
#include <stdint.h>

#define _FAITH_BODY_SIZE(X) ((faith_body_size_t)((X)))

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

#define _FAITH_DECODE_IMPL(payload, payload_size, offset, dst, size,           \
                           invalid_failure, bad_frame_failure)                 \
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
      invalid_failure;                                                         \
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
      invalid_failure;                                                         \
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
      bad_frame_failure;                                                       \
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
      bad_frame_failure;                                                       \
    }                                                                          \
                                                                               \
    if (_fh_size > 0)                                                          \
      memcpy((dst), (payload) + _fh_offset, _fh_size);                         \
                                                                               \
    (offset) += _fh_size;                                                      \
  } while (0)

#define FAITH_DECODE_RETURN(payload, payload_size, offset, dst, size)          \
  _FAITH_DECODE_IMPL(payload, payload_size, offset, dst, size,                 \
                     return FAITH_ERR_INVALID, return FAITH_ERR_BAD_FRAME)

#define FAITH_DECODE_DEFER(payload, payload_size, offset, dst, size)           \
  _FAITH_DECODE_IMPL(payload, payload_size, offset, dst, size,                 \
                     _FH_RETURN_DEFER(FAITH_ERR_INVALID),                      \
                     _FH_RETURN_DEFER(FAITH_ERR_BAD_FRAME))

#define _FAITH_DECODE_INT_BE_IMPL(payload, payload_size, offset, out, type,    \
                                  read_fn, invalid_failure, bad_frame_failure) \
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
      invalid_failure;                                                         \
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
      bad_frame_failure;                                                       \
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
      bad_frame_failure;                                                       \
    }                                                                          \
                                                                               \
    (out) = read_fn((payload) + _fh_offset);                                   \
    (offset) += _fh_int_size;                                                  \
  } while (0)

#define _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, type,  \
                                    read_fn)                                   \
  _FAITH_DECODE_INT_BE_IMPL(payload, payload_size, offset, out, type, read_fn, \
                            return FAITH_ERR_INVALID,                          \
                            return FAITH_ERR_BAD_FRAME)

#define _FAITH_DECODE_INT_BE_DEFER(payload, payload_size, offset, out, type,   \
                                   read_fn)                                    \
  _FAITH_DECODE_INT_BE_IMPL(payload, payload_size, offset, out, type, read_fn, \
                            _FH_RETURN_DEFER(FAITH_ERR_INVALID),               \
                            _FH_RETURN_DEFER(FAITH_ERR_BAD_FRAME))

#define FAITH_DECODE_U16_BE_RETURN(payload, payload_size, offset, out)         \
  _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, uint16_t,    \
                              faith_read_u16_be)

#define FAITH_DECODE_U32_BE_RETURN(payload, payload_size, offset, out)         \
  _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, uint32_t,    \
                              faith_read_u32_be)

#define FAITH_DECODE_U64_BE_RETURN(payload, payload_size, offset, out)         \
  _FAITH_DECODE_INT_BE_RETURN(payload, payload_size, offset, out, uint64_t,    \
                              faith_read_u64_be)

#define FAITH_DECODE_U16_BE_DEFER(payload, payload_size, offset, out)          \
  _FAITH_DECODE_INT_BE_DEFER(payload, payload_size, offset, out, uint16_t,     \
                             faith_read_u16_be)

#define FAITH_DECODE_U32_BE_DEFER(payload, payload_size, offset, out)          \
  _FAITH_DECODE_INT_BE_DEFER(payload, payload_size, offset, out, uint32_t,     \
                             faith_read_u32_be)

#define FAITH_DECODE_U64_BE_DEFER(payload, payload_size, offset, out)          \
  _FAITH_DECODE_INT_BE_DEFER(payload, payload_size, offset, out, uint64_t,     \
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

#define FAITH_DECODE_PROLOGUE(envl_size, sign)                                 \
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

#define _FAITH_DECODE_EPILOGUE_IMPL(envl_size, condition, on_failure)          \
  do {                                                                         \
    const size_t _fh_expected_size = (size_t)(envl_size);                      \
    const size_t _fh_offset = (size_t)offset;                                  \
    const size_t _fh_payload_size = (size_t)payload_size;                      \
                                                                               \
    if (condition) {                                                           \
      _FH_LOG_CODEC_FAILURE("Envelope decode",                                 \
                            "final decode offset is inconsistent",             \
                            "  offset        : %zu\n"                          \
                            "  payload size  : %zu\n"                          \
                            "  expected size : %zu",                           \
                            _fh_offset, _fh_payload_size, _fh_expected_size);  \
      on_failure;                                                              \
    }                                                                          \
  } while (0)

#define FAITH_DECODE_EPILOGUE(envl_size, sign)                                 \
  _FAITH_DECODE_EPILOGUE_IMPL(                                                 \
      envl_size,                                                               \
      (_fh_offset sign _fh_expected_size || _fh_offset sign _fh_payload_size), \
      return FAITH_ERR_BAD_FRAME)

#define FAITH_DECODE_EPILOGUE_DEFER(envl_size, sign)                           \
  _FAITH_DECODE_EPILOGUE_IMPL(                                                 \
      envl_size,                                                               \
      (_fh_offset sign _fh_expected_size || _fh_offset sign _fh_payload_size), \
      _FH_RETURN_DEFER(FAITH_ERR_BAD_FRAME))

#define FAITH_ENCODE_PROLOGUE(envl_size)                                       \
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

#define FAITH_ENCODE_EPILOGUE(envl_size, sign)                                 \
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

faith_status_code_t faith_write_u16_be(uint8_t *out_buf, uint16_t val);
faith_status_code_t faith_write_u32_be(uint8_t *out_buf, uint32_t val);
faith_status_code_t faith_write_u64_be(uint8_t *out_buf, uint64_t val);

uint16_t faith_read_u16_be(const uint8_t *p);
uint32_t faith_read_u32_be(const uint8_t *p);
uint64_t faith_read_u64_be(const uint8_t *p);
