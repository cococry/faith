#pragma once

#include "../protocol.h"
#include "server.h"

faith_status_code_t server_dispatch_frame(server_state_t *s, client_conn_t *cl,
                                          faith_frame_t *frame);

faith_status_code_t server_dispatch_envelope(server_state_t *s,
                                             client_conn_t  *cl,
                                             faith_frame_t  *frame);

faith_status_code_t server_handle_ping(server_state_t *s, client_conn_t *cl,
                                       faith_frame_t *frame);
