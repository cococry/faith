#include "events.h"

faith_status_code_t faith_encode_event_conversation_created(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_event_conversation_created_t *in) {
  FAITH_ENCODE_PROLOGUE(FAITH_EVENT_CONVERSATION_CREATED_DATA_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->conversation_id.bytes,
                      sizeof(in->conversation_id.bytes));

  FAITH_ENCODE_EPILOGUE(FAITH_EVENT_CONVERSATION_CREATED_DATA_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_decode_event_conversation_created(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_event_conversation_created_t *out) {
  FAITH_DECODE_PROLOGUE(FAITH_EVENT_CONVERSATION_CREATED_DATA_SIZE, !=);

  size_t offset = 0;

  FAITH_DECODE_RETURN(payload, payload_size, offset, out->conversation_id.bytes,
                      sizeof(out->conversation_id));

  FAITH_DECODE_EPILOGUE(FAITH_EVENT_CONVERSATION_CREATED_DATA_SIZE, !=);

  return FAITH_OK;
}
const char *faith_event_codec_type_name(faith_event_codec_type_t type) {
  switch (type) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_EVENTS_CODEC(X)
#undef X
  default:
    return "FAITH_EVENT_UNKNOWN";
  }
}
