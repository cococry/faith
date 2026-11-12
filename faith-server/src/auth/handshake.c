#include "handshake.h"

#include "../server/client_io.h"
#include "../server/client_lifecycle.h"
#include "device_link.h"
#include <faith-proto/codec/auth.h>
#include <faith-proto/codec/codec.h>
#include <faith-proto/codec/signatures.h>

#include "../delivery/events.h"

#define _MODULE_NAME "auth/handshake"

static faith_status_code_t _identity_run(faith_db_t *db, void *data);
static void                _identity_complete(void *owner, void *data,
                                              faith_status_code_t status);

static faith_status_code_t _identity_run(faith_db_t *db, void *data) {
  return db_identity_resolve(db, data);
}

static void _identity_complete(void *owner, void *data,
                               faith_status_code_t status) {
  server_state_t    *s = owner;
  db_identity_job_t *job = data;
  job->status = status;
  client_conn_t *cl = s->clients;

  while (cl && cl->conn.id != job->client_id)
    cl = cl->next;

  if (!cl || cl->closing || cl->close_after_flush ||
      cl->state != CLIENT_WAIT_FOR_IDENTITY)
    return;

  faith_status_code_t rc = job->status;

  if (rc == FAITH_ERR_NOT_FOUND && !job->link) {
    server_set_client_state(s, cl, CLIENT_WAIT_FOR_CHALLENGE_RESPONSE);
    rc = device_link_new_device(s, cl, &cl->temp_handshake_params);
  } else if (rc == FAITH_OK) {
    client_device_session_data_t *sess = NULL;
    sess_registry_get_session(&s->rt, &job->auth_id, &job->device.device_id,
                              &sess);

    if (sess)
      sess->verified_at_ms = faith_now_ms();

    rc = auth_authorize_client(s, cl, &job->auth_id, &job->device.device_id,
                               job->device.public_key, sess == NULL);
  }

  if (rc != FAITH_OK) {
    server_client_queue_disconnect(s, cl, FAITH_DISCONNECT_AUTH_FAILED,
                                   FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0,
                                   "Identity verification failed.");
  }
}

faith_status_code_t auth_handle_hello(server_state_t *s, client_conn_t *cl,
                                      const faith_envelope_t *hello_envl) {
  // HELLO {
  //   header: {
  //     sender_id: auth_id
  //   }
  //   body: {
  //     device_id,
  //     public_key,
  //     client_nonce
  //   }
  // }
  //
  if (!s || !cl || cl->closing || !hello_envl)
    return FAITH_ERR_INVALID;

  if (hello_envl->type != FAITH_ENVELOPE_HELLO)
    return FAITH_ERR_INVALID;

  if (cl->state != CLIENT_WAIT_FOR_HELLO) {
    nob_log(ERROR,
            "[%s: client=%" PRIu64
            " fd=%i] Server got invalid HELLO from client.",
            _MODULE_NAME, cl->conn.id, cl->conn.fd);

    return FAITH_ERR_BAD_ENVELOPE;
  }

  faith_envl_cts_hello_t hello;
  _FH_CHECK_RETURN(
      faith_decode_hello_body(hello_envl->body, hello_envl->body_size, &hello));

  uint64_t server_nonce;
  _FH_CHECK_RETURN(
      faith_random_bytes((uint8_t *)&server_nonce, sizeof(server_nonce)));

  /* Construct temporary handshake parameters for identity evaluation. */
  client_auth_handshake_params_t *params = &cl->temp_handshake_params;

  params->sender_auth_id = hello_envl->sender_id;
  params->device_id = hello.device_id;

  memcpy(params->public_key, hello.public_key, sizeof(params->public_key));

  params->server_nonce = server_nonce;
  params->nonce = hello.client_nonce;

  /* Send CHALLENGE envelope. */

  faith_envl_stc_hello_challenge_t challenge = {
      .server_nonce = server_nonce,
  };

  uint8_t body[FAITH_ENVL_STC_HELLO_CHALLENGE_BODY_SIZE];

  faith_body_size_t body_size = 0;

  _FH_CHECK_RETURN(faith_encode_hello_challenge_body(body, &body_size,
                                                     sizeof(body), &challenge));

  faith_envelope_t challenge_envl = {
      .type = FAITH_ENVELOPE_CHALLENGE,
      .recipient_id = hello_envl->sender_id,
      .body = body,
      .body_size = body_size,
  };

  _FH_CHECK_RETURN(server_queue_envelope_or_mark_dead(s, cl, &challenge_envl));

  server_set_client_state(s, cl, CLIENT_WAIT_FOR_CHALLENGE_RESPONSE);

  return FAITH_OK;
}

