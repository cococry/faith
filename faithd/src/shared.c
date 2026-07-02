#include "shared.h"

#include <openssl/rand.h>
#include <openssl/ssl.h>
#include <string.h>

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

const char *faith_strerror(int code) { return strerror(code); }

faith_status_code_t faith_write_bytes_sync(SSL *ssl, const uint8_t *buf,
                                           size_t size) {
  if (!ssl || !buf)
    return FAITH_ERR_INVALID;

  size_t total = 0;

  while (total < size) {
    size_t written = 0;

    int ok = SSL_write_ex(ssl, buf + total, size - total, &written);
    if (ok != 1) {
      int err = SSL_get_error(ssl, ok);

      if (err == SSL_ERROR_WANT_READ || err == SSL_ERROR_WANT_WRITE)
        continue;

      if (err == SSL_ERROR_ZERO_RETURN)
        return FAITH_ERR_CLOSED;

      ERR_print_errors_fp(stderr);
      return FAITH_ERR_IO;
    }

    total += written;
  }

  return FAITH_OK;
}

faith_status_code_t faith_read_bytes_sync(SSL *ssl, uint8_t *buf, size_t size) {
  if (!ssl || !buf)
    return FAITH_ERR_INVALID;

  size_t total = 0;

  while (total < size) {
    size_t nread = 0;
    int    ok = SSL_read_ex(ssl, buf + total, size - total, &nread);

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

void faith_frame_free(faith_frame_t *f) {
  if (!f)
    return;

  free(f->payload);
  memset(f, 0, sizeof(*f));
}

faith_status_code_t faith_read_frame_sync(SSL *ssl, faith_frame_t *out) {
  if (!ssl || !out)
    return FAITH_ERR_INVALID;

  uint8_t len_buf[4];
  uint8_t hdr_buf[4];

  memset(out, 0, sizeof(*out));

  _FH_CHECK_RETURN(faith_read_bytes_sync(ssl, len_buf, sizeof(len_buf)));

  uint32_t frame_size = faith_read_u32_be(len_buf);

  if (frame_size < sizeof(hdr_buf))
    return FAITH_ERR_BAD_FRAME;

  if (frame_size > FAITH_MAX_FRAME_LEN)
    return FAITH_ERR_FRAME_TOO_LARGE;

  _FH_CHECK_RETURN(faith_read_bytes_sync(ssl, hdr_buf, sizeof(hdr_buf)));

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
        faith_read_bytes_sync(ssl, out->payload, out->payload_size);

    if (rc != FAITH_OK) {
      faith_frame_free(out);
      return rc;
    }
  }

  return FAITH_OK;
}

faith_status_code_t faith_write_frame_sync(SSL                   *ssl,
                                           faith_frame_msg_type_t type,
                                           const uint8_t         *payload,
                                           size_t payload_size) {
  if (!ssl)
    return FAITH_ERR_INVALID;

  uint8_t *data = NULL;
  size_t   data_size = 0;

  faith_status_code_t rc =
      faith_encode_frame(type, payload, payload_size, &data, &data_size);

  if (rc != FAITH_OK)
    return rc;

  rc = faith_write_bytes_sync(ssl, data, data_size);

  free(data);

  if (rc != FAITH_OK) {
    nob_log(ERROR, "faith_ssl_write_bytes failed: %s (%d)",
            faith_status_code_name(rc), (int)rc);
    return rc;
  }

  return FAITH_OK;
}

faith_status_code_t faith_encode_frame(faith_frame_msg_type_t type,
                                       const uint8_t         *payload,
                                       size_t payload_size, uint8_t **out_data,
                                       size_t *out_size) {
  if (!out_data || !out_size)
    return FAITH_ERR_INVALID;

  *out_data = NULL;
  *out_size = 0;

  if (payload_size > 0 && !payload)
    return FAITH_ERR_INVALID;

  if (payload_size > FAITH_MAX_PAYLOAD_SIZE)
    return FAITH_ERR_FRAME_TOO_LARGE;

  const size_t header_size_bytes = FAITH_HEADER_SIZE;

  if (payload_size > SIZE_MAX - header_size_bytes)
    return FAITH_ERR_FRAME_TOO_LARGE;

  size_t frame_size_size_t = sizeof(uint16_t) + sizeof(uint16_t) + payload_size;

  if (frame_size_size_t > UINT32_MAX)
    return FAITH_ERR_FRAME_TOO_LARGE;

  if (frame_size_size_t > FAITH_MAX_FRAME_LEN)
    return FAITH_ERR_FRAME_TOO_LARGE;

  size_t data_size = header_size_bytes + payload_size;

  uint8_t *data = malloc(data_size);
  if (!data)
    return FAITH_ERR_NOMEM;

  faith_status_code_t rc = FAITH_OK;

  rc = faith_write_u32_be(data, (uint32_t)frame_size_size_t);
  if (rc != FAITH_OK)
    goto fail;

  rc = faith_write_u16_be(data + sizeof(uint32_t), FAITH_PROTO_VERSION);
  if (rc != FAITH_OK)
    goto fail;

  rc = faith_write_u16_be(data + sizeof(uint32_t) + sizeof(uint16_t),
                          (uint16_t)type);
  if (rc != FAITH_OK)
    goto fail;

  if (payload_size > 0)
    memcpy(data + header_size_bytes, payload, payload_size);

  *out_data = data;
  *out_size = data_size;

  return FAITH_OK;

fail:
  free(data);
  return rc;
}

uint64_t faith_now_ms() {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);

  uint64_t total_ms =
      (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
  return total_ms;
}

inline faith_status_code_t faith_write_u64_be(uint8_t *out_buf, uint64_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline faith_status_code_t faith_write_u32_be(uint8_t *out_buf, uint32_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline faith_status_code_t faith_write_u16_be(uint8_t *out_buf, uint16_t val) {
  if (!out_buf)
    return FAITH_ERR_INVALID;

  const size_t n_bytes = sizeof(val);
  for (size_t i = 0; i < n_bytes; i++) {
    out_buf[i] = (uint8_t)(val >> (n_bytes - (i + 1)) * 8);
  }

  return FAITH_OK;
}

inline uint16_t faith_read_u16_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint16_t)p[0] << 8) | ((uint16_t)p[1]);
}

