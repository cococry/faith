#pragma once

#include "../protocol.h"

#include "../server/server.h"

faith_status_code_t auth_handle_hello(server_state_t *s, client_conn_t *cl,
                                      const faith_envelope_t *hello_envl);

faith_status_code_t
auth_handle_challenge_response(server_state_t *s, client_conn_t *cl,
                               const faith_envelope_t *challenge_response_envl);

faith_status_code_t auth_authorize_client(
    server_state_t *s, client_conn_t *cl, const faith_client_id_t *auth_id,
    const faith_device_id_t *device_id,
    uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE], int register_session);


faith_status_code_t
auth_handshake_complete(server_state_t *s, client_conn_t *cl,
                    const faith_client_id_t *sender_id,
                    const faith_device_id_t *device_id);

faith_status_code_t auth_queue_auth_pending(server_state_t *s,
                                            client_conn_t  *recipient_cl);
