#include "frame.h"
#include "conn.h"

conn_read_result_t
frame_try_full_read(transport_conn_t* conn, faith_frame_t *frame,
                   bool verbose_logging) {
  while (1) {
    size_t consumed = 0;

    if (verbose_logging) {
      nob_log(INFO,
              "[client=%" PRIu64 " fd=%i] Trying to parse frame buffer...",
              conn->conn_id, conn->fd);
    }

    faith_status_code_t rc =
        frame_try_parse_from_buffer(conn->in.buf, conn->in.size, frame, &consumed);

    if (rc == FAITH_OK) {

      if (verbose_logging) {
        nob_log(
            INFO,
            "[client=%" PRIu64
            " fd=%i] Successfully parsed full frame from buffer (%li bytes)",
            conn->conn_id, conn->fd, consumed);
      }

      size_t remaining = conn->in.size - consumed;

      if (remaining > 0) {
        memmove(conn->in.buf, conn->in.buf + consumed, remaining);
      }

      conn->in.size -= consumed;

      return CONN_READ_OK;
    }

    if (rc == FAITH_ERR_INCOMPLETE) {
      /* Not enough bytes yet, so read more decrypted TLS data. */
      if (verbose_logging) {
        nob_log(INFO,
                "[client=%" PRIu64
                " fd=%i] Frame incomplete, reading more bytes...",
                conn->conn_id, conn->fd);
      }
      conn_read_result_t rr = conn_read_more_ssl_bytes(conn, verbose_logging);

      if (rr == CONN_READ_GOT_BYTES) {
        if (verbose_logging) {
          nob_log(INFO,
                  "[client=%" PRIu64
                  " fd=%i] Got new bytes, parsing frame again...",
                  conn->conn_id, conn->fd);
        }
        continue;
      }

      if (rr == CONN_READ_WANT_READ) {
        if (verbose_logging) {
          nob_log(INFO,
                  "[client=%" PRIu64
                  " fd=%i] SSL_read needs to wait for socket to be readable",
                  conn->conn_id, conn->fd);
        }
        return rr;
      }

      if (rr == CONN_READ_WANT_WRITE) {
        if (verbose_logging) {
          nob_log(INFO,
                  "[client=%" PRIu64
                  " fd=%i] SSL_read needs to wait for socket to be writable",
                  conn->conn_id, conn->fd);
        }
        return rr;
      }

      if (rr == CONN_READ_CLOSED) {
        nob_log(INFO,
                "[client=%" PRIu64
                " fd=%i] Connection closed while reading incomplete frame.",
                conn->conn_id, conn->fd);
        return rr;
      }

      nob_log(ERROR,
              "[client=%" PRIu64
              " fd=%i] Error while reading incomplete frame. Error=%i",
              conn->conn_id, conn->fd, rr);

      return rr;
    }

    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Failed to read frame",
            conn->conn_id, conn->fd);

    return CONN_READ_ERROR;
  }


}
faith_status_code_t frame_try_parse_from_buffer(const uint8_t *payload,
                                                       size_t payload_size,
                                                       faith_frame_t *out,
                                                       size_t *consumed_out) {
  if (!consumed_out || !out)
    return FAITH_ERR_INVALID;

  *consumed_out = 0;

  /* accepts and handles NULL <payload> and invalid/incomplete payload_size
   * writes <out> if successfully decoded a full frame */
  faith_status_code_t rc = faith_decode_frame(payload, payload_size, out);

  if (rc != FAITH_OK) {
    return rc;
  }

  *consumed_out = out->frame_size + FAITH_FRAME_LENGTH_SIZE;

  return FAITH_OK;
}
