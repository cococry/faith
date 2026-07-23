#include "dispatch.h"

#include "../codec/protocol.h"
#include "../core/core.h"

#include "../auth/structs.h"


#include "conversation.h"

faith_status_code_t
commands_dispatch_command(server_state_t *s, client_conn_t *cl,
                                const faith_envelope_t *envl) {
  if (!s || !cl || cl->closing || !envl || cl->state != CLIENT_OPEN)
    return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_COMMAND)
    return FAITH_ERR_INVALID;

  if (!cl->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  if (!faith_client_id_equal(cl->auth_id, envl->sender_id))
    return FAITH_ERR_NOT_EQUAL;

  faith_envl_cts_command_t cmd = {0};
  _FH_CHECK_RETURN(
      faith_decode_command_body(envl->body, envl->body_size, &cmd));

  switch(cmd.type) {
    case FAITH_COMMAND_CREATE_CONVERSATION:
      conv_handle_create_conversation(s, cl, &cmd);
      break;
    default:
      nob_log(ERROR, "[client=%" PRIu64 " fd=%i] Server got invalid COMMAND.",
              cl->conn.id, cl->conn.fd);
      return FAITH_ERR_INVALID;
  }

  return FAITH_OK;
}
