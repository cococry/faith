#pragma once

#include "../protocol.h"

typedef struct {
  uint64_t          nonce;
  uint64_t          server_nonce;
  uint8_t           public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  faith_client_id_t sender_auth_id;
  faith_device_id_t device_id;
} client_auth_handshake_params_t;

