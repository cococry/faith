#pragma once

#include <stdint.h>

#include "../auth/structs.h"
#include "../core/core.h"

#define FAITH_EVENT_TYPES(X)                                                   \
  X(FAITH_EVENT_NONE, 0)                                                       \
  X(FAITH_EVENT_CONNECTING, 1)                                                 \
  X(FAITH_EVENT_CONNECTED, 2)                                                  \
  X(FAITH_EVENT_DISCONNECTED, 3)                                               \
  X(FAITH_EVENT_PONG, 4)                                                       \
  X(FAITH_EVENT_ERROR, 5)                                                      \
  X(FAITH_EVENT_MESSAGE_RECEIVED, 6)                                           \
  X(FAITH_EVENT_DEVICE_AUTH_PENDING, 7)                                        \
  X(FAITH_EVENT_DEVICE_LINK_REQUEST, 8)                                        \
  X(FAITH_EVENT_DEVICE_AUTH_RESPONSE_ACK, 9)                                   \
  X(FAITH_EVENT_DEVICE_AUTH_RESPONSE_FAILED, 10)                               \
  X(FAITH_EVENT_DEVICE_LINK_CANCELLED, 11)                                     \
  X(FAITH_EVENT_AUTHORIZED, 12)                                                \
  X(FAITH_EVENT_SERVER_DISCONNECT, 13)                                         \
  X(FAITH_EVENT_MSG_REQUEST_RECEIVED, 14)                                      \
  X(FAITH_EVENT_MSG_REQUEST_RESPONDED, 15)                                     \
  X(FAITH_EVENT_MSG_REQUEST_ACK, 16)                                           \
  X(FAITH_EVENT_MSG_REQUEST_FAILED, 17)                                        \
  X(FAITH_EVENT_MSG_REQUEST_RESPONSE_ACK, 18)                                  \
  X(FAITH_EVENT_MSG_REQUEST_RESPONSE_FAILED, 19)

typedef enum {
#define X(name, value) name = value,
  FAITH_EVENT_TYPES(X)
#undef X
} faith_event_type_t;

typedef struct faith_client faith_client_t;

typedef struct {

  const char *host;
  uint16_t    port;

  const char *server_name;
  const char *ca_file;
  int         insecure_skip_verify;

  uint16_t proto_ver;

} faith_client_config_t;

typedef struct {
  uint32_t type;
  uint64_t value0;
  uint64_t value1;

  uint8_t value0_128[16];
  uint8_t value1_128[16];
  uint8_t value2_128[16];

  char message[256];

  char  *chat_message;
  size_t chat_message_size;
} faith_event_t;

faith_status_code_t faith_client_init_global(int log_enable_tracing);

faith_client_t *faith_client_create(const faith_client_config_t *cfg);
void            faith_client_destroy(faith_client_t *client);

faith_status_code_t faith_client_start(faith_client_t *client);
faith_status_code_t faith_client_stop(faith_client_t *client);

faith_status_code_t faith_client_send_msg(faith_client_t   *client,
                                          faith_client_id_t recipient_auth_id,
                                          const char       *msg);

int faith_client_event_fd(faith_client_t *client);

faith_status_code_t faith_client_next_event(faith_client_t *client,
                                            faith_event_t  *out);
faith_status_code_t faith_client_free_event(faith_event_t *ev);

faith_status_code_t
faith_client_approve_pending_device_auth(faith_client_t *client);

faith_status_code_t
faith_client_deny_pending_device_auth(faith_client_t *client);

faith_status_code_t faith_client_reconnect(faith_client_t *client);

const char *faith_event_name(faith_event_type_t ev);
