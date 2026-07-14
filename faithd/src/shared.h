#pragma once

#include <openssl/err.h>
#include <stdint.h>
#include <time.h>

#include "../third_party/nob.h"

#define FAITH_PROTO_VERSION    faith_version_pack(0, 0, 1)
#define FAITH_MAX_FRAME_LEN    256
#define FAITH_MAX_MSG_SIZE     65536
#define FAITH_MAX_PAYLOAD_SIZE 128

#define FAITH_CLIENT_ID_SIZE 16
#define FAITH_DEVICE_ID_SIZE 16
#define FAITH_CLIENT_ID_NONE ((faith_client_id_t){0})
#define FAITH_DEVICE_ID_NONE ((faith_device_id_t){0})

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
  _FAITH_BODY_SIZE(sizeof(faith_device_id_t) /* client device id */ +          \
                   FAITH_ED25519_PUBLIC_KEY_SIZE /* client public key */ +     \
                   sizeof(uint64_t) /* client nonce */)

#define FAITH_ENVL_HELLO_CHALLENGE_BODY_SIZE                                   \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* server nonce */)

#define FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE                          \
  _FAITH_BODY_SIZE(FAITH_ED25519_SIGNATURE_SIZE /* signature response */ +     \
                   FAITH_DEVICE_ID_SIZE /*device ID new*/)

#define FAITH_DEVICE_LINK_CODE_SIZE 16

#define FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE                               \
  _FAITH_BODY_SIZE(FAITH_CLIENT_ID_SIZE /* auth ID */ +                        \
                   FAITH_ED25519_PUBLIC_KEY_SIZE /*public key new device */ +  \
                   FAITH_DEVICE_ID_SIZE /* device ID new */ +                  \
                   FAITH_DEVICE_LINK_CODE_SIZE /* code */ +                    \
                   sizeof(uint64_t) /* expires_at_ms*/)

#define FAITH_CLIENT_DISCONNECT_MSG_MAX 128

#define FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE                             \
  (sizeof(uint32_t) /* reason */ + sizeof(uint32_t) /* reconnect policy */ +   \
   sizeof(uint64_t) /* retry after milliseconds */ +                           \
   sizeof(uint64_t) /* ban expiration timestamp */ +                           \
   sizeof(uint16_t) /* message length */ +                                     \
   FAITH_CLIENT_DISCONNECT_MSG_MAX /* message bytes */)

#define FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE                              \
  (sizeof(faith_client_id_t) /* auth ID */ +                                   \
   sizeof(faith_device_id_t) /* new device ID */ +                             \
   FAITH_ED25519_PUBLIC_KEY_SIZE /* new device public key */ +                 \
   FAITH_DEVICE_LINK_CODE_SIZE /* device-link verification code */ +           \
   sizeof(uint64_t) /* request expiration timestamp */ +                       \
   sizeof(faith_device_id_t) /* responding authorized device ID */ +           \
   sizeof(uint32_t) /* approve/deny response type */)

#define FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE                                   \
  (sizeof(faith_client_id_t) /* auth ID */ +                                   \
   sizeof(faith_device_id_t) /* client device ID */ +                          \
   FAITH_ED25519_PUBLIC_KEY_SIZE /* client public key */ +                     \
   sizeof(uint64_t) /* client nonce */ + sizeof(uint64_t) /* server nonce */)

#define _FH_CHECK_RETURN(expr)                                                 \
  do {                                                                         \
    faith_status_code_t _fh_rc = (expr);                                       \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(ERROR,                                                           \
              "_FH_CHECK_RETURN failed:\n"                                          \
              "  expression : %s\n"                                            \
              "  status     : %s (%d)\n"                                       \
              "  function   : %s\n"                                            \
              "  location   : %s:%d",                                          \
              #expr, faith_status_code_name(_fh_rc), (int)_fh_rc, __func__,    \
              __FILE__, __LINE__);                                             \
      return _fh_rc;                                                           \
    }                                                                          \
  } while (0)

