#pragma once

#include <stdint.h>

#include "helpers.h"

#define FAITH_MSG_PONG_PAYLOAD_SIZE                                            \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* client-server nonce */ +                \
                   sizeof(uint64_t)) /* server sent at ms */

#define FAITH_MSG_PING_PAYLOAD_SIZE                                            \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* client-server nonce */ +                \
                   sizeof(uint64_t)) /* server sent at ms */

#define FAITH_MSG_TYPES(X)                                                     \
  X(FAITH_MSG_PING, 0)                                                         \
  X(FAITH_MSG_PONG, 1)                                                         \
  X(FAITH_MSG_ENVL, 2)

typedef enum {
#define X(name, value) name = value,
  FAITH_MSG_TYPES(X)
#undef X
} faith_frame_msg_type_t;

typedef struct {
  uint64_t nonce;
  uint64_t client_sent_at_ms;
} faith_msg_ping_t;

typedef struct {
  uint64_t nonce;
  uint64_t server_sent_at_ms;
} faith_msg_pong_t;

const char *faith_frame_msg_name(faith_frame_msg_type_t msg);
