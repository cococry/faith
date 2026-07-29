#pragma once

#include <faith-proto/core/core.h>

#include <faith-proto/codec/commands.h>
#include <faith-proto/codec/envelopes.h>

#include "../server/server.h"

faith_status_code_t command_dispatch(server_state_t *s, client_conn_t *cl,
                                     const faith_envelope_t *envl);
