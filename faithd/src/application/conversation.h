#pragma once

#include <stdint.h>

#define FAITH_CONVERSATION_ID_SIZE 16

typedef struct {
  uint8_t bytes[FAITH_CONVERSATION_ID_SIZE];
} faith_conversation_id_t;
