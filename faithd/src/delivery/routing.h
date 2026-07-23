#pragma once

#include "../core/core.h"

#include "../server/server.h"

faith_status_code_t
delivery_route_envelope_to_auth_id(server_state_t *s, client_conn_t *cl_sender,
                                   const faith_client_id_t *recipient_auth_id,
                                   const faith_envelope_t  *envl);
