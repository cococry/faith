#include "conn.h"
#include <limits.h>
#include <openssl/ssl.h>
#include <sys/epoll.h>

#define _MODULE_NAME "[transport/conn] "

faith_status_code_t conn_init(transport_conn_t *conn) {
  if (!conn)
    return FAITH_ERR_INVALID;

  conn->in.type = TRANSPORT_QUEUE_INPUT;
  conn->out.type = TRANSPORT_QUEUE_OUTPUT;

  return FAITH_OK;
}
transport_result_t conn_read_more_ssl_bytes(transport_conn_t *conn,
                                            bool              verbose_logging) {

  if (!conn)
    return TRANSPORT_RES_ERROR;

  uint8_t tmp[4096];

  int nread = 0;
  int err = tls_read(&conn->tls, tmp, sizeof(tmp), &nread);
  if (err == INT_MAX) {
    nob_log(ERROR, _MODULE_NAME "Invalid arguments specified for tls_read()");
    return TRANSPORT_RES_ERROR;
  }

  if (nread > 0) {
    _FH_CHECK(conn_queue_enqueue_bytes(&conn->in, tmp, (size_t)nread,
                                       verbose_logging));

    if (_fh_rc != FAITH_OK) 
      return TRANSPORT_RES_ERROR;

    return TRANSPORT_RES_GOT_BYTES;
  }

  if (err == SSL_ERROR_WANT_READ)
    return TRANSPORT_RES_WANT_READ;

  if (err == SSL_ERROR_WANT_WRITE)
    return TRANSPORT_RES_WANT_WRITE;

  if (err == SSL_ERROR_ZERO_RETURN)
    return TRANSPORT_RES_CLOSED;

  nob_log(ERROR, "tls_read() failed. SSL error: %i", err);
  ERR_print_errors_fp(stderr);

  return TRANSPORT_RES_ERROR;
}

faith_status_code_t conn_flush_output(transport_conn_t   *conn,
                                      transport_result_t *o_res) {
  if (!conn || !o_res || (!conn->out.buf && conn->out.size > 0))
    return FAITH_ERR_INVALID;

  *o_res = TRANSPORT_RES_ERROR;

  if (!conn->out.buf) {
    nob_log(WARNING,
            "[client=%" PRIu64 " fd=%i] Requested to flush output but output "
                               "queue buffer is not allocated.",
            conn->id, conn->fd);
    return FAITH_OK;
  }

  while (conn->out.off < conn->out.size) {
    size_t remaining = conn->out.size - conn->out.off;
    /* becaused this is passed as int to SSL, we need to clamp to integer
     * range */
    int clamped_write = remaining > INT_MAX ? INT_MAX : (int)remaining;

    int nwrite = 0;
    int err = tls_write(&conn->tls, conn->out.buf + conn->out.off,
                        clamped_write, &nwrite);

    if (err == INT_MAX) {
      nob_log(ERROR,
              "[client=%" PRIu64 " fd=%i] failed to write %i bytes over the "
              "wire. Invalid arguments specified.",
              conn->id, conn->fd, nwrite);
      /* Fatal argument error => return immediately */
      return FAITH_ERR_INVALID;
    }

    nob_log(INFO, "[client=%" PRIu64 " fd=%i] wrote %i bytes over the wire.",
            conn->id, conn->fd, nwrite);

    if (nwrite > 0) {
      conn->out.off += (size_t)nwrite;
      continue;
    }

    if (err == SSL_ERROR_WANT_WRITE || err == SSL_ERROR_WANT_READ) {
      *o_res = (err == SSL_ERROR_WANT_WRITE) ? TRANSPORT_RES_WANT_WRITE
                                             : TRANSPORT_RES_WANT_READ;
      return FAITH_OK;
    }

    switch (err) {
    case SSL_ERROR_ZERO_RETURN:
      *o_res = TRANSPORT_RES_CLOSED;
      return FAITH_OK;
    default:
      *o_res = TRANSPORT_RES_ERROR;
      nob_log(ERROR, "tls_write() failed.");
      ERR_print_errors_fp(stderr);
      return FAITH_ERR_IO;
    }
  }

  _FH_CHECK_RETURN(conn_queue_free(&conn->out));

  *o_res = TRANSPORT_RES_COMPLETE;

  return FAITH_OK;
}

bool conn_output_empty(transport_conn_t *conn) {
  if (!conn)
    return false;

  int rc = tls_BIO_ctrl_pending(&conn->tls);
  if (rc == INT_MAX)
    return 0;

  return conn->out.size == 0 && rc == 0;
}

faith_status_code_t conn_queue_enqueue_bytes(transport_queue_t *queue,
                                       const uint8_t *bytes, size_t n_bytes,
                                       bool verbose_logging) {
  if ((!bytes && n_bytes != 0))
    return FAITH_ERR_INVALID;

  const char* type = queue->type == TRANSPORT_QUEUE_INPUT ? "input" : "output";

  if (verbose_logging) {
    nob_log(INFO, _MODULE_NAME "Trying to enqueue %zu %s bytes...", n_bytes, type);
  }

  if (n_bytes == 0) {
    if (verbose_logging) {
      nob_log(WARNING, _MODULE_NAME "Tried to enqueue zero-length %s", type);
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
    nob_log(INFO, _MODULE_NAME "Successfully enqueued %zu %s bytes",
            n_bytes, type);
  }

  return FAITH_OK;
}

faith_status_code_t conn_queue_free(transport_queue_t *queue) {
  if (queue == NULL)
    return FAITH_ERR_INVALID;

  free(queue->buf);
  memset(queue, 0, sizeof(*queue));

  return FAITH_OK;
}

