#include "shared.h"

#include <openssl/ssl.h>
#include <string.h>

#define NOB_STRIP_PREFIX
#include "../nob.h" 

inline uint16_t faith_version_pack(uint8_t major, uint8_t minor, uint8_t patch) {
  return ((uint16_t)(major & 0x1f) << 11) |
    ((uint16_t)(minor & 0x1f) << 6)  |
    ((uint16_t)(patch & 0x3f));
}

inline uint8_t faith_version_major(uint16_t v) {
  return (uint8_t)((v >> 11) & 0x1f);
}

inline uint8_t faith_version_minor(uint16_t v) {
  return (uint8_t)((v >> 6) & 0x1f);
}

inline uint8_t faith_version_patch(uint16_t v) {
  return (uint8_t)(v & 0x3f);
}

const char* faith_strerror(int code) {
  return strerror(code);
}

faith_status_code_t faith_ssl_write_bytes(SSL* ssl, const uint8_t* buf, size_t size) {
  if(!ssl || !buf) 
    return FAITH_ERR_INVALID;

  size_t total = 0;

  while (total < size) {
    size_t written = 0;

    int ok = SSL_write_ex(ssl, buf + total, size - total, &written);
    if (ok <= 0) {
      int err = SSL_get_error(ssl, ok);
      (void)err;
      ERR_print_errors_fp(stderr);
      return FAITH_ERR_IO;
    }

    total += written;
  }

  return FAITH_OK;
}

faith_status_code_t faith_ssl_read_bytes(SSL* ssl, uint8_t* buf, size_t size) {
  if(!ssl || !buf) 
    return FAITH_ERR_INVALID;

  size_t total = 0;

  while (total < size) {
    size_t nread = 0;
    int ok = SSL_read_ex(ssl, buf + total, size - total, &nread);

    if (ok <= 0) {
      int err = SSL_get_error(ssl, ok);

      if (err == SSL_ERROR_ZERO_RETURN) {
        fprintf(stderr, "error closed: %li\n", nread);
        return FAITH_ERR_CLOSED;
      }

      ERR_print_errors_fp(stderr);
      fprintf(stderr, "error io: %li\n", nread);
      return FAITH_ERR_IO;
    }

    total += nread;
  }

  return FAITH_OK;
}

void faith_frame_free(faith_frame_t* f) {
  if (!f) return;

  if (f->payload) {
    free(f->payload);
    f->payload = NULL;
  }
}

faith_status_code_t faith_read_frame_ssl(SSL *ssl, faith_frame_t* out)
{
  if (!ssl || !out)
    return FAITH_ERR_INVALID;

  uint8_t len_buf[4];
  uint8_t hdr_buf[4];

  memset(out, 0, sizeof(*out));

  _FH_CHECK_RETURN(faith_ssl_read_bytes(ssl, len_buf, sizeof(len_buf)));

  uint32_t frame_size = faith_read_u32_be(len_buf);

  if (frame_size < sizeof(hdr_buf))
    return FAITH_ERR_BAD_FRAME;

  if (frame_size > FAITH_MAX_FRAME_LEN)
    return FAITH_ERR_FRAME_TOO_LARGE;

  _FH_CHECK_RETURN(faith_ssl_read_bytes(ssl, hdr_buf, sizeof(hdr_buf)));

  out->frame_size = frame_size;
  out->proto_ver = faith_read_u16_be(hdr_buf);
  out->msg_type = faith_read_u16_be(hdr_buf + sizeof(uint16_t));
  out->payload_size = frame_size - sizeof(hdr_buf);

  if (out->proto_ver != FAITH_PROTO_VERSION)
    return FAITH_ERR_UNSUPPORTED_VER;

  if (out->payload_size > FAITH_MAX_PAYLOAD_SIZE)
    return FAITH_ERR_FRAME_TOO_LARGE;

  if (out->payload_size > 0) {
    out->payload = malloc(out->payload_size);
    if (!out->payload)
      return FAITH_ERR_NOMEM;

    faith_status_code_t rc =
      faith_ssl_read_bytes(ssl, out->payload, out->payload_size);

    if (rc != FAITH_OK) {
      faith_frame_free(out);
      return rc;
    }
  }

  return FAITH_OK;
}