inline uint32_t faith_read_u32_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
         ((uint32_t)p[2] << 8) | ((uint32_t)p[3]);
}

inline uint64_t faith_read_u64_be(const uint8_t *p) {
  if (!p)
    return 0;
  return ((uint64_t)p[0] << 56) | ((uint64_t)p[1] << 48) |
         ((uint64_t)p[2] << 40) | ((uint64_t)p[3] << 32) |
         ((uint64_t)p[4] << 24) | ((uint64_t)p[5] << 16) |
         ((uint64_t)p[6] << 8) | ((uint64_t)p[7]);
}

const char *faith_status_code_name(faith_status_code_t code) {
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

const char *faith_event_name(faith_event_type_t ev) {
  switch (ev) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    FAITH_EVENT_TYPES(X)
#undef X
  default:
    return "FAITH_EVENT_UNKNOWN";
  }
}

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

faith_status_code_t faith_encode_envelope(uint8_t *out_buf, size_t *out_size,
                                          size_t buf_cap_in_bytes,
                                          const faith_envelope_t *env) {
  if (!out_buf || !out_size || !env)
    return FAITH_ERR_INVALID;

  const size_t env_size = FAITH_ENVL_HEADER_SIZE + env->body_size;

  if (buf_cap_in_bytes < env_size)
    return FAITH_ERR_OVERFLOW;

  size_t offset = 0;
  // Type (faith_envelope_type_t -> uint32_t)
  _FH_CHECK_RETURN(faith_write_u32_be(out_buf + offset, env->type));
  offset += sizeof(uint32_t);
  // Sender (client_id_t -> 16 raw bytes)
  memcpy(out_buf + offset, env->sender_id.bytes, sizeof(env->sender_id.bytes));
  offset += sizeof(env->sender_id.bytes);
  // Recipient (client_id_t -> 16 raw bytes)
  memcpy(out_buf + offset, env->recipient_id.bytes,
         sizeof(env->recipient_id.bytes));
  offset += sizeof(env->recipient_id.bytes);
  // Body Size (uint32_t)
  _FH_CHECK_RETURN(faith_write_u32_be(out_buf + offset, env->body_size));

  if (env->body_size > 0 && env->body != NULL) {
    offset += sizeof(uint32_t);

    if (offset + env->body_size > buf_cap_in_bytes)
      return FAITH_ERR_OVERFLOW;
    memcpy(out_buf + offset, env->body, env->body_size);
  }

  *out_size = env_size;

  return FAITH_OK;
}

