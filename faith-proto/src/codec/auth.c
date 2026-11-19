#include "codec/auth.h"

bool faith_client_id_equal(faith_auth_id_t a, faith_auth_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
}

bool faith_device_id_equal(faith_device_id_t a, faith_device_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
}

bool faith_username_valid(const char *username, size_t size) {
  if (size > FAITH_USERNAME_MAX || (!username && size))
    return false;

  for (size_t i = 0; i < size; i++) {
    char c = username[i];

    if (!((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
          (c >= '1' && c <= '9') || c == '_'))
      return false;
  }

  return true;
}
