#pragma once

#include <stdint.h>

#define NOB_STRIP_PREFIX
#include "../../third_party/nob.h"

#define _FH_CHECK_GOTO(expr, result, label)                                    \
  do {                                                                         \
    faith_status_code_t _fh_rc = (expr);                                       \
    result = _fh_rc;                                                           \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(NOB_ERROR,                                                       \
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

#define _FH_CHECK_SCOPED(expr)                                                 \
  do {                                                                         \
    _FH_CHECK(expr);                                                           \
  } while (0)

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
  X(FAITH_ERR_ALREADY_EXISTS, 24)                                              \
  X(FAITH_ERR_ALREADY_REMOVED, 25)                                             \
  X(FAITH_ERR_EPOLL, 26)

typedef enum {
#define X(name, value) name = value,
  FAITH_STATUS_CODES(X)
#undef X
} faith_status_code_t;

#define FAITH_PROTO_VERSION faith_version_pack(0, 0, 1)

uint16_t faith_version_pack(uint8_t major, uint8_t minor, uint8_t patch);
uint8_t  faith_version_major(uint16_t v);
uint8_t  faith_version_minor(uint16_t v);
uint8_t  faith_version_patch(uint16_t v);

const char *faith_status_code_name(faith_status_code_t code);

uint64_t faith_now_ms(void);

faith_status_code_t faith_id128_to_hex(const uint8_t bytes[16], char out[33]);

void faith_log_handler(Nob_Log_Level level, const char *fmt, va_list args);
