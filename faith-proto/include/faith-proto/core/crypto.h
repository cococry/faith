#pragma once

#include "core.h"
#include <openssl/err.h>

#define FAITH_ED25519_SIGNATURE_SIZE 64

#define FAITH_ED25519_PUBLIC_KEY_SIZE  32
#define FAITH_ED25519_PRIVATE_KEY_SIZE 32
#define FAITH_ED25519_SIGNATURE_SIZE   64

faith_status_code_t faith_random_bytes(uint8_t *o_buf, int num);

faith_status_code_t
faith_gen_ed25519_keypair(void   *handle,
                          uint8_t private_key[FAITH_ED25519_PRIVATE_KEY_SIZE],
                          uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]);

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
