#pragma once

#include "../protocol.h"

typedef struct {
  faith_client_id_t auth_id_sender;
  faith_client_id_t auth_id_receiver;
  uint64_t          created_at_ms;
  uint64_t          expires_at_ms;
} storage_msg_request_t;

/* hashmap [request ID -> server_msg_request_t*] */
typedef struct {
  faith_request_id_t     key;
  storage_msg_request_t *value;
} stored_msg_request_t;

typedef struct {
  stored_msg_request_t *msg_requests;
} storage_state_t;

faith_status_code_t
storage_store_msg_request(storage_state_t             *st,
                          const faith_request_id_t    *request_id,
                          const storage_msg_request_t *request);

faith_status_code_t
storage_remove_msg_request(storage_state_t          *st,
                           const faith_request_id_t *request_id);

faith_status_code_t
storage_get_msg_request(storage_state_t          *st,
                        const faith_request_id_t *request_id,
                        storage_msg_request_t   **out);
