#pragma once

#include "../auth/structs.h"
#include "../core/core.h"

typedef struct {
  uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
} client_identity_t;

struct client_conn_t;

typedef struct {
  struct client_conn_t *conn;
  client_identity_t     ident;
} client_device_session_data_t;

typedef struct {
  faith_device_id_t             key; /* device id */
  client_device_session_data_t *value;
} client_session_device_t;

/* nested hashmap [auth id -> device id -> conn/sess data] */
typedef struct {
  faith_client_id_t key; /* client/auth id */
  client_session_device_t
      *value; /* a hashmap of device id -> client conn/sess data*/
} client_session_user_t;

typedef struct {
  client_session_user_t *active_users;
} sess_registry_state_t;

faith_status_code_t
sess_registry_get_user_from_auth_id(sess_registry_state_t   *rt,
                                    const faith_client_id_t *auth_id,
                                    client_session_user_t  **o_user);

bool sess_registry_auth_id_registered(sess_registry_state_t   *rt,
                                      const faith_client_id_t *auth_id);

faith_status_code_t
sess_registry_get_devices(sess_registry_state_t    *rt,
                          const faith_client_id_t  *auth_id,
                          client_session_device_t **o_devmap);

faith_status_code_t sess_registry_register_session(
    sess_registry_state_t *rt, const faith_client_id_t *auth_id,
    const faith_device_id_t *device_id, struct client_conn_t *cl,
    const uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]);

faith_status_code_t sess_registry_get_session(
    sess_registry_state_t *rt, const faith_client_id_t *auth_id,
    const faith_device_id_t *device_id, client_device_session_data_t **o_sess);

faith_status_code_t
sess_registry_unregister_session(sess_registry_state_t   *rt,
                                 const faith_client_id_t *auth_id,
                                 const faith_device_id_t *device_id);

faith_status_code_t sess_registry_destroy(sess_registry_state_t *rt);