#define _FH_CHECK_GOTO(expr, result, label)                                    \
  do {                                                                         \
    faith_status_code_t _fh_rc = (expr);                                       \
    result = _fh_rc;                                                           \
    if (_fh_rc != FAITH_OK) {                                                  \
      nob_log(ERROR,                                                           \
              "_FH_CHECK_GOTO failed:\n"                                          \
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
              "_FH_CHECK failed:\n"                                          \
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
  X(FAITH_ERR_BAD_ENVELOPE, 21)

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
  X(FAITH_EVENT_ERROR, 5)                                                      \
  X(FAITH_EVENT_MESSAGE_RECEIVED, 6)                                           \
  X(FAITH_EVENT_DEVICE_AUTH_PENDING, 7)                                        \
  X(FAITH_EVENT_DEVICE_LINK_REQUEST, 8)                                        \
  X(FAITH_EVENT_DEVICE_AUTH_RESPONSE_ACK, 9)                                   \
  X(FAITH_EVENT_DEVICE_LINK_CANCELLED, 10)                                     \
  X(FAITH_EVENT_AUTHORIZED, 11)

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
  X(FAITH_ENVELOPE_MSG_ERR, 5)                                                 \
  X(FAITH_ENVELOPE_CHALLENGE, 6)                                               \
  X(FAITH_ENVELOPE_CHALLENGE_RESPONSE, 7)                                      \
  X(FAITH_ENVELOPE_DEVICE_LINK_REQUEST, 8)                                     \
  X(FAITH_ENVELOPE_DEVICE_AUTH_PENDING, 9)                                     \
  X(FAITH_ENVELOPE_DEVICE_AUTH_APPROVE, 10)                                    \
  X(FAITH_ENVELOPE_DEVICE_AUTH_DENY, 11)                                       \
  X(FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_ACK, 12)                               \
  X(FAITH_ENVELOPE_DEVICE_LINK_CANCELLED, 13)                                  \
  X(FAITH_ENVELOPE_CLIENT_DISCONNECT, 14)

#define FAITH_CLIENT_DISCONNECT_REASONS(X)                                     \
  X(FAITH_DISCONNECT_REASON_NONE, 0)                                           \
  /* Temporary transport/server conditions */                                  \
  X(FAITH_DISCONNECT_SERVER_SHUTDOWN, 1)                                       \
  X(FAITH_DISCONNECT_SERVER_BUSY, 2)                                           \
  X(FAITH_DISCONNECT_RATE_LIMITED, 3)                                          \
  X(FAITH_DISCONNECT_TEMPORARY_FAILURE, 4)                                     \
  /* Protocol/auth failures */                                                 \
  X(FAITH_DISCONNECT_BAD_PROTOCOL, 5)                                          \
  X(FAITH_DISCONNECT_UNSUPPORTED_VERSION, 6)                                   \
  X(FAITH_DISCONNECT_AUTH_FAILED, 7)                                           \
  X(FAITH_DISCONNECT_DEVICE_REJECTED, 8)                                       \
  X(FAITH_DISCONNECT_DUPLICATE_SESSION, 9)                                     \
  /* Administrative/security actions */                                        \
  X(FAITH_DISCONNECT_IDENTITY_BANNED, 10)                                      \
  X(FAITH_DISCONNECT_DEVICE_BANNED, 11)                                        \
  X(FAITH_DISCONNECT_IP_BANNED, 12)                                            \
  X(FAITH_DISCONNECT_ABUSE, 13)

typedef enum {
#define X(name, value) name = value,
  FAITH_CLIENT_DISCONNECT_REASONS(X)
#undef X
} faith_client_disconnect_reason_t;

#define FAITH_CLIENT_DISCONNECT_POLICIES(X)                                    \
  X(FAITH_CLIENT_RECONNECT_ALLOWED, 0)                                         \
  X(FAITH_CLIENT_RECONNECT_FORBIDDEN, 1)                                       \
  X(FAITH_CLIENT_RECONNECT_AFTER_DELAY, 2)

typedef enum {
#define X(name, value) name = value,
  FAITH_CLIENT_DISCONNECT_POLICIES(X)
#undef X
} faith_client_reconnect_policy_t;

#define FAITH_DEVICE_LINK_REQ_EXPIRATION_TIME_MS 1000 * 30 // 30 seconds

typedef enum {
#define X(name, value) name = value,
  FAITH_ENVELOPE_TYPES(X)
#undef X
} faith_envelope_type_t;

typedef struct {
  uint8_t bytes[16];
} faith_client_id_t;

typedef struct {
  uint8_t bytes[16];
} faith_device_id_t;

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

  char msg[FAITH_CLIENT_DISCONNECT_MSG_MAX];
} faith_envl_stc_client_disconnect_t;

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
const char *
faith_client_reconnect_policy_name(faith_client_reconnect_policy_t policy);
const char *
faith_client_disconnect_reason_name(faith_client_disconnect_reason_t reason);

faith_status_code_t faith_encode_envelope(uint8_t *out_buf, size_t *out_size,
                                          size_t buf_cap_in_bytes,
                                          const faith_envelope_t *env);

faith_status_code_t faith_decode_envelope(const uint8_t    *payload,
                                          size_t            payload_size,
                                          faith_envelope_t *o_envl);

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

faith_status_code_t
faith_encode_client_disconnect(uint8_t *out_buf, faith_body_size_t *out_size,
                               size_t buf_cap_in_bytes,
                               const faith_envl_stc_client_disconnect_t *in);

faith_status_code_t
     faith_decode_client_disconnect(const uint8_t                      *payload,
                                    faith_body_size_t                   payload_size,
                                    faith_envl_stc_client_disconnect_t *out);
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

faith_status_code_t faith_gen_sign_buf_hello_handshake(
    uint8_t *o_buf, size_t buf_cap_in_bytes,
    const faith_signature_hello_handshake_t *src);

faith_status_code_t faith_gen_sign_buf_device_link_response(
    uint8_t *o_buf, size_t buf_cap_in_bytes,
    const faith_signature_device_link_response_t *src);

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
