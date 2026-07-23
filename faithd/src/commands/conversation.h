#pragma once

#include "../core/core.h"

#include "../codec/commands.h"

#include "../server/server.h"

faith_status_code_t
conv_handle_create_conversation(server_state_t *s, client_conn_t *cl,
                                const faith_envl_cts_command_t *cmd);