faith_status_code_t auth_handle_challenge_response(
    server_state_t *s, client_conn_t *cl,
    const faith_envelope_t *challenge_response_envl) {
  if (!s || !cl || cl->closing || !challenge_response_envl)
    return FAITH_ERR_INVALID;

  if (challenge_response_envl->type != FAITH_ENVELOPE_CHALLENGE_RESPONSE) {
    nob_log(ERROR,
            "[%s: client=%" PRIu64 " fd=%i] Invalid envelope type. Expected "
            "FAITH_ENVELOPE_CHALLENGE_RESPONSE, got %s",
            _MODULE_NAME, cl->conn.id, cl->conn.fd,
            faith_envelope_name(challenge_response_envl->type));

    return FAITH_ERR_INVALID;
  }

  if (cl->state != CLIENT_WAIT_FOR_CHALLENGE_RESPONSE) {
    nob_log(ERROR,
            "[%s: client=%" PRIu64
            " fd=%i] Server got invalid CHALLENGE_RESPONSE from client.",
            _MODULE_NAME, cl->conn.id, cl->conn.fd);

    return FAITH_ERR_BAD_ENVELOPE;
  }

  client_auth_handshake_params_t *params = &cl->temp_handshake_params;

  if (!faith_client_id_equal(challenge_response_envl->sender_id,
                             params->sender_auth_id)) {
    nob_log(ERROR,
            "[%s: client=%" PRIu64
            " fd=%i] Server got CHALLENGE_RESPONSE from invalid client.",
            _MODULE_NAME, cl->conn.id, cl->conn.fd);

    return FAITH_ERR_INVALID;
  }

  if (!challenge_response_envl->body ||
      challenge_response_envl->body_size != FAITH_ED25519_SIGNATURE_SIZE) {
    nob_log(ERROR,
            "[%s: client=%" PRIu64
            " fd=%i] Server got invalid CHALLENGE_RESPONSE envelope contents.",
            _MODULE_NAME, cl->conn.id, cl->conn.fd);

    return FAITH_ERR_INVALID;
  }

  uint8_t                      *verification_public_key = params->public_key;
  client_device_session_data_t *sess = NULL;
  faith_status_code_t           sess_rc = sess_registry_get_session(
      &s->rt, &params->sender_auth_id, &params->device_id, &sess);

  if (sess_rc != FAITH_OK && sess_rc != FAITH_ERR_NOT_FOUND)
    return sess_rc;

  if (sess) {
    if (memcmp(params->public_key, sess->ident.public_key,
               FAITH_ED25519_PUBLIC_KEY_SIZE) != 0)
      return FAITH_ERR_UNAUTHORIZED;

    verification_public_key = sess->ident.public_key;
  }

  uint8_t client_signature[FAITH_ED25519_SIGNATURE_SIZE] = {0};
  memcpy(client_signature, challenge_response_envl->body,
         sizeof(client_signature));

  /* Construct message buffer for signature generation
   *    Message Buffer {
   *      sender_auth_id,
   *      verification_public_key,
   *      client_nonce,
   *      server_nonce,
   *      device_id
   *    } */

  faith_signature_hello_handshake_t sign_msg = {0};

  sign_msg.auth_id = params->sender_auth_id;
  sign_msg.device_id = params->device_id;

  memcpy(sign_msg.public_key, verification_public_key,
         FAITH_ED25519_PUBLIC_KEY_SIZE);

  sign_msg.client_nonce = params->nonce;
  sign_msg.server_nonce = params->server_nonce;

  size_t  msg_size = 0;
  uint8_t msg_buf[FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE];
  {
    _FH_CHECK(faith_gen_sign_buf_hello_handshake(msg_buf, &msg_size,
                                                 sizeof(msg_buf), &sign_msg));

    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR,
              "[%s: client=%" PRIu64
              " fd=%i] Failed to generate signing message buffer",
              _MODULE_NAME, cl->conn.id, cl->conn.fd);

      return _fh_rc;
    }
  }

  /* Verify the signature */
  faith_status_code_t verification_rc = FAITH_ERR_UNAUTHORIZED;
  {
    _FH_CHECK(faith_verify_signature_raw_pubkey(
        verification_public_key, msg_buf, sizeof(msg_buf), client_signature,
        sizeof(client_signature)));

    verification_rc = _fh_rc;
  }

  bool authorized = verification_rc == FAITH_OK;

  if (!authorized) {
    goto reject;
  }

  /* =============================== */
  /* Client passed authorization */
  /* =============================== */

  if (!sess || faith_now_ms() - sess->verified_at_ms >= 30000) {
    db_identity_job_t job = {
        .client_id = cl->conn.id,
        .auth_id = params->sender_auth_id,
        .device.device_id = params->device_id,
    };
    memcpy(job.device.public_key, params->public_key,
           FAITH_ED25519_PUBLIC_KEY_SIZE);

    /* Submit the database job to resolve the client's identity (creating it if
     * the auth-id is not already a persistent identity in the DB, resolving the
     * already present entry if it exists, or failing if additional checks
     * failed) */
    _FH_CHECK_RETURN(auth_identity_submit(s, &job));

    server_set_client_state(s, cl, CLIENT_WAIT_FOR_IDENTITY);

    return FAITH_OK;
  }

  _FH_CHECK_RETURN(auth_authorize_client(s, cl, &params->sender_auth_id,
                                         &params->device_id,
                                         verification_public_key, false));

  return FAITH_OK;

