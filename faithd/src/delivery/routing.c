#include "routing.h"
#include "../server/client_io.h"

#include "../../third_party/stb_ds.h"

faith_status_code_t
delivery_route_envelope_to_auth_id(server_state_t *s, client_conn_t *cl_sender,
                                   const faith_auth_id_t  *recipient_auth_id,
                                   const faith_envelope_t *envl) {
  if (!s || !cl_sender || cl_sender->closing || !recipient_auth_id || !envl)
    return FAITH_ERR_INVALID;

  if (cl_sender->state != CLIENT_OPEN)
    return FAITH_ERR_BAD_ENVELOPE;

  if (!cl_sender->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  if (envl->body_size > 0 && !envl->body)
    return FAITH_ERR_INVALID;

  client_session_user_t *recipient_user = NULL;
  _FH_CHECK_RETURN(sess_registry_get_user_from_auth_id(
      &s->rt, recipient_auth_id, &recipient_user));
  if (!recipient_user) {

    char recipient_auth_id_hex[33];

    _FH_CHECK_RETURN(
        faith_id128_to_hex(recipient_auth_id->bytes, recipient_auth_id_hex));

    nob_log(INFO,
            "[client=%" PRIu64 " fd=%i] Envelope %s: Recipient "
            "(auth_id: %s) is offline; Not routing envelope.",
            cl_sender->conn.id, cl_sender->conn.fd,
            faith_envelope_name(envl->type), recipient_auth_id_hex);

    return FAITH_OK;
  }

  char recipient_auth_id_hex[33];

  _FH_CHECK_RETURN(
      faith_id128_to_hex(recipient_auth_id->bytes, recipient_auth_id_hex));

  faith_status_code_t device_loop_rc = FAITH_OK;
  _FH_FOR_EACH_AUTH_DEVICE_CONNECTION(
      s, recipient_auth_id, recipient_cl, device_loop_rc, {
        faith_envelope_t routing_envl = *envl;
        routing_envl.sender_id = cl_sender->ident.auth_id;
        routing_envl.recipient_id = *recipient_auth_id;
        _FH_CHECK(
            server_queue_envelope_or_mark_dead(s, recipient_cl, &routing_envl));

        char recipient_device_id_hex[33];
        _FH_CHECK_RETURN(faith_id128_to_hex(recipient_cl->ident.device_id.bytes,
                                            recipient_device_id_hex));

        if (_fh_rc != FAITH_OK) {
          nob_log(ERROR,
                  "[client=%" PRIu64
                  " fd=%i] Envelope %s: Failed to route envelope to "
                  "recipient device (auth_id: %s, device_id: %s).",
                  cl_sender->conn.id, cl_sender->conn.fd,
                  faith_envelope_name(envl->type), recipient_auth_id_hex,
                  recipient_device_id_hex);
          continue;
        }

        nob_log(INFO,
                "[client=%" PRIu64
                " fd=%i] Envelope %s: Routed envelope to recipient "
                "device (auth_id: %s, device_id: %s).",
                cl_sender->conn.id, cl_sender->conn.fd,
                faith_envelope_name(envl->type), recipient_auth_id_hex,
                recipient_device_id_hex);
      });

  return FAITH_OK;
}

faith_status_code_t
delivery_route_application_msg_envelope(server_state_t *s, client_conn_t *cl,
                                        faith_envelope_t *envl) {
  if (!s || !cl || !envl)
    return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_MSG_SEND)
    return FAITH_ERR_INVALID;

  /* MSG_SEND specifically requires a nonempty body. */
  if (envl->body_size == 0 || !envl->body)
    return FAITH_ERR_INVALID;

  _FH_CHECK_RETURN(
      delivery_route_envelope_to_auth_id(s, cl, &envl->recipient_id, envl));

  return FAITH_OK;
}
