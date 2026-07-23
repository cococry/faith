#pragma once

#include "../auth/envelopes.h"
#include "../reactor/reactor.h"
#include "../transport/conn.h"
#include "sess_registry.h"

#include "../auth/structs.h"

#include <stdatomic.h>

#define CLIENT_STATES(X)                                                       \
  X(CLIENT_HANDSHAKE, 0)                                                       \
  X(CLIENT_OPEN, 1)                                                            \
  X(CLIENT_CLOSING, 2)                                                         \
  X(CLIENT_WAIT_FOR_HELLO, 3)                                                  \
  X(CLIENT_WAIT_FOR_CHALLENGE_RESPONSE, 4)                                     \
  X(CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE, 5)

typedef enum {
#define X(name, value) name = value,
  CLIENT_STATES(X)
#undef X
} client_state_t;

typedef struct client_conn_t {
  // connection id
  faith_auth_id_t auth_id;
  faith_device_id_t device_id;

  transport_conn_t conn;
  reactor_source_t reactor_source;

  client_state_t state;

  uint32_t ev_mask;

  struct client_conn_t *next;
  struct client_conn_t *prev;

  client_auth_handshake_params_t    temp_handshake_params;
  faith_envl_stc_device_link_req_t *pending_device_link_req;
  struct client_conn_t             *pending_device_link_conn;

  int authorized;

  int closing;
  int close_after_flush;
} client_conn_t;

typedef struct {
  int listen_port;
} server_config_t;

typedef struct {
  reactor_source_t  listen_source;
  reactor_context_t reactor;

  tls_context_t tls;

  atomic_uint_fast64_t  next_client_id;
  struct client_conn_t *clients;

  sess_registry_state_t rt;

  server_config_t cfg;
} server_state_t;

faith_status_code_t server_init(server_state_t *s, const server_config_t *cfg);

faith_status_code_t server_loop(server_state_t *s);

faith_status_code_t server_destroy(server_state_t *s);

void server_link_client(server_state_t *s, client_conn_t *cl);

void server_unlink_client(server_state_t *s, client_conn_t *cl);

void server_set_client_state(server_state_t *s, struct client_conn_t *cl,
                             client_state_t state);
