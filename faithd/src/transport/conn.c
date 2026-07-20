#include "conn.h"
#include <limits.h>
#include <openssl/ssl.h>

#define _MODULE_NAME "[transport/conn] "

conn_read_result_t conn_read_more_ssl_bytes(transport_conn_t *conn,
                                            bool              verbose_logging) {

  if (!conn)
    return CONN_READ_ERROR;

  uint8_t tmp[4096];

  int nread = 0;
  int err = tls_read(&conn->tls, tmp, sizeof(tmp), &nread);
  if (err == INT_MAX) {
    return CONN_READ_ERROR;
  }

  if (nread > 0) {
    faith_status_code_t rc =
        conn_enqueue_bytes(&conn->in, tmp, (size_t)nread, verbose_logging);

    if (rc != FAITH_OK)
      return CONN_READ_ERROR;

    return CONN_READ_GOT_BYTES;
  }

  if (err == SSL_ERROR_WANT_READ)
    return CONN_READ_WANT_READ;

  if (err == SSL_ERROR_WANT_WRITE)
    return CONN_READ_WANT_WRITE;

  if (err == SSL_ERROR_ZERO_RETURN)
    return CONN_READ_CLOSED;

  return CONN_READ_ERROR;
}

faith_status_code_t conn_enqueue_bytes(transport_queue_t *queue,
                                       const uint8_t *bytes, size_t n_bytes,
                                       bool verbose_logging) {
  if ((!bytes && n_bytes != 0))
    return FAITH_ERR_INVALID;

  if (verbose_logging) {
    nob_log(INFO, _MODULE_NAME "Trying to enqueue %zu input bytes...", n_bytes);
  }

  if (n_bytes == 0) {
    if (verbose_logging) {
      nob_log(WARNING, _MODULE_NAME "Tried to enqueue zero-length input");
    }

    return FAITH_OK;
  }

  if (queue->size > FAITH_MAX_CLIENT_IN_QUEUE ||
      n_bytes > FAITH_MAX_CLIENT_IN_QUEUE - queue->size)
    return FAITH_ERR_OVERFLOW;

  const size_t needed = queue->size + n_bytes;

  if (needed > queue->cap) {
    size_t new_cap = queue->cap ? queue->cap : 4096u;

    while (new_cap < needed) {
      if (new_cap >= FAITH_MAX_CLIENT_IN_QUEUE) {
        new_cap = FAITH_MAX_CLIENT_IN_QUEUE;
        break;
      }

      if (new_cap > SIZE_MAX / 2) {
        new_cap = needed;
        break;
      }

      new_cap *= 2;

      if (new_cap > FAITH_MAX_CLIENT_IN_QUEUE)
        new_cap = FAITH_MAX_CLIENT_IN_QUEUE;
    }

    if (new_cap < needed)
      return FAITH_ERR_OVERFLOW;

    uint8_t *p = realloc(queue->buf, new_cap);
    if (!p)
      return FAITH_ERR_NOMEM;

    queue->buf = p;
    queue->cap = new_cap;
  }

  memcpy(queue->buf + queue->size, bytes, n_bytes);
  queue->size = needed;

  if (verbose_logging) {
    nob_log(INFO, _MODULE_NAME "Successfully enqueued %zu input bytes",
            n_bytes);
  }

  return FAITH_OK;
}
