#pragma once

#include "../auth/structs.h"

typedef struct {
  faith_auth_id_t   auth_id;
  faith_device_id_t device_id;
  uint8_t           public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
} application_user_identity_t;
