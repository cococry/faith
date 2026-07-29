#pragma once

#include <faith-proto/codec/codec.h>
#include <faith-proto/codec/msg.h>
#include <faith-proto/core/core.h>

#include "server.h"

faith_status_code_t server_queue_frame(server_state_t *s, client_conn_t *cl,
                                       const faith_frame_t *frame);

faith_status_code_t server_queue_envelope(server_state_t *s, client_conn_t *cl,
                                          const faith_envelope_t *envl);

faith_status_code_t
server_queue_envelope_or_mark_dead(server_state_t *s, client_conn_t *cl,
                                   const faith_envelope_t *envl);

faith_status_code_t server_flush_client_output(server_state_t       *s,
                                               struct client_conn_t *cl);

faith_status_code_t server_drive_tls_handshake(server_state_t *s,
                                               client_conn_t  *cl);

faith_status_code_t server_drive_client_read(server_state_t *s,
                                             client_conn_t  *cl);
