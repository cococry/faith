#pragma once

#include "../auth/structs.h"
#include "core.h"

#include <stdint.h>

#define FAITH_ENVELOPE_TYPES(X)                                                \
  X(FAITH_ENVELOPE_HELLO, 0)                                                   \
  X(FAITH_ENVELOPE_HELLO_OK, 1)                                                \
  X(FAITH_ENVELOPE_MSG_SEND, 2)                                                \
  X(FAITH_ENVELOPE_CHALLENGE, 6)                                               \
  X(FAITH_ENVELOPE_CHALLENGE_RESPONSE, 7)                                      \
  X(FAITH_ENVELOPE_DEVICE_LINK_REQUEST, 8)                                     \
  X(FAITH_ENVELOPE_DEVICE_AUTH_PENDING, 9)                                     \
  X(FAITH_ENVELOPE_DEVICE_AUTH_APPROVE, 10)                                    \
  X(FAITH_ENVELOPE_DEVICE_AUTH_DENY, 11)                                       \
  X(FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_ACK, 12)                               \
  X(FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_FAILED, 13)                            \
  X(FAITH_ENVELOPE_DEVICE_LINK_CANCELLED, 14)                                  \
  X(FAITH_ENVELOPE_CLIENT_DISCONNECT, 15)                                      \
  X(FAITH_ENVELOPE_COMMAND, 16)                                                \
  X(FAITH_ENVELOPE_COMMAND_RESULT, 17)                                         \
  X(FAITH_ENVELOPE_EVENT, 18)

typedef enum {
#define X(name, value) name = value,
  FAITH_ENVELOPE_TYPES(X)
#undef X
} faith_envelope_type_t;

#define FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED                                   \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* sequence number */ +                    \
                   sizeof(uint32_t) /* type */ +                               \
                   sizeof(faith_body_size_t) /* data size */                   \
  )

#define FAITH_EVENTS_ENVL(X) X(FAITH_EVENT_CONVERSATION_CREATED, 0)

typedef enum {
#define X(name, value) name = value,
  FAITH_EVENTS_ENVL(X)
#undef X
} faith_event_envelope_type_t;

typedef struct {
  faith_envelope_type_t type;

  faith_auth_id_t sender_id;
  faith_auth_id_t recipient_id;

  faith_body_size_t body_size;
  uint8_t          *body;
} faith_envelope_t;

typedef struct {
  /* The server-generated sequence number
   * of this event */
  uint64_t seq_num;

  faith_event_envelope_type_t type;

  faith_body_size_t data_size;
  uint8_t          *data;
} faith_envl_stc_event_t;

const char *faith_envelope_name(faith_envelope_type_t env);
