#pragma once

#include <stdint.h>

#include "../shared.h"

typedef struct faith_client faith_client_t;

typedef struct {

  const char *host;
  uint16_t    port;

  const char *server_name;
  const char *ca_file;
  int         insecure_skip_verify;

  uint16_t proto_ver;

} faith_client_config_t;

typedef struct {
  uint32_t type;
  uint64_t value0;
  uint64_t value1;
  char     message[256];

  char  *chat_message;
  size_t chat_message_size;
} faith_event_t;

faith_status_code_t faith_client_init_global(int log_enable_tracing);

faith_client_t *faith_client_create(const faith_client_config_t *cfg);
void            faith_client_destroy(faith_client_t *client);

faith_status_code_t faith_client_start(faith_client_t *client);
faith_status_code_t faith_client_stop(faith_client_t *client);

faith_status_code_t
faith_client_send_message(faith_client_t   *client,
                          faith_client_id_t recipient_auth_id, const char *msg);

int faith_client_event_fd(faith_client_t *client);

faith_status_code_t faith_client_next_event(faith_client_t *client,
                                            faith_event_t  *out);
faith_status_code_t faith_client_free_event(faith_event_t *ev);
