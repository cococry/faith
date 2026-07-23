#include "structs.h"

bool faith_client_id_equal(faith_client_id_t a, faith_client_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
}

bool faith_device_id_equal(faith_device_id_t a, faith_device_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
}
