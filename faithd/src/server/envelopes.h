#pragma once

#include <stdint.h>

#define FAITH_MAX_CLIENT_DISCONNECT_MSG 128

#define FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED                       \
  _FAITH_BODY_SIZE(sizeof(uint32_t) /* reason */ +                             \
                   sizeof(uint32_t) /* reconnect policy */ +                   \
                   sizeof(uint64_t) /* retry after milliseconds */ +           \
                   sizeof(uint64_t) /* ban expiration timestamp */ +           \
                   sizeof(uint16_t) /* message length */)

#define FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_MAX                         \
  _FAITH_BODY_SIZE(FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_FIXED +          \
                   FAITH_MAX_CLIENT_DISCONNECT_MSG)

#define FAITH_CLIENT_DISCONNECT_REASONS(X)                                     \
  X(FAITH_DISCONNECT_REASON_NONE, 0)                                           \
  X(FAITH_DISCONNECT_SERVER_SHUTDOWN, 1)                                       \
  X(FAITH_DISCONNECT_SERVER_BUSY, 2)                                           \
  X(FAITH_DISCONNECT_RATE_LIMITED, 3)                                          \
  X(FAITH_DISCONNECT_TEMPORARY_FAILURE, 4)                                     \
  X(FAITH_DISCONNECT_BAD_PROTOCOL, 5)                                          \
  X(FAITH_DISCONNECT_UNSUPPORTED_VERSION, 6)                                   \
  X(FAITH_DISCONNECT_AUTH_FAILED, 7)                                           \
  X(FAITH_DISCONNECT_DEVICE_REJECTED, 8)                                       \
  X(FAITH_DISCONNECT_DUPLICATE_SESSION, 9)                                     \
  X(FAITH_DISCONNECT_IDENTITY_BANNED, 10)                                      \
  X(FAITH_DISCONNECT_DEVICE_BANNED, 11)                                        \
  X(FAITH_DISCONNECT_IP_BANNED, 12)                                            \
  X(FAITH_DISCONNECT_ABUSE, 13)                                                \
  X(FAITH_DISCONNECT_MEMORY_LIMIT, 14)                                         \
  X(FAITH_DISCONNECT_INTERNAL_ERROR, 15)

typedef enum {
#define X(name, value) name = value,
  FAITH_CLIENT_DISCONNECT_REASONS(X)
#undef X
} faith_client_disconnect_reason_t;

#define FAITH_CLIENT_DISCONNECT_POLICIES(X)                                    \
  X(FAITH_CLIENT_RECONNECT_ALLOWED, 0)                                         \
  X(FAITH_CLIENT_RECONNECT_FORBIDDEN, 1)

typedef enum {
#define X(name, value) name = value,
  FAITH_CLIENT_DISCONNECT_POLICIES(X)
#undef X
} faith_client_reconnect_policy_t;

typedef struct {
  faith_client_reconnect_policy_t  reconnect_policy;
  faith_client_disconnect_reason_t reason;

  uint64_t retry_after_ms;
  uint64_t banned_until_ms;

  uint16_t msg_size;
  char     msg[FAITH_MAX_CLIENT_DISCONNECT_MSG + 1];
} faith_envl_stc_client_disconnect_t;
