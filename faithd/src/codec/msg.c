#include "msg.h"

const char *faith_frame_msg_name(faith_frame_msg_type_t msg) {
  switch (msg) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_MSG_TYPES(X)
#undef X
  default:
    return "FAITH_MSG_UNKNOWN";
  }
}