faith_status_code_t faith_write_frame_ssl(SSL* ssl, 
    faith_frame_msg_type_t type, uint8_t* payload, size_t payload_size) {

  if(!ssl) return FAITH_ERR_INVALID;

  const size_t header_size_bytes = 
    sizeof(uint32_t)  + /* frame size */
    sizeof(uint16_t)  + /* proto version */ 
    sizeof(uint16_t);   /* msg type */ 

  if (payload_size > FAITH_MAX_PAYLOAD_SIZE) 
    return FAITH_ERR_FRAME_TOO_LARGE;

  const size_t data_size = header_size_bytes + payload_size;
  uint8_t data[data_size];

  // Frame size specifices the size of the frame data 
  // in bytes excluding the frame size itself (4 bytes). 
  uint32_t frame_size = sizeof(uint16_t) + sizeof(uint16_t) + payload_size;

  if(frame_size > FAITH_MAX_FRAME_LEN)
    return FAITH_ERR_FRAME_TOO_LARGE;

  // Write header
  _FH_CHECK_RETURN(faith_write_u32_be(data, frame_size));
  _FH_CHECK_RETURN(faith_write_u16_be(data + sizeof(uint32_t), FAITH_PROTO_VERSION));
  _FH_CHECK_RETURN(faith_write_u16_be(data + sizeof(uint32_t) + sizeof(uint16_t), 
        (uint16_t)type));

  if (payload_size > 0 && payload == NULL) {
    return FAITH_ERR_INVALID;
  }

  // Insert payload data
  if (payload_size > 0 && payload != NULL) {
    memcpy(data + header_size_bytes, payload, payload_size);
  }

  // Write data with SSL
  _FH_CHECK_RETURN(faith_ssl_write_bytes(ssl, data, data_size));

  return FAITH_OK;
}

uint64_t faith_now_ms() {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);

  uint64_t total_ms = (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
  return total_ms;
}

inline faith_status_code_t faith_write_u64_be(
    uint8_t* out_buf, uint64_t val
    ) {
  if(!out_buf) return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline faith_status_code_t faith_write_u32_be(
    uint8_t* out_buf, uint32_t val
    ) {
  if(!out_buf) return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline faith_status_code_t faith_write_u16_be(
    uint8_t* out_buf, uint16_t val
    ) {
  if(!out_buf) return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline uint16_t faith_read_u16_be(const uint8_t *p) {
  if(!p) return 0;
  return ((uint16_t)p[0] << 8) |
    ((uint16_t)p[1]);
}

inline uint32_t faith_read_u32_be(const uint8_t *p) {
  if(!p) return 0;
  return ((uint32_t)p[0] << 24) |
    ((uint32_t)p[1] << 16) |
    ((uint32_t)p[2] << 8)  |
    ((uint32_t)p[3]);
}

inline uint64_t faith_read_u64_be(const uint8_t *p) {
  if(!p) return 0;
  return ((uint64_t)p[0] << 56) |
    ((uint64_t)p[1] << 48) |
    ((uint64_t)p[2] << 40) |
    ((uint64_t)p[3] << 32) |
    ((uint64_t)p[4] << 24) |
    ((uint64_t)p[5] << 16) |
    ((uint64_t)p[6] << 8)  |
    ((uint64_t)p[7]);
}


const char* faith_status_code_name(faith_status_code_t code) {
  switch (code) {
#define X(name, value) case name: return #name;
    FAITH_STATUS_CODES(X)
#undef X
    default:
      return "FAITH_ERR_UNKNOWN";
  }
}


