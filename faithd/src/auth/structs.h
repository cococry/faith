#pragma once

#include <stdbool.h>
#include <stdint.h>

#include "../core/core.h"
#include "../core/crypto.h"

#define _FH_FOR_EACH_AUTH_DEVICE(SERVER, AUTH_ID, RECIPIENT, STATUS_OUT, BODY) \
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

#define FAITH_AUTH_ID_SIZE   16
#define FAITH_DEVICE_ID_SIZE 16

typedef struct {
  uint8_t bytes[FAITH_AUTH_ID_SIZE];
} faith_auth_id_t;

typedef struct {
  uint8_t bytes[FAITH_DEVICE_ID_SIZE];
} faith_device_id_t;

typedef struct {
  uint64_t          nonce;
  uint64_t          server_nonce;
  uint8_t           public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  faith_auth_id_t   sender_auth_id;
  faith_device_id_t device_id;
} client_auth_handshake_params_t;

bool faith_client_id_equal(faith_auth_id_t a, faith_auth_id_t b);
bool faith_device_id_equal(faith_device_id_t a, faith_device_id_t b);
