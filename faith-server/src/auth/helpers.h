#pragma once

#include <stdbool.h>
#include <stdint.h>

#include <faith-proto/core/core.h>
#include <faith-proto/core/crypto.h>

#include <faith-proto/codec/auth.h>

typedef struct {
  uint64_t          nonce;
  uint64_t          server_nonce;
  uint8_t           public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  faith_auth_id_t   sender_auth_id;
  faith_device_id_t device_id;
} client_auth_handshake_params_t;

#define _FH_FOR_EACH_AUTH_DEVICE_CONNECTION(SERVER, AUTH_ID, RECIPIENT,        \
                                            STATUS_OUT, BODY)                  \
  do {                                                                         \
    server_state_t          *_fh_iter_server = (SERVER);                       \
    const faith_auth_id_t   *_fh_iter_auth_id = (AUTH_ID);                     \
    client_session_device_t *_fh_iter_devices = NULL;                          \
                                                                               \
    (STATUS_OUT) = sess_registry_get_devices(                                  \
        &_fh_iter_server->rt, _fh_iter_auth_id, &_fh_iter_devices);            \
                                                                               \
    if ((STATUS_OUT) == FAITH_ERR_NOT_FOUND) {                                 \
      (STATUS_OUT) = FAITH_OK;                                                 \
    } else if ((STATUS_OUT) == FAITH_OK) {                                     \
      ptrdiff_t _fh_iter_count = hmlen(_fh_iter_devices);                      \
                                                                               \
      for (ptrdiff_t _fh_iter_i = 0; _fh_iter_i < _fh_iter_count;              \
           ++_fh_iter_i) {                                                     \
        if (!_fh_iter_devices[_fh_iter_i].value ||                             \
            !_fh_iter_devices[_fh_iter_i].value->conn)                         \
          continue;                                                            \
                                                                               \
        struct client_conn_t *(RECIPIENT) =                                    \
            _fh_iter_devices[_fh_iter_i].value->conn;                          \
                                                                               \
        if ((RECIPIENT)->closing || !(RECIPIENT)->authorized ||                \
            (RECIPIENT)->state != CLIENT_OPEN)                                 \
          continue;                                                            \
                                                                               \
        BODY                                                                   \
      }                                                                        \
    }                                                                          \
  } while (0)

#define _FH_FOR_EACH_AUTH_DEVICE_SESSION(SERVER, AUTH_ID, DEVICE_SESSION,      \
                                         STATUS_OUT, BODY)                     \
  do {                                                                         \
    server_state_t          *_fh_iter_server = (SERVER);                       \
    const faith_auth_id_t   *_fh_iter_auth_id = (AUTH_ID);                     \
    client_session_device_t *_fh_iter_devices = NULL;                          \
                                                                               \
    if (!_fh_iter_server || !_fh_iter_auth_id) {                               \
      (STATUS_OUT) = FAITH_ERR_INVALID;                                        \
      break;                                                                   \
    }                                                                          \
                                                                               \
    (STATUS_OUT) = sess_registry_get_devices(                                  \
        &_fh_iter_server->rt, _fh_iter_auth_id, &_fh_iter_devices);            \
                                                                               \
    if ((STATUS_OUT) == FAITH_ERR_NOT_FOUND) {                                 \
      (STATUS_OUT) = FAITH_OK;                                                 \
    } else if ((STATUS_OUT) == FAITH_OK) {                                     \
      ptrdiff_t _fh_iter_count = hmlen(_fh_iter_devices);                      \
                                                                               \
      for (ptrdiff_t _fh_iter_i = 0; _fh_iter_i < _fh_iter_count;              \
           ++_fh_iter_i) {                                                     \
        if (!_fh_iter_devices[_fh_iter_i].value)                               \
          continue;                                                            \
                                                                               \
        client_device_session_data_t *(DEVICE_SESSION) =                       \
            _fh_iter_devices[_fh_iter_i].value;                                \
                                                                               \
        BODY                                                                   \
      }                                                                        \
    }                                                                          \
  } while (0)

