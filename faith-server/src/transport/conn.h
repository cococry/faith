#pragma once

#include <faith-proto/core/core.h>

#include "tls.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define FAITH_MAX_CLIENT_OUT_QUEUE (1024u * 1024u)
#define FAITH_MAX_CLIENT_IN_QUEUE  (1024u * 1024u)

typedef enum {
  TRANSPORT_RES_COMPLETE,
  TRANSPORT_RES_GOT_BYTES,
  TRANSPORT_RES_WANT_READ,
  TRANSPORT_RES_WANT_WRITE,
  TRANSPORT_RES_CLOSED,
  TRANSPORT_RES_ERROR
} transport_result_t;

typedef enum {
  TRANSPORT_QUEUE_INPUT,
  TRANSPORT_QUEUE_OUTPUT,
} transport_queue_type_t;

typedef struct {
  uint8_t *buf;
  size_t   size;
  size_t   cap;
  size_t   off;

  transport_queue_type_t type;
} transport_queue_t;

typedef struct {
  uint64_t id;
  int      fd;

  transport_queue_t in;
  transport_queue_t out;

  tls_state_fd_t tls;
} transport_conn_t;

faith_status_code_t conn_init(transport_conn_t *conn);

transport_result_t conn_read_more_ssl_bytes(transport_conn_t *conn);

faith_status_code_t conn_flush_output(transport_conn_t   *conn,
                                      transport_result_t *o_res);

bool conn_output_empty(transport_conn_t *conn);

faith_status_code_t conn_queue_enqueue_bytes(transport_queue_t *queue,
                                             const uint8_t     *bytes,
                                             size_t             n_bytes);

faith_status_code_t conn_queue_free(transport_queue_t *queue);
