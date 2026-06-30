#pragma once

#include <openssl/err.h>
#include <stdint.h>
#include <time.h>

#include "../nob.h"

#define FAITH_PROTO_VERSION    faith_version_pack(0, 0, 1)
#define FAITH_MAX_FRAME_LEN    128
#define FAITH_MAX_PAYLOAD_SIZE 128
#define FAITH_HEADER_SIZE                                                      \
  sizeof(uint32_t) /* frame size    */ +                                       \
      sizeof(uint16_t) /* proto version */ +                                   \
      sizeof(uint16_t) /* message type  */
#define FAITH_ENVL_HEADER_SIZE                                                 \
  (sizeof(uint32_t) /* envelope type */ +                                      \
   sizeof(uint32_t) /* sender id     */ +                                      \
   sizeof(uint32_t) /* recipient id  */ + sizeof(uint32_t) /* body size */)

#define _FH_CHECK_RETURN(expr)                                                 \
  do {                                                                         \
    faith_status_code_t _fh_rc = (expr);                                       \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(ERROR, "%s failed: %s (%d)", #expr,                              \
              faith_status_code_name(_fh_rc), (int)_fh_rc);                    \
      return _fh_rc;                                                           \
    }                                                                          \
  } while (0)

#define _FH_CHECK(expr)                                                        \
  faith_status_code_t _fh_rc = (expr);                                         \
  do {                                                                         \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(ERROR, "%s failed: %s (%d)", #expr,                              \
              faith_status_code_name(_fh_rc), (int)_fh_rc);                    \
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
  X(FAITH_ERR_UNAUTHORIZED, 13)

typedef enum {
#define X(name, value) name = value,
  FAITH_STATUS_CODES(X)
#undef X
} faith_status_code_t;

#define FAITH_EVENT_TYPES(X)                                                   \
  X(FAITH_EVENT_NONE, 0)                                                       \
  X(FAITH_EVENT_CONNECTING, 1)                                                 \
  X(FAITH_EVENT_CONNECTED, 2)                                                  \
  X(FAITH_EVENT_DISCONNECTED, 3)                                               \
  X(FAITH_EVENT_PONG, 4)                                                       \
  X(FAITH_EVENT_ERROR, 5)

typedef enum {
#define X(name, value) name = value,
  FAITH_EVENT_TYPES(X)
#undef X
} faith_event_type_t;

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
  X(FAITH_ENVELOPE_MSG_ERR, 5)

typedef enum {
#define X(name, value) name = value,
  FAITH_ENVELOPE_TYPES(X)
#undef X
} faith_envelope_type_t;

typedef uint32_t client_id_t;
typedef uint32_t device_id_t;

typedef struct {
  faith_envelope_type_t type;

  client_id_t sender_id;
  client_id_t recipient_id;

  uint32_t body_size;

  uint8_t *body;
} faith_envelope_t;

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

uint64_t faith_now_ms(void);

faith_status_code_t faith_write_u64_be(uint8_t *out_buf, uint64_t val);
faith_status_code_t faith_write_u32_be(uint8_t *out_buf, uint32_t val);
faith_status_code_t faith_write_u16_be(uint8_t *out_buf, uint16_t val);

uint16_t faith_read_u16_be(const uint8_t *p);
uint32_t faith_read_u32_be(const uint8_t *p);
uint64_t faith_read_u64_be(const uint8_t *p);

const char *faith_status_code_name(faith_status_code_t code);
const char *faith_event_name(faith_event_type_t ev);
const char *faith_frame_msg_name(faith_frame_msg_type_t msg);
const char *faith_envelope_name(faith_envelope_type_t env);

faith_status_code_t faith_encode_envelope(uint8_t *out_buf, size_t *out_size,
                                          size_t buf_cap_in_bytes,
                                          const faith_envelope_t *env);

void faith_log_handler(Nob_Log_Level level, const char *fmt, va_list args);
