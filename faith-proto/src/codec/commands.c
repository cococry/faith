#include "codec/commands.h"

#include "codec/helpers.h"

faith_status_code_t faith_encode_cmd_create_conversation(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_cmd_create_converstation_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_CMD_CREATE_CONVERSATION_BODY_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->conversant_id.bytes, sizeof(in->conversant_id.bytes));

  FAITH_ENCODE_EPILOGUE(FAITH_CMD_CREATE_CONVERSATION_BODY_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t
faith_decode_cmd_create_conversation(const uint8_t    *payload,
                                     faith_body_size_t payload_size,
                                     faith_cmd_create_converstation_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_CMD_CREATE_CONVERSATION_BODY_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->conversant_id.bytes,
                      sizeof(out->conversant_id));

  FAITH_DECODE_EPILOGUE(FAITH_CMD_CREATE_CONVERSATION_BODY_SIZE, !=);

  return FAITH_OK;
}

const char *faith_command_type_name(faith_command_type_t type) {
  switch (type) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_COMMAND_TYPES(X)
#undef X
  default:
    return "FAITH_COMMAND_TYPE_UNKNOWN";
  }
}
const char *faith_command_result_name(faith_command_result_t res) {

  switch (res) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_COMMAND_RESULTS(X)
#undef X
  default:
    return "FAITH_COMMAND_RESULT_UNKNOWN";
  }
}
const char *faith_command_result_err_name(faith_command_result_err_t err) {

  switch (err) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_COMMAND_RESULT_ERRS(X)
#undef X
  default:
    return "FAITH_COMMAND_ERR_UNKNOWN";
  }
}
