#include "dispatch.h"

#include "../codec/protocol.h"
#include "../core/core.h"

#include "../server/client_io.h"

#include "../auth/structs.h"

#include "conversation.h"

static faith_status_code_t
queue_command_result(server_state_t *s, client_conn_t *cl,
                     const faith_command_id_t *cmd_id,
                     faith_command_type_t type, faith_command_result_err_t err,
                     faith_command_result_t result) {
  if (!s || !cl || !cmd_id)
    return FAITH_ERR_INVALID;

  faith_envl_stc_command_result_t res = {0};
  res.cmd_id = *cmd_id;
  res.type = type;
  res.err = err;
  res.result = result;

  uint8_t           body[FAITH_ENVL_STC_COMMAND_RESULT_BODY_SIZE] = {0};
  faith_body_size_t body_size = 0;
  _FH_CHECK_RETURN(
      faith_encode_command_result_body(body, &body_size, sizeof(body), &res));

  faith_envelope_t envl = {0};
  envl.body = body;
  envl.body_size = body_size;
  envl.type = FAITH_ENVELOPE_COMMAND_RESULT;
  envl.recipient_id = cl->auth_id;

  _FH_CHECK_RETURN(server_queue_envelope_or_mark_dead(s, cl, &envl));

  return FAITH_OK;
}

faith_status_code_t command_dispatch(server_state_t *s, client_conn_t *cl,
                                     const faith_envelope_t *envl) {
  if (!s || !cl || cl->closing || !envl || cl->state != CLIENT_OPEN)
    return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_COMMAND)
    return FAITH_ERR_INVALID;

  faith_status_code_t _fh_result = FAITH_OK;

  faith_command_result_err_t err = FAITH_COMMAND_ERR_NONE;
  faith_command_result_t     result = FAITH_COMMAND_RESULT_NONE;
  faith_envl_cts_command_t   cmd = {0};

  if (!cl->authorized) {
    return FAITH_ERR_UNAUTHORIZED;
  }

  if (!faith_client_id_equal(cl->auth_id, envl->sender_id)) {
    return FAITH_ERR_NOT_EQUAL;
  }

  _FH_CHECK_RETURN(
      faith_decode_command_body(envl->body, envl->body_size, &cmd));

  switch (cmd.type) {
  case FAITH_COMMAND_CREATE_CONVERSATION:
    _FH_CHECK_DEFER(
        conv_handle_create_conversation(s, cl, &cmd, &result, &err));
    break;
  default:
    nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Server got invalid COMMAND.",
            cl->conn.id, cl->conn.fd);

    err = FAITH_COMMAND_ERR_BAD_COMMAND;
    result = FAITH_COMMAND_RESULT_REJECTED;
    _FH_RETURN_DEFER(FAITH_ERR_INVALID);
  }

  if (result == FAITH_COMMAND_RESULT_NONE) {
    err = FAITH_COMMAND_ERR_BAD_COMMAND;
    result = FAITH_COMMAND_RESULT_REJECTED;
    _FH_RETURN_DEFER(FAITH_ERR_INVALID);
  }

defer: {
  _FH_CHECK(queue_command_result(s, cl, &cmd.cmd_id, cmd.type, err, result));
  if (_fh_rc != FAITH_OK) {
    _fh_result = _fh_rc;
  }
}
  return _fh_result;
}
