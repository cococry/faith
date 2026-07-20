#pragma once


#include "tls.h"
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

typedef enum {
  CONN_READ_OK,
  CONN_READ_GOT_BYTES,
  CONN_READ_WANT_READ,
  CONN_READ_WANT_WRITE,
  CONN_READ_CLOSED,
  CONN_READ_ERROR
} conn_read_result_t;


typedef struct {
  uint8_t *buf;
  size_t   size;
  size_t   cap;
  size_t   off;
} transport_queue_t;

typedef struct {
  uint64_t conn_id;
  int fd;

  transport_queue_t in; 
  transport_queue_t out;

  tls_state_fd_t tls;
} transport_conn_t;

static int conn_drive_client_read(struct server_state_t     *s,
                                  struct client_conn_t      *cl,
                                  const struct server_cfg_t *cfg);

conn_read_result_t conn_read_more_ssl_bytes(transport_conn_t *conn,
                                            bool              verbose_logging);

faith_status_code_t conn_enqueue_bytes(transport_queue_t *queue,
                                       const uint8_t *bytes, size_t n_bytes,
                                       bool verbose_logging);


