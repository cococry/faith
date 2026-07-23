#pragma once

#include "../core/core.h"
#include "envelopes.h"

#include "server.h"

faith_status_code_t server_client_adopt_fd(server_state_t *s, int client_fd);

faith_status_code_t server_close_client(server_state_t *s,
                                        client_conn_t **cl_ptr);

faith_status_code_t
server_client_queue_disconnect(server_state_t *s, client_conn_t *cl,
                               faith_client_disconnect_reason_t reason,
                               faith_client_reconnect_policy_t reconnect_policy,
                               uint64_t                        retry_after_ms,
                               uint64_t banned_until_ms, const char *message);