reject: {

  /* =============================== */
  /* Client failed authorization */
  /* =============================== */

  char sender_auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1];
  char device_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1];
  _FH_CHECK_RETURN(
      faith_id128_to_hex(params->sender_auth_id.bytes, sender_auth_id_hex));

  _FH_CHECK_RETURN(faith_id128_to_hex(params->device_id.bytes, device_id_hex));
  nob_log(ERROR,
          "[%s: client=%" PRIu64
          " fd=%i] Client failed authorization for requested routing session "
          "(auth_id=%s, device_id=%s). ",
          _MODULE_NAME, cl->conn.id, cl->conn.fd, sender_auth_id_hex,
          device_id_hex);

  return FAITH_ERR_UNAUTHORIZED;
}
}

faith_status_code_t auth_authorize_client(
    server_state_t *s, client_conn_t *cl, const faith_auth_id_t *auth_id,
    const faith_device_id_t *device_id,
    uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE], bool register_session) {
  if (!s || !cl || cl->closing || !auth_id || !device_id || !public_key)
    return FAITH_ERR_INVALID;

  cl->ident.auth_id = *auth_id;
  cl->ident.device_id = *device_id;

  if (register_session) {
    _FH_CHECK_RETURN(sess_registry_register_session(&s->rt, auth_id, device_id,
                                                    cl, public_key));
  } else {
    client_device_session_data_t *sess = NULL;
    _FH_CHECK_RETURN(
        sess_registry_get_session(&s->rt, auth_id, device_id, &sess));

    if (!sess)
      return FAITH_ERR_UNAUTHORIZED;

    if (sess->conn && sess->conn != cl)
      return FAITH_ERR_UNAUTHORIZED;

    sess->conn = cl;
  }

  cl->authorized = 1;
  _FH_CHECK_RETURN(auth_handshake_complete(s, cl, auth_id, device_id));

  char cl_auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1];
  char cl_device_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1];
  _FH_CHECK_RETURN(faith_id128_to_hex(cl->ident.auth_id.bytes, cl_auth_id_hex));
  _FH_CHECK_RETURN(
      faith_id128_to_hex(cl->ident.device_id.bytes, cl_device_id_hex));

  nob_log(INFO,
          "[%s: client=%" PRIu64 " fd=%i] Client passed authorization for "
          "requested routing session. (auth_id=%s, device_id=%s)",
          _MODULE_NAME, cl->conn.id, cl->conn.fd, cl_auth_id_hex,
          cl_device_id_hex);

  return FAITH_OK;
}

