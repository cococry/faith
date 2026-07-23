#pragma once

#include "../auth/structs.h"

#include <stdint.h>

#define FAITH_ENVELOPE_TYPES(X)                                                \
  X(FAITH_ENVELOPE_HELLO, 0)                                                   \
  X(FAITH_ENVELOPE_HELLO_OK, 1)                                                \
  X(FAITH_ENVELOPE_MSG_SEND, 2)                                                \
  X(FAITH_ENVELOPE_MSG_DELIVER, 3)                                             \
  X(FAITH_ENVELOPE_MSG_ACK, 4)                                                 \
  X(FAITH_ENVELOPE_MSG_ERR, 5)                                                 \
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
  X(FAITH_ENVELOPE_MSG_REQUEST, 16)                                            \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONSE, 17)                                   \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONSE_ACK, 18)                               \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONSE_FAILED, 19)                            \
  X(FAITH_ENVELOPE_MSG_REQUEST_ACK, 20)                                        \
  X(FAITH_ENVELOPE_MSG_REQUEST_FAILED, 21)                                     \
  X(FAITH_ENVELOPE_MSG_REQUEST_RECEIVED, 22)                                   \
  X(FAITH_ENVELOPE_MSG_REQUEST_RESPONDED, 23)

typedef enum {
#define X(name, value) name = value,
  FAITH_ENVELOPE_TYPES(X)
#undef X
} faith_envelope_type_t;

typedef uint32_t faith_body_size_t;
#define FAITH_BODY_SIZE_T_MAX UINT32_MAX

typedef struct {
  faith_envelope_type_t type;

  faith_client_id_t sender_id;
  faith_client_id_t recipient_id;

  faith_body_size_t body_size;

  uint8_t *body;
} faith_envelope_t;

const char *faith_envelope_name(faith_envelope_type_t env);
