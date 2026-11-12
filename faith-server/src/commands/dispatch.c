#include "dispatch.h"

#include <faith-proto/codec/auth.h>
#include <faith-proto/codec/codec.h>
#include <faith-proto/core/core.h>

#include "../server/client_io.h"

#include "conversation.h"

#define _MODULE_NAME "commands/dispatch"

faith_status_code_t
command_queue_result(server_state_t *s, client_conn_t *cl,
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
  envl.recipient_id = cl->ident.auth_id;

  _FH_CHECK_RETURN(server_queue_envelope_or_mark_dead(s, cl, &envl));

  return FAITH_OK;
}

faith_status_code_t command_dispatch(server_state_t *s, client_conn_t *cl,
                                     const faith_envelope_t *envl) {
  if (!s || !cl || cl->closing || !envl || cl->state != CLIENT_OPEN)
    return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_COMMAND)
    return FAITH_ERR_INVALID;

  faith_command_result_err_t err = FAITH_COMMAND_ERR_NONE;
  faith_command_result_t     result = FAITH_COMMAND_RESULT_NONE;
  faith_envl_cts_command_t   cmd = {0};

  if (!cl->authorized) {
    return FAITH_ERR_UNAUTHORIZED;
  }

  if (!faith_client_id_equal(cl->ident.auth_id, envl->sender_id)) {
    return FAITH_ERR_NOT_EQUAL;
  }

  _FH_CHECK_RETURN(
      faith_decode_command_body(envl->body, envl->body_size, &cmd));

  client_device_session_data_t *sess = NULL;
  faith_status_code_t lookup = sess_registry_get_session(&s->rt,
      &cl->ident.auth_id, &cl->ident.device_id, &sess);

  if (lookup != FAITH_OK || !sess) {
    free(cmd.payload);
    return FAITH_ERR_UNAUTHORIZED;
  }

  uint64_t now = faith_now_ms();

  if (now - sess->command_window_ms >= 60000) {
    sess->command_window_ms = now;
    sess->command_count = 0;
  }

  if (now - s->command_window_ms >= 60000) {
    s->command_window_ms = now;
    s->command_count = 0;
  }

  if (sess->command_count >= 60 || s->command_count >= 6000) {
    free(cmd.payload);
    return command_queue_result(s, cl, &cmd.cmd_id, cmd.type,
        FAITH_COMMAND_ERR_TIMED_OUT, FAITH_COMMAND_RESULT_REJECTED);
  }

  sess->command_count++;
  s->command_count++;

  switch (cmd.type) {
  case FAITH_COMMAND_CREATE_CONVERSATION:
    {
      faith_status_code_t rc = conv_handle_create_conversation(s, cl, &cmd);
      free(cmd.payload);
      return rc;
    }

  default:
    nob_log(ERROR, "[%s: client=%" PRIu64 " fd=%i] Server got invalid COMMAND.",
            _MODULE_NAME, cl->conn.id, cl->conn.fd);

    err = FAITH_COMMAND_ERR_BAD_COMMAND;
    result = FAITH_COMMAND_RESULT_REJECTED;
  }

  if (result == FAITH_COMMAND_RESULT_NONE) {
    err = FAITH_COMMAND_ERR_BAD_COMMAND;
    result = FAITH_COMMAND_RESULT_REJECTED;
  }

  _FH_CHECK_RETURN(
      command_queue_result(s, cl, &cmd.cmd_id, cmd.type, err, result));
  return FAITH_OK;
}