faith_status_code_t auth_handshake_ready(server_state_t *s, client_conn_t *cl,
                                         const faith_auth_id_t   *sender_id,
                                         const faith_device_id_t *device_id) {
  if (!s || !cl || cl->closing || !device_id)
    return FAITH_ERR_INVALID;

  if (!cl->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  /* Send HELLO_OK evelope back to client */
  faith_envelope_t hello_ok_envl = {0};
  hello_ok_envl.type = FAITH_ENVELOPE_HELLO_OK;
  hello_ok_envl.recipient_id = *sender_id;

  _FH_CHECK_RETURN(server_queue_envelope_or_mark_dead(s, cl, &hello_ok_envl));

  cl->ident.auth_id = *sender_id;
  cl->ident.device_id = *device_id;

  server_set_client_state(s, cl, CLIENT_OPEN);

  char auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1];
  char device_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1];

  faith_status_code_t _fh_result = FAITH_OK;

  _FH_CHECK_RETURN(faith_id128_to_hex(cl->ident.auth_id.bytes, auth_id_hex));
  _FH_CHECK_RETURN(
      faith_id128_to_hex(cl->ident.device_id.bytes, device_id_hex));

  nob_log(INFO,
          "[%s: client=%" PRIu64
          " fd=%i] Server accepted HELLO (auth id: %s, device id: %s)",
          _MODULE_NAME, cl->conn.id, cl->conn.fd, auth_id_hex, device_id_hex);

  return _fh_result;
}

faith_status_code_t auth_queue_auth_pending(server_state_t *s,
                                            client_conn_t  *recipient_cl) {
  if (!s || !recipient_cl || recipient_cl->closing)
    return FAITH_ERR_INVALID;

  faith_envelope_t device_auth_pending_envl = {0};
  device_auth_pending_envl.type = FAITH_ENVELOPE_DEVICE_AUTH_PENDING;
  _FH_CHECK_RETURN(server_queue_envelope_or_mark_dead(
      s, recipient_cl, &device_auth_pending_envl));

  return FAITH_OK;
}

faith_status_code_t auth_identity_submit(server_state_t          *s,
                                         const db_identity_job_t *request) {
  db_identity_job_t *job = malloc(sizeof(*job));

  if (!job)
    return FAITH_ERR_NOMEM;

  *job = *request;
  db_async_job_t task = {.run = _identity_run,
                         .complete = _identity_complete,
                         .destroy = free,
                         .data = job};

  faith_status_code_t rc = db_async_submit(&s->db_pool, &task);

  if (rc != FAITH_OK)
    free(job);

  return rc;
}

faith_status_code_t
auth_handshake_complete(server_state_t *s, client_conn_t *cl,
                        const faith_auth_id_t   *auth_id,
                        const faith_device_id_t *device_id) {
  (void)auth_id;
  (void)device_id;

  return delivery_load_events(s, cl);
}
