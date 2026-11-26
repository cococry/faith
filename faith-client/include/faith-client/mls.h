#pragma once

#include "faith-proto/codec/auth.h"
#include <openssl/evp.h>

typedef struct {
  struct {
    EVP_PKEY *pair;
    uint8_t   pub[FAITH_ED25519_PUBLIC_KEY_SIZE];
    uint8_t   prv[FAITH_ED25519_PRIVATE_KEY_SIZE];
  } signing_key;

  faith_auth_id_t   auth_id;
  faith_device_id_t device_id;
} faith_mls_client_t;

faith_mls_client_t *faith_mls_client_create(const faith_auth_id_t   *auth_id,
                                            const faith_device_id_t *device_id);

void faith_mls_client_destroy(faith_mls_client_t *mls_client);
