#include "device_link.h"
#include "../../third_party/stb_ds.h"
#include "../server/client_io.h"
#include "../server/client_lifecycle.h"

#include "../codec/signatures.h"

#include "../delivery/routing.h"
#include "structs.h"

#define NOB_IMPLEMENTATION
#include "../../third_party/nob.h"

#include "../logging/logging.h"
#include "handshake.h"

static faith_status_code_t send_device_auth_response_failed(server_state_t *s,
                                                            client_conn_t *cl) {
  if (!s || !cl || cl->closing)
    return FAITH_ERR_INVALID;

  faith_envelope_t failed_envl = {
      .type = FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_FAILED,
      .recipient_id = cl->ident.auth_id,
      .body = NULL,
      .body_size = 0,
  };

  _FH_CHECK(server_queue_envelope_or_mark_dead(s, cl, &failed_envl));
  return _fh_rc;
}

faith_status_code_t
device_link_handle_device_response(server_state_t *s, client_conn_t *cl,
                                   const faith_envelope_t *response_envl) {
  if (!s || !cl || cl->closing || !response_envl)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_OPEN)
    return FAITH_ERR_BAD_ENVELOPE;

  faith_status_code_t _fh_result = FAITH_OK;
  int                 blame_responder = 0;

  faith_envl_stc_device_link_req_t *req = cl->pending_device_link_req;
  struct client_conn_t             *req_cl = cl->pending_device_link_conn;

  if (!req || !req_cl) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got %s but there is no "
            "device link request pending. Rejecting envelope.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));
    return FAITH_ERR_INVALID;
  }

  if (response_envl->type != FAITH_ENVELOPE_DEVICE_AUTH_APPROVE &&
      response_envl->type != FAITH_ENVELOPE_DEVICE_AUTH_DENY) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Invalid envelope type. Expected "
            "FAITH_ENVELOPE_DEVICE_AUTH_APPROVE or "
            "FAITH_ENVELOPE_DEVICE_AUTH_DENY, got %s",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    return FAITH_ERR_INVALID;
  }

  if (!response_envl->body ||
      response_envl->body_size !=
          FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got invalid %s envelope contents.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    _FH_CHECK_RETURN(send_device_auth_response_failed(s, cl));

    return FAITH_ERR_INVALID;
  }

  if (!cl->authorized) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(faith_id128_to_hex(cl->ident.auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(cl->ident.device_id.bytes, device_id_hex));

    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got unauthorized %s"
            "from client. Client (auth_id=%s, device_id=%s) is not authorized.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type),
            auth_id_hex, device_id_hex);

    /* Return UNAUTHORIZED without rejecting/closing the client connection that
     * requested the device link. We are protecting the pending client
     * connection here. */
    return FAITH_ERR_UNAUTHORIZED;
  }

  if (!req_cl || req_cl->closing) {
    char req_auth_id_hex[33];
    char req_device_id_hex[33];
    _FH_CHECK_RETURN(faith_id128_to_hex(req->auth_id.bytes, req_auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(req->device_id_new.bytes, req_device_id_hex));

    nob_log(INFO,
            "[client=%" PRIu64 " fd=%i] Client that requested"
            "their device (device_id=%s) to be linked to auth_id=%s has "
            "already been closed.",
            cl->conn.id, cl->conn.fd, req_auth_id_hex, req_device_id_hex);

    _FH_CHECK_RETURN(send_device_auth_response_failed(s, cl));

    return FAITH_OK;
  }

  faith_envl_cts_device_link_response_t response = {0};
  _FH_CHECK_RETURN(faith_decode_device_link_response_body(
      response_envl->body, response_envl->body_size, &response));

  if (!faith_device_id_equal(response.device_id_new, req->device_id_new)) {
    nob_log(
        ERROR,
        "[client=%" PRIu64 " fd=%i] Server got %s but the sent"
        "device_id does not match the device_id that requested the approval.",
        cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    _FH_CHECK_RETURN(send_device_auth_response_failed(s, cl));

    /* Return INVALID without rejecting/closing the client connection that
     * requested the device link. */
    return FAITH_ERR_INVALID;
  }

  if (faith_now_ms() > cl->pending_device_link_req->expires_at_ms) {
    nob_log(ERROR,
            "[client=%" PRIu64 " fd=%i] Server got %s but the "
            "link request has already expired.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    /* Reject the requesting client conection if the request has expired but
     * keep the responding one open. */
    _FH_RETURN_DEFER(FAITH_ERR_EXPIRED);
  }

  /* Construct message buffer for signature generation
   *    Message Buffer {
   *      auth_id,
   *      device_id_new,
   *      public_key_new_device,
   *      code,
   *      expires_at_ms,
   *      device_id_approving (device ID of the responding device)
   *    } */

  faith_signature_device_link_response_t sign_msg = {0};
  sign_msg.auth_id = req->auth_id;
  sign_msg.device_id_new = req->device_id_new;

  memcpy(sign_msg.public_key_new_device, req->public_key_new_device,
         FAITH_ED25519_PUBLIC_KEY_SIZE);
  memcpy(sign_msg.code, req->code, sizeof(req->code));

  sign_msg.expires_at_ms = req->expires_at_ms;
  sign_msg.device_id_responding = cl->ident.device_id;

  sign_msg.type = response_envl->type == FAITH_ENVELOPE_DEVICE_AUTH_APPROVE
                      ? FAITH_DEVICE_LINK_APPROVE
                      : FAITH_DEVICE_LINK_DENY;

  size_t  msg_size = 0;
  uint8_t msg_buf[FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE];
  {
    blame_responder = 1;
    _FH_CHECK(faith_gen_sign_buf_device_link_response(
        msg_buf, &msg_size, sizeof(msg_buf), &sign_msg));

    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR,
              "Failed to generate %s signing "
              "message buffer.",
              faith_envelope_name(response_envl->type));

      blame_responder = 1;
      _FH_RETURN_DEFER(_fh_rc);
    }
  }

  client_device_session_data_t *sess = NULL;
  {
    _FH_CHECK(sess_registry_get_session(&s->rt, &cl->ident.auth_id,
                                        &cl->ident.device_id, &sess));
    /* We specifically need routing_get_session() to return FAITH_OK. This is
     * returned only if <cl->ident.auth_id> is a registered client_route_user_t
     * and <cl->ident.device_id> is a registered client_route_device_t of that
     * user.
     * */
    if (_fh_rc != FAITH_OK) {
      _fh_result = _fh_rc;
      /* Reject the requesting client conection if the authorized connection
       * does not actually have an authorized session. */

      blame_responder = 1;
      _FH_RETURN_DEFER(FAITH_ERR_UNAUTHORIZED);
    }
  }

  /* This means cl->ident.auth_id is registered but cl->ident.device_id is not,
   * effectively telling us that the client connection is not yet authorized.
   * Because we checked cl->authorized above, this should never happen with
   * correct behaviour.*/
  if (!sess) {
    nob_log(ERROR,
            "[client=%" PRIu64
            " fd=%i] Server got %s but client connection that "
            "sent the envelope does not have registered session data. "
            "However, the client connection IS authorized, so there is "
            "probably a deeper issue.",
            cl->conn.id, cl->conn.fd, faith_envelope_name(response_envl->type));

    _FH_CHECK_RETURN(send_device_auth_response_failed(s, cl));

    return FAITH_ERR_INVALID;
  }

  /* Verify the signature */
  faith_status_code_t verification_rc = FAITH_ERR_UNAUTHORIZED;
  {
    _FH_CHECK(faith_verify_signature_raw_pubkey(
        sess->ident.public_key, msg_buf, sizeof(msg_buf),
        response.signature_response, sizeof(response.signature_response)));

    verification_rc = _fh_rc;
  }

  int authorized = verification_rc == FAITH_OK;
  if (!authorized) {
    _FH_RETURN_DEFER(verification_rc == FAITH_ERR_NOT_EQUAL
                         ? FAITH_ERR_UNAUTHORIZED
                         : verification_rc);
  }

  /* ======================================== */
  /* Client proved their legitimacy to us. */
  /* ======================================== */

  faith_status_code_t rc = FAITH_OK;

  switch (response_envl->type) {
  case FAITH_ENVELOPE_DEVICE_AUTH_DENY: {
    _FH_CHECK(server_client_queue_disconnect(
        s, req_cl, FAITH_DISCONNECT_DEVICE_REJECTED,
        FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
        "The device has been rejected by the account owner."));
    if (_fh_rc != FAITH_OK)
      rc = _fh_rc;
    break;
  }
  case FAITH_ENVELOPE_DEVICE_AUTH_APPROVE:
    _FH_CHECK_DEFER(auth_authorize_client(s, req_cl, &req->auth_id,
                                          &req->device_id_new,
                                          req->public_key_new_device, 1));
    break;
  default:
    return FAITH_ERR_UNREACHABLE;
  }

  faith_envelope_t ack_envl = {0};
  ack_envl.type = FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_ACK;
  _FH_CHECK_RETURN(
      delivery_route_envelope_to_auth_id(s, cl, &cl->ident.auth_id, &ack_envl));

  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE_CONNECTION(s, &cl->ident.auth_id, recipient,
                                      device_loop_rc,
                                      { device_link_remove_request(cl); });

  return device_loop_rc == FAITH_OK ? rc : device_loop_rc;

defer: {
  /* Copies for logging */
  faith_auth_id_t   client_id = req->auth_id;
  faith_device_id_t device_id_new = req->device_id_new;

  /* Remove the pending device link request from <cl>,
   * the requested client */
  device_link_remove_request(cl);

  /* Close the connection that requested the device link: <req_cl> */
  char buf[FAITH_MAX_CLIENT_DISCONNECT_MSG];
  snprintf(buf, sizeof(buf),
           "Client failed device-link authorization. (Error: %s)",
           faith_status_code_name(_fh_result));

  /* Don't propagate status code */
  _FH_CHECK_SCOPED(server_client_queue_disconnect(
      s, req_cl, FAITH_DISCONNECT_AUTH_FAILED, FAITH_CLIENT_RECONNECT_FORBIDDEN,
      0, 0, buf));

  char auth_id_hex[33];
  char device_id_hex[33];
  _FH_CHECK_RETURN(faith_id128_to_hex(client_id.bytes, auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(device_id_new.bytes, device_id_hex));

  nob_log(ERROR,
          "[client=%" PRIu64 " fd=%i] Client connection"
          "(auth_id=%s, device_id=%s) failed authorization for device link "
          "request; Error %s. Device with device_id=%s will not be linked to "
          "auth_id=%s. "
          "Closing connection. ",
          req_cl->conn.id, req_cl->conn.fd, auth_id_hex, device_id_hex,
          faith_status_code_name(_fh_result), device_id_hex, auth_id_hex);

  /* Don't propagate status code */
  {
    _FH_CHECK(send_device_auth_response_failed(s, cl));
  }

  return blame_responder ? _fh_result : FAITH_OK;
}
}

faith_status_code_t
device_link_new_device(server_state_t *s, client_conn_t *cl,
                       const client_auth_handshake_params_t *params) {
  if (!s || !cl || cl->closing || !params)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_WAIT_FOR_CHALLENGE_RESPONSE)
    return FAITH_ERR_BAD_ENVELOPE;

  if (g_verbose_logging) {
    char cl_auth_id_hex[33];
    char new_device_id_hex[33];
    _FH_CHECK_RETURN(
        faith_id128_to_hex(params->sender_auth_id.bytes, cl_auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(params->device_id.bytes, new_device_id_hex));

    nob_log(
        INFO,
        "[client=%" PRIu64
        " fd=%i] Handling newly joined device (device_id=%s) for auth_id=%s.",
        cl->conn.id, cl->conn.fd, new_device_id_hex, cl_auth_id_hex);
  }

  if (!sess_registry_auth_id_registered(&s->rt, &params->sender_auth_id)) {
    server_client_queue_disconnect(
        s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
        FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
        "The specified auth ID is in an invalid internal "
        "state.");
    return FAITH_ERR_NOT_FOUND;
  }

  /* Send device authorization request to every already registered device
   for that auth ID */
  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE_CONNECTION(
      s, &params->sender_auth_id, authorized_cl, device_loop_rc, {
        faith_envl_stc_device_link_req_t *req = NULL;
        _FH_CHECK(device_link_queue_request(
            s, authorized_cl, cl, &params->sender_auth_id, params->public_key,
            &params->device_id, &req));

        if (_fh_rc != FAITH_OK || !req) {
          char sender_auth_id_hex[33];
          _FH_CHECK_RETURN(faith_id128_to_hex(params->sender_auth_id.bytes,
                                              sender_auth_id_hex));
          char device_id_hex[33];
          _FH_CHECK_RETURN(
              faith_id128_to_hex(params->device_id.bytes, device_id_hex));
          nob_log(ERROR,
                  "Failed to send device link request to authorized "
                  "device session with device_id: %s (auth_id: %s)",
                  device_id_hex, sender_auth_id_hex);

          return _fh_rc;
        }

        /* Set pending device link request of receiving client connection. We
         * also store the client connection that sent the device link request.
         */
        authorized_cl->pending_device_link_req = req;
        authorized_cl->pending_device_link_conn = cl;
      });

  /* Send DEVICE_AUTH_PENDING to the connection that requested the
   * new device */
  _FH_CHECK_RETURN(auth_queue_auth_pending(s, cl));

  cl->pending_auth_id = params->sender_auth_id;
  server_set_client_state(s, cl, CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE);

  return FAITH_OK;
}

faith_status_code_t
device_link_queue_request_cancellation(server_state_t *s,
                                       client_conn_t  *requesting_cl) {
  if (!s || !requesting_cl)
    return FAITH_ERR_INVALID;

  if (requesting_cl->state != CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE)
    return FAITH_OK;

  client_auth_handshake_params_t *params =
      &requesting_cl->temp_handshake_params;

  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE_CONNECTION(
      s, &params->sender_auth_id, authorized_cl, device_loop_rc, {
        if (authorized_cl->pending_device_link_conn != requesting_cl)
          continue;

        /* Send DEVICE_LINK_CANCELLED to the authorized device */
        faith_envelope_t envl = {0};
        envl.type = FAITH_ENVELOPE_DEVICE_LINK_CANCELLED;
        envl.recipient_id = authorized_cl->ident.auth_id;
        _FH_CHECK(server_queue_envelope_or_mark_dead(s, authorized_cl, &envl));

        if (_fh_rc != FAITH_OK && device_loop_rc == FAITH_OK)
          device_loop_rc = _fh_rc;

        /* Remove the pending device link request from the client
         * connection */
        device_link_remove_request(authorized_cl);
      });

  return device_loop_rc;
}

faith_status_code_t device_link_queue_request(
    server_state_t *s, client_conn_t *recipient_cl, client_conn_t *request_cl,
    const faith_auth_id_t *auth_id,
    const uint8_t          public_key_new_device[FAITH_ED25519_PUBLIC_KEY_SIZE],
    const faith_device_id_t           *new_device_id,
    faith_envl_stc_device_link_req_t **o_req) {
  if (!s || !auth_id || !public_key_new_device || !new_device_id ||
      !recipient_cl) {
    return FAITH_ERR_INVALID;
  }
  if (recipient_cl->state != CLIENT_OPEN)
    return FAITH_ERR_BAD_ENVELOPE;
  if (request_cl->closing || recipient_cl->closing)
    return FAITH_ERR_INVALID;

  if (!recipient_cl->authorized) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->ident.auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->ident.device_id.bytes, device_id_hex));
    nob_log(ERROR,
            "Not sending device link request to "
            "device with device_id: %s (auth_id: %s). Client connection is "
            "not authorized.",
            device_id_hex, auth_id_hex);

    return FAITH_ERR_UNAUTHORIZED;
  }

  if (recipient_cl->pending_device_link_req != NULL) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->ident.auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_cl->ident.device_id.bytes, device_id_hex));
    nob_log(ERROR,
            "Not sending device link request to "
            "device with device_id: %s (auth_id: %s). Another device link "
            "request is already pending.",
            device_id_hex, auth_id_hex);

    server_client_queue_disconnect(
        s, request_cl, FAITH_DISCONNECT_DEVICE_REJECTED,
        FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
        "Another device-link request is already pending on this account.");

    return FAITH_OK;
  }

  /* Allocate device link request */
  faith_envl_stc_device_link_req_t *req = calloc(1, sizeof(*req));
  if (!req)
    return FAITH_ERR_NOMEM;

  req->device_id_new = *new_device_id;

  memcpy(req->public_key_new_device, public_key_new_device,
         FAITH_ED25519_PUBLIC_KEY_SIZE);

  req->auth_id = *auth_id;

  _FH_CHECK_RETURN(faith_random_bytes(req->code, sizeof(req->code)));

  req->expires_at_ms =
      faith_now_ms() + FAITH_DEVICE_LINK_REQ_EXPIRATION_TIME_MS;

  /* Serialize device link reqest */

  uint8_t           body[FAITH_ENVL_STC_DEVICE_LINK_REQ_BODY_SIZE] = {0};
  faith_body_size_t body_size = 0;
  _FH_CHECK_RETURN(
      faith_encode_device_link_req_body(body, &body_size, sizeof(body), req));

  faith_envelope_t device_link_req_envl = {0};
  device_link_req_envl.body = body;
  device_link_req_envl.body_size = body_size;
  device_link_req_envl.recipient_id = *auth_id;
  device_link_req_envl.type = FAITH_ENVELOPE_DEVICE_LINK_REQUEST;

  _FH_CHECK_RETURN(server_queue_envelope_or_mark_dead(s, recipient_cl,
                                                      &device_link_req_envl));

  *o_req = req;

  return FAITH_OK;
}

void device_link_remove_request(struct client_conn_t *cl) {

  if (!cl || !cl->pending_device_link_req)
    return;

  free(cl->pending_device_link_req);
  cl->pending_device_link_req = NULL;
  cl->pending_device_link_conn = NULL;
}
