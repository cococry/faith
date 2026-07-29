#include "codec/envelopes.h"

const char *faith_envelope_name(faith_envelope_type_t env) {
  switch (env) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_ENVELOPE_TYPES(X)
#undef X
  default:
    return "FAITH_ENVELOPE_UNKNOWN";
  }
}
