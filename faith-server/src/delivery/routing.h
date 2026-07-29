#pragma once

#include <faith-proto/core/core.h>
#include <faith-proto/codec/envelopes.h>

#include "../server/server.h"

faith_status_code_t
delivery_route_envelope_to_auth_id(server_state_t *s, client_conn_t *cl_sender,
                                   const faith_auth_id_t  *recipient_auth_id,
                                   const faith_envelope_t *envl);

faith_status_code_t
delivery_route_application_msg_envelope(server_state_t *s, client_conn_t *cl,
                                        faith_envelope_t *envl);
