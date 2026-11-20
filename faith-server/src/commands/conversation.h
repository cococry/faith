#pragma once

#include <faith-proto/codec/commands.h>
#include <faith-proto/core/core.h>

#include "../server/server.h"

faith_status_code_t
conv_handle_conversation_request(server_state_t *s, client_conn_t *cl,
                                const faith_envl_cts_command_t *cmd);

faith_status_code_t
conv_handle_conversation_response(server_state_t *s, client_conn_t *cl,
                                  const faith_envl_cts_command_t *cmd);

faith_status_code_t
conv_claim(faith_db_t *db, const uint8_t id[FAITH_CONVERSATION_ID_SIZE],
           const faith_auth_id_t *owner, const bool *force, bool disclose);

faith_status_code_t
conv_handle_group_delivery(server_state_t *s, client_conn_t *cl,
                           const faith_envl_cts_command_t *cmd);
