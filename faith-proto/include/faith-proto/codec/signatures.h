#pragma once

#include "../core/crypto.h"

#include "auth.h" 

#define FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE                              \
  (FAITH_AUTH_ID_SIZE /* auth ID */ +                                          \
   FAITH_DEVICE_ID_SIZE /* new device ID */ +                                  \
   FAITH_ED25519_PUBLIC_KEY_SIZE /* new device public key */ +                 \
   FAITH_DEVICE_LINK_CODE_SIZE /* device-link verification code */ +           \
   sizeof(uint64_t) /* request expiration timestamp */ +                       \
   FAITH_DEVICE_ID_SIZE /* responding authorized device ID */ +                \
   sizeof(uint32_t) /* approve/deny response type */)

#define FAITH_SIGNATURE_MSG_REQUEST_RESPONSE_SIZE                              \
  (FAITH_AUTH_ID_SIZE /* auth ID requesting */ +                               \
   FAITH_AUTH_ID_SIZE /* auth ID receiving */ +                                \
   FAITH_DEVICE_ID_SIZE /* device id receiving */) +                           \
      FAITH_REQUEST_ID_SIZE /* server request ID */ +                          \
      sizeof(uint32_t) /* type */

#define FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE                                   \
  (FAITH_AUTH_ID_SIZE /* auth ID */ +                                          \
   FAITH_DEVICE_ID_SIZE /* client device ID */ +                               \
   FAITH_ED25519_PUBLIC_KEY_SIZE /* client public key */ +                     \
   sizeof(uint64_t) /* client nonce */ + sizeof(uint64_t) /* server nonce */)

typedef struct {
  faith_auth_id_t   auth_id;
  faith_device_id_t device_id;

  uint8_t  public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  uint64_t client_nonce;
  uint64_t server_nonce;

} faith_signature_hello_handshake_t;

typedef struct {
  faith_auth_id_t   auth_id;
  faith_device_id_t device_id_new;
  uint8_t           public_key_new_device[FAITH_ED25519_PUBLIC_KEY_SIZE];

  uint8_t  code[FAITH_DEVICE_LINK_CODE_SIZE];
  uint64_t expires_at_ms;

  faith_device_id_t                 device_id_responding;
  faith_device_link_response_type_t type;

} faith_signature_device_link_response_t;

faith_status_code_t
faith_gen_sign_buf_hello_handshake(uint8_t *out_buf, size_t *out_size,
                                   size_t buf_cap_in_bytes,
                                   const faith_signature_hello_handshake_t *n);

faith_status_code_t faith_gen_sign_buf_device_link_response(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_device_link_response_t *in);
