#pragma once

#include "../codec/protocol.h"
#include "server.h"

faith_status_code_t server_dispatch_frame(server_state_t *s, client_conn_t *cl,
                                          faith_frame_t *frame);