faith_status_code_t faith_decode_envelope(const uint8_t    *payload,
                                          size_t            payload_size,
                                          faith_envelope_t *o_envl) {

  if (!o_envl)
    return FAITH_ERR_INVALID;

  memset(o_envl, 0, sizeof(*o_envl));

  if (!payload)
    return payload_size == 0 ? FAITH_ERR_BAD_FRAME : FAITH_ERR_INVALID;

  if (payload_size < FAITH_ENVL_HEADER_SIZE)
    return FAITH_ERR_BAD_FRAME;

  size_t offset = 0;
  // Type (faith_envelope_type_t -> uint32_t)
  uint32_t type = faith_read_u32_be(payload + offset);
  offset += sizeof(uint32_t);
  // Sender (client_id_t -> 16 raw bytes)
  client_id_t sender_id;
  memcpy(sender_id.bytes, payload + offset, sizeof(sender_id.bytes));
  offset += sizeof(sender_id.bytes);
  // Recipient (client_id_t -> 16 raw bytes)
  client_id_t recipient_id;
  memcpy(recipient_id.bytes, payload + offset, sizeof(recipient_id.bytes));
  offset += sizeof(recipient_id.bytes);
  // Body Size (uint32_t)
  uint32_t body_size = faith_read_u32_be(payload + offset);

  offset += sizeof(uint32_t);

  if (body_size > payload_size - offset)
    return FAITH_ERR_BAD_FRAME;

  if (body_size != payload_size - offset)
    return FAITH_ERR_BAD_FRAME;

  uint8_t *body = NULL;
  if (body_size != 0) {
    body = malloc(body_size);
    if (!body)
      return FAITH_ERR_NOMEM;

    printf("MEMCPY: %zu, %i\n", offset, body_size);
    memcpy(body, payload + offset, body_size);
  }

  *o_envl = (faith_envelope_t){.body_size = body_size,
                               .body = body,
                               .recipient_id = recipient_id,
                               .sender_id = sender_id,
                               .type = type};

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

int faith_client_id_equal(client_id_t a, client_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
}

int faith_device_id_equal(device_id_t a, device_id_t b) {
  return memcmp(a.bytes, b.bytes, 16) == 0;
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

faith_status_code_t faith_random_bytes(uint8_t *o_buf, int num) {
  if (RAND_bytes(o_buf, num) != 1) {
    nob_log(ERROR, "Failed to generate auth_id with OpenSSL RAND_bytes()");
    return FAITH_ERR_SSL;
  }
  return FAITH_OK;
}

faith_status_code_t
faith_gen_ed25519_keypair(void   *handle,
                          uint8_t private_key[FAITH_ED25519_PRIVATE_KEY_SIZE],
                          uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]) {
  if (!handle || !private_key || !public_key)
    return FAITH_ERR_INVALID;

  faith_status_code_t status = FAITH_OK;

  EVP_PKEY    **out_keypair = (EVP_PKEY **)handle;
  EVP_PKEY     *keypair = NULL;
  EVP_PKEY_CTX *ctx = NULL;

  ctx = EVP_PKEY_CTX_new_id(EVP_PKEY_ED25519, NULL);
  if (!ctx) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  if (EVP_PKEY_keygen_init(ctx) != 1) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  if (EVP_PKEY_keygen(ctx, &keypair) != 1) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  {
    size_t priv_len = 0;

    if (EVP_PKEY_get_raw_private_key(keypair, NULL, &priv_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }

    if (priv_len != FAITH_ED25519_PRIVATE_KEY_SIZE) {
      nob_log(ERROR, "Generated private key has unexpected byte size: %zu",
              priv_len);
      status = FAITH_ERR_INVALID;
      goto cleanup;
    }

    if (EVP_PKEY_get_raw_private_key(keypair, private_key, &priv_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }
  }

  {
    size_t pub_len = 0;

    if (EVP_PKEY_get_raw_public_key(keypair, NULL, &pub_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }

    if (pub_len != FAITH_ED25519_PUBLIC_KEY_SIZE) {
      nob_log(ERROR, "Generated public key has unexpected byte size: %zu",
              pub_len);
      status = FAITH_ERR_INVALID;
      goto cleanup;
    }

    if (EVP_PKEY_get_raw_public_key(keypair, public_key, &pub_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }
  }

  *out_keypair = keypair;
  keypair = NULL;

cleanup:
  EVP_PKEY_free(keypair);
  EVP_PKEY_CTX_free(ctx);
  return status;
}
