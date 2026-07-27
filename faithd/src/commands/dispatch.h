#pragma once

#include "../core/core.h"

#include "../codec/commands.h"

#include "../server/server.h"

faith_status_code_t command_dispatch(server_state_t *s, client_conn_t *cl,
                                     const faith_envelope_t *envl);
