#include "helpers.h"

#include <stdlib.h>

faith_status_code_t faith_write_u16_be(uint8_t *out_buf, uint16_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

faith_status_code_t faith_write_u32_be(uint8_t *out_buf, uint32_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

faith_status_code_t faith_write_u64_be(uint8_t *out_buf, uint64_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

uint16_t faith_read_u16_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint16_t)p[0] << 8) | ((uint16_t)p[1]);
}

uint32_t faith_read_u32_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
         ((uint32_t)p[2] << 8) | ((uint32_t)p[3]);
}

uint64_t faith_read_u64_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint64_t)p[0] << 56) | ((uint64_t)p[1] << 48) |
         ((uint64_t)p[2] << 40) | ((uint64_t)p[3] << 32) |
         ((uint64_t)p[4] << 24) | ((uint64_t)p[5] << 16) |
         ((uint64_t)p[6] << 8) | ((uint64_t)p[7]);
}
