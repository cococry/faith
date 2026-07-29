#include "sess_registry.h"

#include "server.h"

#define STB_DS_IMPLEMENTATION
#include "../../third_party/stb_ds.h"

#define _MODULE_NAME "server/sess_registry"

faith_status_code_t
sess_registry_get_user_from_auth_id(sess_registry_state_t  *rt,
                                    const faith_auth_id_t  *auth_id,
                                    client_session_user_t **o_user) {
  if (!rt || !auth_id || !o_user)
    return FAITH_ERR_INVALID;

  client_session_user_t *user = hmgetp_null(rt->active_users, *auth_id);
  *o_user = user;

  return FAITH_OK;
}

bool sess_registry_auth_id_registered(sess_registry_state_t *rt,
                                      const faith_auth_id_t *auth_id) {
  client_session_user_t *user = NULL;
  _FH_CHECK(sess_registry_get_user_from_auth_id(rt, auth_id, &user));
  if (_fh_rc != FAITH_OK) {
    return 0;
  }
  return user != NULL;
}

faith_status_code_t
sess_registry_get_devices(sess_registry_state_t    *rt,
                          const faith_auth_id_t    *auth_id,
                          client_session_device_t **o_devmap) {
  if (!rt || !o_devmap)
    return FAITH_ERR_INVALID;

  *o_devmap = NULL;

  client_session_user_t *user = NULL;
  _FH_CHECK_RETURN(sess_registry_get_user_from_auth_id(rt, auth_id, &user));
  if (!user)
    return FAITH_ERR_NOT_FOUND;

  *o_devmap = user->value;

  return FAITH_OK;
}

faith_status_code_t sess_registry_register_session(
    sess_registry_state_t *rt, const faith_auth_id_t *auth_id,
    const faith_device_id_t *device_id, client_conn_t *cl,
    const uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]) {
  if (!rt || !auth_id || !device_id || !public_key)
    return FAITH_ERR_INVALID;

  /* hmget() inserts <auth_id> if not found */
  client_session_device_t *devmap = hmget(rt->active_users, *auth_id);

  /* allocate new session data */
  client_device_session_data_t *sess = calloc(1, sizeof(*sess));
  if (!sess) {
    return FAITH_ERR_NOMEM;
  }

  sess->conn = cl;

  /* init event inbox for this device */
  device_event_inbox_init(&sess->inbox);

  /* assign public key to new session */
  sess->ident.auth_id = cl->ident.auth_id;
  sess->ident.device_id = cl->ident.device_id;
  memcpy(sess->ident.public_key, public_key, FAITH_ED25519_PUBLIC_KEY_SIZE);

  /* insert session data at <auth_id, device_id pair> */
  hmput(devmap, *device_id, sess);

  /* write pointer back to avoid stale pointers */
  hmput(rt->active_users, *auth_id, devmap);

  char auth_id_hex[33];
  char device_id_hex[33];

  faith_status_code_t _fh_result = FAITH_OK;
  _FH_CHECK_DEFER(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
  _FH_CHECK_DEFER(faith_id128_to_hex(device_id->bytes, device_id_hex));

  nob_log(INFO,
          "[%s] Registered session with auth_id=%s, device_id=%s (online "
          "clients: %zu)",
          _MODULE_NAME, auth_id_hex, device_id_hex, hmlen(rt->active_users));

  return FAITH_OK;
defer:
  free(sess);
  return _fh_result;
}

faith_status_code_t sess_registry_get_session(
    sess_registry_state_t *rt, const faith_auth_id_t *auth_id,
    const faith_device_id_t *device_id, client_device_session_data_t **o_sess) {
  if (!rt || !o_sess || !auth_id || !device_id)
    return FAITH_ERR_INVALID;

  *o_sess = NULL;

  client_session_user_t *user = NULL;
  _FH_CHECK_RETURN(sess_registry_get_user_from_auth_id(rt, auth_id, &user));
  if (!user)
    return FAITH_ERR_NOT_FOUND;

  client_session_device_t *devmap = user->value;

  client_session_device_t *dev = hmgetp_null(devmap, *device_id);

  // Session is registered, but exact device not registered yet, *o_sess will be
  // NULL to indicate this state.
  if (!dev)
    return FAITH_OK;

  *o_sess = dev->value;

  return FAITH_OK;
}

faith_status_code_t
sess_registry_unregister_session(sess_registry_state_t   *rt,
                                 const faith_auth_id_t   *auth_id,
                                 const faith_device_id_t *device_id) {
  if (!rt || !auth_id || !device_id)
    return FAITH_ERR_INVALID;

  client_session_user_t *user = NULL;
  _FH_CHECK_RETURN(sess_registry_get_user_from_auth_id(rt, auth_id, &user));
  if (!user)
    return FAITH_ERR_NOT_FOUND;

  client_session_device_t *devmap = user->value;

  client_session_device_t *dev = hmgetp_null(devmap, *device_id);
  if (!dev)
    return FAITH_ERR_NOT_FOUND;

  /* <dev->value> is a dynamically allocated client_device_session_data_t
   */
  free(dev->value);
  dev->value = NULL;

  (void)hmdel(devmap, *device_id);

  if (hmlen(devmap) == 0) {
    hmfree(devmap);
    (void)hmdel(rt->active_users, *auth_id);
  } else {
    /* write pointer back to avoid stale pointers */
    hmput(rt->active_users, *auth_id, devmap);
  }

  char auth_id_hex[33];
  char device_id_hex[33];

  _FH_CHECK_RETURN(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(device_id->bytes, device_id_hex));

  nob_log(INFO,
          "[%s] Unregistered session with auth_id=%s, device_id=%s (online "
          "clients: "
          "%zu)",
          _MODULE_NAME, auth_id_hex, device_id_hex, hmlen(rt->active_users));

  return FAITH_OK;
}

faith_status_code_t sess_registry_destroy(sess_registry_state_t *rt) {
  if (!rt)
    return FAITH_ERR_INVALID;

  ptrdiff_t user_count = hmlen(rt->active_users);

  for (ptrdiff_t i = 0; i < user_count; ++i) {
    client_session_device_t *devices = rt->active_users[i].value;
    ptrdiff_t                device_count = hmlen(devices);

    for (ptrdiff_t j = 0; j < device_count; ++j) {
      free(devices[j].value);
      devices[j].value = NULL;
    }

    hmfree(devices);
    rt->active_users[i].value = NULL;
  }

  hmfree(rt->active_users);
  rt->active_users = NULL;

  return FAITH_OK;
}
