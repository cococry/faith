#pragma once

#include <faith-proto/codec/auth.h>
#include <faith-proto/codec/envelopes.h>

#include "../server/server.h"

#define FAITH_DEVICE_LINK_REQ_EXPIRATION_TIME_MS 1000 * 60 /* 60 seconds */

faith_status_code_t
device_link_handle_device_response(server_state_t *s, client_conn_t *cl,
                                   const faith_envelope_t *response_envl);

faith_status_code_t
device_link_new_device(server_state_t *s, client_conn_t *cl,
                       const client_auth_handshake_params_t *params);

faith_status_code_t
device_link_queue_request_cancellation(server_state_t *s,
                                       client_conn_t  *requesting_cl);

faith_status_code_t device_link_queue_request(
    server_state_t *s, client_conn_t *recipient_cl, client_conn_t *request_cl,
    const faith_auth_id_t *auth_id,
    const uint8_t          public_key_new_device[FAITH_ED25519_PUBLIC_KEY_SIZE],
    const faith_device_id_t           *new_device_id,
    faith_envl_stc_device_link_req_t **o_req);

void device_link_remove_request(struct client_conn_t *cl);
