#include "core/core.h"
#include <sys/time.h>
#include <time.h>

inline uint16_t faith_version_pack(uint8_t major, uint8_t minor,
                                   uint8_t patch) {
  return ((uint16_t)(major & 0x1f) << 11) | ((uint16_t)(minor & 0x1f) << 6) |
         ((uint16_t)(patch & 0x3f));
}

inline uint8_t faith_version_major(uint16_t v) {
  return (uint8_t)((v >> 11) & 0x1f);
}

inline uint8_t faith_version_minor(uint16_t v) {
  return (uint8_t)((v >> 6) & 0x1f);
}

inline uint8_t faith_version_patch(uint16_t v) { return (uint8_t)(v & 0x3f); }
const char    *faith_status_code_name(faith_status_code_t code) {
  switch (code) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_STATUS_CODES(X)
#undef X
  default:
    return "FAITH_ERR_UNKNOWN";
  }
}
uint64_t faith_now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);

  uint64_t total_ms =
      (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
  return total_ms;
}

faith_status_code_t faith_id128_to_hex(const uint8_t bytes[16], char out[33]) {
  if (!out || !bytes)
    return FAITH_ERR_INVALID;

  for (size_t i = 0; i < 16; ++i) {
    int written = snprintf(out + (i * 2), 3, "%02x", bytes[i]);

    if (written != 2)
      return FAITH_ERR_INVALID;
  }

  return FAITH_OK;
}

void faith_log_handler(Nob_Log_Level level, const char *fmt, va_list args) {
  if (level < nob_minimal_log_level)
    return;

  const char *level_name = NULL;

  switch (level) {
  case NOB_INFO:
    level_name = "INFO";
    break;
  case NOB_WARNING:
    level_name = "WARNING";
    break;
  case NOB_ERROR:
    level_name = "ERROR";
    break;
  case NOB_NO_LOGS:
    return;
  default:
    NOB_UNREACHABLE("Nob_Log_Level");
  }

  struct timeval tv;
  gettimeofday(&tv, NULL);

  time_t    now = tv.tv_sec;
  struct tm tm_utc;

#if defined(_WIN32)
  gmtime_s(&tm_utc, &now);
#else
  gmtime_r(&now, &tm_utc);
#endif

  char timestamp[32];

  strftime(timestamp, sizeof(timestamp), "%Y-%m-%dT%H:%M:%S", &tm_utc);

  fprintf(stderr, "%s.%03ldZ [%s]%*s ", timestamp, tv.tv_usec / 1000,
          level_name, (int)(7 - strlen(level_name)), "");

  vfprintf(stderr, fmt, args);
  fprintf(stderr, "\n");
}
