#include "conversation.h"

#include "../delivery/events.h"

static faith_status_code_t send_conversation_created(
    server_state_t *s, client_conn_t *cl,
    const faith_event_conversation_created_t *conv_created) {

  if (!s || !cl || !conv_created)
    return FAITH_ERR_INVALID;
  uint8_t           data[FAITH_EVENT_CONVERSATION_CREATED_DATA_SIZE] = {0};
  faith_body_size_t data_size = 0;

  _FH_CHECK_RETURN(faith_encode_event_conversation_created(
      data, &data_size, sizeof(data), conv_created));

  _FH_CHECK_RETURN(delivery_queue_event(s, cl, FAITH_EVENT_CONVERSATION_CREATED,
                                        data, data_size));

  return FAITH_OK;
}

faith_status_code_t conv_handle_create_conversation(
    server_state_t *s, client_conn_t *cl, const faith_envl_cts_command_t *cmd,
    faith_command_result_t *o_result, faith_command_result_err_t *o_err) {
  if (!s || !cl || !cmd || !o_result || !o_err)
    return FAITH_ERR_INVALID;

  *o_result = FAITH_COMMAND_RESULT_NONE;
  *o_err = FAITH_COMMAND_ERR_NONE;

  faith_status_code_t        _fh_result = FAITH_OK;
  faith_command_result_err_t potential_err = FAITH_COMMAND_ERR_BAD_COMMAND;

  faith_cmd_create_converstation_t create_conv_cmd = {0};
  {
    _FH_CHECK_DEFER(faith_decode_cmd_create_conversation(
        cmd->payload, cmd->payload_size, &create_conv_cmd));
  }

  faith_conversation_id_t conv_id = {0};

  potential_err = FAITH_COMMAND_ERR_INTERNAL_ERROR;
  _FH_CHECK_DEFER(faith_random_bytes(conv_id.bytes, sizeof(conv_id.bytes)));

  faith_event_conversation_created_t conv_created = {.conversation_id =
                                                         conv_id};
  _FH_CHECK_RETURN(send_conversation_created(s, cl, &conv_created));

  *o_result = FAITH_COMMAND_RESULT_ACCEPTED;

  return FAITH_OK;

defer:
  *o_result = FAITH_COMMAND_RESULT_REJECTED;
  *o_err = potential_err;
  return _fh_result;
}
