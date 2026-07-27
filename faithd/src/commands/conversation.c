#include "conversation.h"

faith_status_code_t conv_handle_create_conversation(
    server_state_t *s, client_conn_t *cl, const faith_envl_cts_command_t *cmd,
    faith_command_result_t *o_result, faith_command_result_err_t *o_err) {
  if (!s || !cl || !cmd || !o_result || !o_err)
    return FAITH_ERR_INVALID;

  *o_result = FAITH_COMMAND_RESULT_NONE;
  *o_err = FAITH_COMMAND_ERR_NONE;

  faith_cmd_create_converstation_t create_conv_cmd = {0};
  {
    _FH_CHECK(faith_decode_cmd_create_conversation(
        cmd->payload, cmd->payload_size, &create_conv_cmd));
    if (_fh_rc != FAITH_OK) {
      *o_result = FAITH_COMMAND_RESULT_REJECTED;
      *o_err = FAITH_COMMAND_ERR_BAD_COMMAND;
      return _fh_rc;
    }
  }

  char auth_id_hex[33];
  _FH_CHECK(
      faith_id128_to_hex(create_conv_cmd.conversant_id.bytes, auth_id_hex));
  printf("Someone wants to chat with: %s\n", auth_id_hex);

  *o_result = FAITH_COMMAND_RESULT_ACCEPTED;
  *o_err = FAITH_COMMAND_ERR_NONE;

  return FAITH_OK;
}
