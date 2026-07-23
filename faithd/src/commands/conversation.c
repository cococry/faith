#include "conversation.h"

faith_status_code_t
conv_handle_create_conversation(server_state_t *s, client_conn_t *cl,
                                const faith_envl_cts_command_t *cmd) {
  if(!s || !cl || !cmd) return FAITH_ERR_INVALID;

  faith_cmd_create_converstation_t create_conv_cmd =  {0};
  _FH_CHECK_RETURN(faith_decode_cmd_create_conversation(cmd->payload,cmd->payload_size, &create_conv_cmd));

  char auth_id_hex[33];
  _FH_CHECK_RETURN(
      faith_id128_to_hex(create_conv_cmd.conversant_id.bytes, auth_id_hex));
  printf("Someone wants to chat with: %s\n", auth_id_hex);

  return FAITH_OK;
}
