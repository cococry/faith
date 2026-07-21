#pragma once

#include "../protocol.h"
#include "../reactor/reactor.h"
#include "../transport/conn.h"

typedef struct {
  uint64_t          nonce;
  uint64_t          server_nonce;
  uint8_t           public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  faith_client_id_t sender_auth_id;
  faith_device_id_t device_id;
} client_temporary_handshake_params_t;

#define CLIENT_STATES(X)                                                       \
  X(CLIENT_HANDSHAKE, 0)                                                       \
  X(CLIENT_OPEN, 1)                                                            \
  X(CLIENT_CLOSING, 2)                                                         \
  X(CLIENT_WAIT_FOR_HELLO, 3)                                                  \
  X(CLIENT_WAIT_FOR_CHALLENGE_RESPONSE, 4)                                     \
  X(CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE, 5)

enum client_state_t {
#define X(name, value) name = value,
  CLIENT_STATES(X)
#undef X
};

typedef struct {
  uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
} client_identity_t;

struct client_conn_t {
  // connection id
  faith_client_id_t auth_id;
  faith_device_id_t device_id;

  transport_conn_t conn;
  reactor_source_t reactor_source;

  enum client_state_t state;

  uint32_t ev_mask;

  struct client_conn_t *next;
  struct client_conn_t *prev;

  client_temporary_handshake_params_t temp_handshake_params;
  faith_envl_stc_device_link_req_t   *pending_device_link_req;
  struct client_conn_t               *pending_device_link_conn;

  int authorized;

  int closing;
  int close_after_flush;
};

typedef struct client_conn_t client_conn_t;
