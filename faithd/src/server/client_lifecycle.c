#include "client_lifecycle.h"

#include "../auth/device_link.h"

#include "client_io.h"

#include "../../third_party/stb_ds.h"

#define _MODULE_NAME "server/client_lifecycle"

faith_status_code_t server_client_adopt_fd(server_state_t *s, int client_fd) {

  struct client_conn_t *cl = calloc(1, sizeof(*cl));
  if (!cl) {
    nob_log(WARNING, "failed to allocate memory with calloc() for pending "
                     "client connection.");
    close(client_fd);
    return FAITH_ERR_NOMEM;
  }

  faith_status_code_t _fh_result = FAITH_OK;

  /* Add client to linked list of clients (server_close_client calls
   * server_unlink_client) in defer. */
  server_link_client(s, cl);

  _FH_CHECK_DEFER(conn_init(&cl->conn));

  cl->conn.id = atomic_fetch_add(&s->next_client_id, 1);
  cl->conn.fd = client_fd;

  cl->reactor_source = (reactor_source_t){.fd = client_fd,
                                          .interests = REACTOR_READABLE,
                                          .user_data = cl,
                                          .type = REACTOR_SOURCE_CLIENT};

  _FH_CHECK_DEFER(tls_new_with_fd(&s->tls, cl->conn.fd, &cl->conn.tls));

  _FH_CHECK_DEFER(reactor_add(&s->reactor, &cl->reactor_source));

  server_set_client_state(s, cl, CLIENT_HANDSHAKE);

  nob_log(INFO, "accepted new client id=%" PRIu64 " fd=%i", cl->conn.id,
          client_fd);

  return FAITH_OK;
defer:
  _FH_CHECK_SCOPED(server_close_client(s, &cl));
  return _fh_result;
}

faith_status_code_t server_close_client(server_state_t *s,
                                        client_conn_t **cl_ptr) {
  if (!s || !cl_ptr || !*cl_ptr)
    return FAITH_ERR_INVALID;

  struct client_conn_t *cl = *cl_ptr;

  faith_status_code_t result = FAITH_OK;
  if (cl->state == CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE) {
    faith_status_code_t rc = device_link_queue_request_cancellation(s, cl);

    if (rc != FAITH_OK) {
      nob_log(ERROR,
              "[%s: client=%" PRIu64
              " fd=%d] Failed to cancel pending device-link request: %s",
              _MODULE_NAME, cl->conn.id, cl->conn.fd,
              faith_status_code_name(rc));

      if (result == FAITH_OK)
        result = rc;
    }
  }

  *cl_ptr = NULL;

  server_unlink_client(s, cl);

  int device_link_req_pending =
      cl->authorized && (cl->pending_device_link_conn != NULL &&
                         cl->pending_device_link_req != NULL);

  if (device_link_req_pending) {

    client_session_device_t *devices = NULL;
    faith_status_code_t      rc =
        sess_registry_get_devices(&s->rt, &cl->ident.auth_id, &devices);
    if (rc != FAITH_OK || devices == NULL) {
      nob_log(ERROR,
              "[%s: client=%" PRIu64
              " fd=%d] Failed to enumerate account devices: %s",
              _MODULE_NAME, cl->conn.id, cl->conn.fd,
              faith_status_code_name(rc));
      if (result == FAITH_OK)
        result = rc;
    }
    /* avoid client-to-client communication on server shutdown */
    else if (hmlen(devices) - 1 == 0) {
      rc = server_client_queue_disconnect(
          s, cl->pending_device_link_conn, FAITH_DISCONNECT_TEMPORARY_FAILURE,
          FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
          "All authorized devices of the account you "
          "are trying to link to have disconnected.");
      if (result == FAITH_OK)
        result = rc;
    }

    device_link_remove_request(cl);
  }

  if (cl->authorized) {
    client_device_session_data_t *sess = NULL;
    _FH_CHECK_RETURN(sess_registry_get_session(&s->rt, &cl->ident.auth_id,
                                               &cl->ident.device_id, &sess));

    if (sess) {
      sess->conn = NULL;
    }
  }

  {
    _FH_CHECK(reactor_remove(&s->reactor, &cl->reactor_source));
    if (_fh_rc != FAITH_OK) {
      result = _fh_rc;
    }
  }

  {
    _FH_CHECK(tls_shutdown(&cl->conn.tls));
    if (_fh_rc != FAITH_OK) {
      result = _fh_rc;
    }
  }

  const int      log_fd = cl->conn.fd;
  const uint64_t log_conn_id = cl->conn.id;

  if (cl->conn.fd >= 0) {
    if (close(cl->conn.fd) < 0) {
      nob_log(ERROR, "[%s: client=%" PRIu64 " fd=%d] close failed: %s",
              _MODULE_NAME, cl->conn.id, cl->conn.fd, strerror(errno));

      if (result == FAITH_OK)
        result = FAITH_ERR_IO;
    }

    cl->conn.fd = -1;
  }

  /* An LLM would have generated .in then .out */
  {
    _FH_CHECK(conn_queue_free(&cl->conn.out));
    if (_fh_rc != FAITH_OK) {
      result = _fh_rc;
    }
  }
  {
    _FH_CHECK(conn_queue_free(&cl->conn.in));
    if (_fh_rc != FAITH_OK) {
      result = _fh_rc;
    }
  }

  nob_log(INFO, "[%s: client=%" PRIu64 " fd=%d] Closed client", _MODULE_NAME,
          log_conn_id, log_fd);

  free(cl);

  return result;
}

faith_status_code_t
server_client_queue_disconnect(server_state_t *s, struct client_conn_t *cl,
                               faith_client_disconnect_reason_t reason,
                               faith_client_reconnect_policy_t reconnect_policy,
                               uint64_t                        retry_after_ms,
                               uint64_t banned_until_ms, const char *message) {
  if (!s || !cl)
    return FAITH_ERR_INVALID;

  if (cl->closing)
    return FAITH_OK;

  faith_envl_stc_client_disconnect_t disconnect_envl = {0};

  disconnect_envl.reason = (uint32_t)reason;
  disconnect_envl.reconnect_policy = (uint32_t)reconnect_policy;
  disconnect_envl.retry_after_ms = retry_after_ms;
  disconnect_envl.banned_until_ms = banned_until_ms;

  if (message)
    snprintf(disconnect_envl.msg, sizeof(disconnect_envl.msg), "%s", message);

  uint8_t           body[FAITH_ENVL_STC_CLIENT_DISCONNECT_BODY_SIZE_MAX] = {0};
  faith_body_size_t body_size = 0;

  {
    _FH_CHECK(faith_encode_client_disconnect_body(
        body, &body_size, sizeof(body), &disconnect_envl));
    if (_fh_rc != FAITH_OK) {
      /* We cannot produce the final protocol message, so close immediately */
      cl->closing = 1;
      return _fh_rc;
    }
  }

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_CLIENT_DISCONNECT;
  envl.recipient_id = cl->state == CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE
                          ? cl->pending_auth_id
                          : cl->ident.auth_id;
  envl.body = body;
  envl.body_size = body_size;

  {
    _FH_CHECK(server_queue_envelope(s, cl, &envl));

    if (_fh_rc != FAITH_OK) {
      /* Sending failed. There is no useful recovery for this connection, so
       * close immediately. */
      cl->closing = 1;
      return _fh_rc;
    }
  }

  /* Stop accepting further application messages but allow the queued
   * DISCONNECT envelope to be written first. */
  cl->close_after_flush = 1;

  return FAITH_OK;
}
