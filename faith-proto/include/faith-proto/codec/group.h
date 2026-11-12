#pragma once
#include "../core/core.h"
#include "auth.h"
#include "core_types.h"

#define FAITH_GROUP_MAX_MEMBERS 32u
#define FAITH_GROUP_MAX_TARGETS 64u
#define FAITH_GROUP_MAX_TEXT    4096u
#define FAITH_GROUP_HEADER_SIZE 48u
#define FAITH_GROUP_MAX_PACKET                                                 \
  (FAITH_GROUP_HEADER_SIZE + 16u * FAITH_GROUP_MAX_MEMBERS +                   \
   FAITH_GROUP_MAX_TEXT)
#define FAITH_GROUP_PLAINTEXT_V1 1u

typedef enum {
  FAITH_GROUP_CREATE = 1,
  FAITH_GROUP_UPDATE = 2,
  FAITH_GROUP_MESSAGE = 3
} faith_group_kind_t;

typedef struct {
  uint8_t            format;
  faith_group_kind_t kind;
  uint64_t           revision;
  uint8_t            conversation_id[16];
  uint8_t            message_id[16];
  uint16_t           n_members;
  faith_auth_id_t    members[FAITH_GROUP_MAX_MEMBERS];
  const uint8_t     *text;
  uint32_t           text_size;
} faith_group_packet_t;

faith_status_code_t faith_group_encode(uint8_t *out, size_t cap, size_t *size,
                                       const faith_group_packet_t *packet);

faith_status_code_t faith_group_decode(const uint8_t *data, size_t size,
                                       faith_group_packet_t *packet);
