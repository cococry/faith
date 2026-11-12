#pragma once

#include <stdint.h>

#include <faith-proto/codec/auth.h>
#include <faith-proto/codec/group.h>
#include <faith-proto/core/core.h>

#define FAITH_CLIENT_COMMAND_TIMEOUT 10000 // 10 seconds

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
  X(FAITH_EVENT_COMMAND_RESULT, 14)                                            \
  X(FAITH_EVENT_COMMAND_FAILED, 15)                                            \
  X(FAITH_EVENT_CONVERSATION_READY, 16)                                        \
  X(FAITH_EVENT_GROUP_RECEIVED, 17)

typedef enum {
#define X(name, value) name = value,
  FAITH_EVENT_TYPES(X)
#undef X
} faith_event_type_t;

typedef struct faith_client faith_client_t;

/* Return FAITH_OK only after an idempotent durable commit. The callback runs
 * on the reader thread. Key stored events by (auth_id, device_id, sequence).
 * A repeated sequence must not repeat application side effects.
 * GROUP_PACKET data is sender_auth_id[16] followed by faith_group encoded data.
 * Validate the sender/membership against local conversation state before
 * applying it. A safely rejected packet may be recorded as handled to unblock
 * the queue. GROUP_RECEIVED notifications carry the same body in chat_message;
 * value0 is the delivery sequence, value1 the kind, and value0_128 the
 * conversation ID. Notification memory belongs to faith_client_free_event(). */
typedef faith_status_code_t (*faith_client_store_event_fn)(
    void *context, const faith_auth_id_t *auth_id,
    const faith_device_id_t *device_id, uint64_t sequence, uint32_t type,
    const uint8_t *data, size_t size);

typedef struct {

  const char *host;
  const char *identity_path;
  uint16_t    port;

  const char *server_name;
  const char *ca_file;
  int         insecure_skip_verify;

  uint16_t                    proto_ver;
  faith_client_store_event_fn store_event;
  void                       *store_event_context;

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

/* Without store_event, events are delivered to the application without ACK.
 * Call this only after durably applying every event through sequence. For
 * CONVERSATION_READY, value0 is sequence and value0_128 is conversation ID. */
faith_status_code_t faith_client_ack_events(faith_client_t *client,
                                            uint64_t        sequence);

/* retrieve new client identity. populates auth_id and device_id */
faith_status_code_t faith_client_identity(faith_client_t    *client,
                                          faith_auth_id_t   *auth,
                                          faith_device_id_t *device);

/* deliver a group package */
faith_status_code_t
faith_client_deliver_group(faith_client_t             *client,
                           const faith_group_packet_t *packet,
                           const faith_auth_id_t *targets, size_t n_targets);
