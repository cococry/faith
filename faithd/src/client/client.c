#include "client.h"
#include <arpa/inet.h>
#include <assert.h>
#include <errno.h>
#include <netinet/in.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/rand.h>
#include <openssl/ssl.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/eventfd.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

#define NOB_IMPLEMENTATION
#include "../../third_party/nob.h"

#define MAX_QUEUED_EVENTS 512

static int g_log_enable_tracing = true;

typedef struct {
  EVP_PKEY *keypair;
  uint8_t   public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  uint8_t   private_key[FAITH_ED25519_PRIVATE_KEY_SIZE];

  faith_client_id_t auth_id;
  faith_device_id_t device_id;
} client_identity_t;

struct faith_client {
  char     host[256];
  uint16_t port;

  char server_name[256];
  char ca_file[512];
  int  insecure_skip_verify;

  int       running;
  pthread_t thread;

  int sockfd;
  int event_fd;

  pthread_mutex_t lock;
  pthread_mutex_t write_lock;
  pthread_mutex_t ping_lock;

  pthread_t reader_thread;
  pthread_t pinger_thread;

  uint64_t ping_nonce;
  uint64_t ping_sent_at_ms;
  int      ping_active;

  atomic_bool connected;

  faith_event_t ev_queue[MAX_QUEUED_EVENTS];
  uint16_t      ev_queue_front;
  uint16_t      ev_queue_back;
  uint16_t      ev_queue_len;

  SSL     *ssl;
  SSL_CTX *ssl_ctx;

  uint16_t proto_ver;

  client_identity_t ident;

  faith_envl_stc_device_link_req_t *pending_device_link_req;

  uint64_t nonce_tmp;
};

static faith_status_code_t
client_push_event_ex(faith_client_t *client, faith_event_type_t type,
                     uint64_t value0, uint64_t value1, const char *message,
                     char *chat_message, size_t chat_message_size) {
  if (!client)
    return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);

  /* Enqueue event */

  if (client->ev_queue_len == MAX_QUEUED_EVENTS ||
      client->ev_queue_back > MAX_QUEUED_EVENTS - 1) {
    pthread_mutex_unlock(&client->lock);
    nob_log(ERROR, "Event queue overflow.");
    return FAITH_ERR_OVERFLOW;
  }

  faith_event_t ev;
  ev.value0 = value0;
  ev.value1 = value1;
  ev.type = type;
  ev.chat_message = chat_message;
  ev.chat_message_size = chat_message_size;
  if (message != NULL) {
    snprintf(ev.message, sizeof(ev.message), "%s", message);
  } else {
    ev.message[0] = '\0';
  }

  client->ev_queue[client->ev_queue_back] = ev;
  client->ev_queue_back = (client->ev_queue_back + 1) % MAX_QUEUED_EVENTS;
  client->ev_queue_len++;

  uint64_t one = 1;
  ssize_t  wr = write(client->event_fd, &one, sizeof(one));
  if (wr < 0 && errno != EAGAIN) {
    nob_log(ERROR, "eventfd write failed: %s", strerror(errno));
  }

  pthread_mutex_unlock(&client->lock);

  return FAITH_OK;
}

static faith_status_code_t client_push_event(faith_client_t    *client,
                                             faith_event_type_t type,
                                             uint64_t value0, uint64_t value1,
                                             const char *message) {
  return client_push_event_ex(client, type, value0, value1, message, NULL, 0);
}

static int client_is_running(faith_client_t *client) {
  int running;

  pthread_mutex_lock(&client->lock);
  running = client->running;
  pthread_mutex_unlock(&client->lock);

  return running;
}

static int client_init_sock(const char *host, int port) {
  int                sockfd = -1;
  struct sockaddr_in serveraddr;

  sockfd = socket(AF_INET, SOCK_STREAM, 0);
  if (sockfd < 0) {
    nob_log(ERROR, "Client socket creation failed: %s", strerror(errno));
    return -1;
  }

  memset(&serveraddr, 0, sizeof(serveraddr));
  serveraddr.sin_family = AF_INET;
  serveraddr.sin_port = htons((uint16_t)port);

  if (inet_pton(AF_INET, host, &serveraddr.sin_addr) != 1) {
    nob_log(ERROR, "Invalid IPv4 address: %s", host);
    close(sockfd);
    return -1;
  }

  if (connect(sockfd, (struct sockaddr *)&serveraddr, sizeof(serveraddr)) < 0) {
    nob_log(ERROR, "Connection to server failed: %s", strerror(errno));
    close(sockfd);
    return -1;
  }

  if (g_log_enable_tracing)
    nob_log(INFO, "[faith] Client connected");

  return sockfd;
}

static void _sleep_ms(unsigned ms) {
  struct timespec ts;

  ts.tv_sec = ms / 1000;
  ts.tv_nsec = (long)(ms % 1000) * 1000000L;

  while (nanosleep(&ts, &ts) < 0 && errno == EINTR)
    ;
}

static void _sleep_heartbeat_interval(faith_client_t *client) {
  for (int i = 0; i < 100; i++) {
    if (!client_is_running(client) || !atomic_load(&client->connected))
      return;

    _sleep_ms(100);
  }
}

static uint32_t client_next_backoff_ms(uint32_t current) {
  if (current < 250)
    return 250;

  if (current >= 30000)
    return 30000;

  current *= 2;

  if (current > 30000)
    return 30000;

  return current;
}

static SSL_CTX *client_create_ssl_ctx(faith_client_t *client) {
  SSL_CTX *ctx = NULL;

  ctx = SSL_CTX_new(TLS_client_method());
  if (ctx == NULL) {
    nob_log(ERROR, "failed to create client SSL_CTX");
    ERR_print_errors_fp(stderr);
    return NULL;
  }

  if (!SSL_CTX_set_min_proto_version(ctx, TLS1_2_VERSION)) {
    nob_log(ERROR, "failed to set minimum TLS version");
    ERR_print_errors_fp(stderr);
    SSL_CTX_free(ctx);
    return NULL;
  }

  long opts = SSL_OP_NO_RENEGOTIATION;
  SSL_CTX_set_options(ctx, opts);

  if (client->insecure_skip_verify) {
    // nob_log(WARNING, "[faith]: TLS certificate verification is disabled");
    SSL_CTX_set_verify(ctx, SSL_VERIFY_NONE, NULL);
    return ctx;
  }

  SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, NULL);

  if (client->ca_file[0] != '\0') {
    if (SSL_CTX_load_verify_locations(ctx, client->ca_file, NULL) != 1) {
      nob_log(ERROR, "failed to load CA file: %s", client->ca_file);
      ERR_print_errors_fp(stderr);
      SSL_CTX_free(ctx);
      return NULL;
    }
  } else {
    if (SSL_CTX_set_default_verify_paths(ctx) != 1) {
      nob_log(ERROR, "failed to load default CA paths");
      ERR_print_errors_fp(stderr);
      SSL_CTX_free(ctx);
      return NULL;
    }
  }

  return ctx;
}

static SSL *client_create_ssl_conn(faith_client_t *client, SSL_CTX *ctx,
                                   int sockfd) {
  if (!client || !ctx || sockfd < 0)
    return NULL;

  SSL *ssl = NULL;

  ssl = SSL_new(ctx);
  if (ssl == NULL) {
    nob_log(ERROR, "Failed to create SSL object for client");
    return NULL;
  }

  if (SSL_set_fd(ssl, sockfd) != 1) {
    nob_log(ERROR, "Failed to set FD of SSL connection for client");
    return NULL;
  }

  if (!client->insecure_skip_verify && client->server_name[0] != '\0') {
    if (SSL_set_tlsext_host_name(ssl, client->server_name) != 1) {
      nob_log(ERROR, "Failed to set TLS hostname to '%s'", client->server_name);
      return NULL;
    }

    if (SSL_set1_host(ssl, client->server_name) != 1) {
      nob_log(ERROR, "Failed to set TLS hostname to '%s'", client->server_name);
      return NULL;
    }
  }

  if (SSL_connect(ssl) <= 0) {
    nob_log(ERROR, "TLS handshake failed");
    return NULL;
  }

  return ssl;
}

static void client_cleanup_connection(faith_client_t *client) {
  if (!client) {
    return;
  }

  pthread_mutex_lock(&client->write_lock);

  if (client->ssl) {
    SSL_free(client->ssl);
    client->ssl = NULL;
  }

  if (client->ssl_ctx) {
    SSL_CTX_free(client->ssl_ctx);
    client->ssl_ctx = NULL;
  }

  pthread_mutex_unlock(&client->write_lock);

  pthread_mutex_lock(&client->lock);
  int fd = client->sockfd;
  client->sockfd = -1;
  pthread_mutex_unlock(&client->lock);

  if (fd >= 0) {
    close(fd);
  }
}

static faith_status_code_t client_init_ssl(faith_client_t *client) {
  if (!client)
    return FAITH_ERR_INVALID;

  int fd = client_init_sock(client->host, client->port);

  if (fd < 0) {
    client_push_event(client, FAITH_EVENT_ERROR, 0, 0,
                      "Client connection failed. Server down?");
    return FAITH_ERR_IO;
  }

  pthread_mutex_lock(&client->lock);
  client->sockfd = fd;
  pthread_mutex_unlock(&client->lock);

  SSL_CTX *ctx = NULL;
  ctx = client_create_ssl_ctx(client);
  if (!ctx) {
    client_push_event(client, FAITH_EVENT_ERROR, 0, 0,
                      "TLS context creation failed");
    return FAITH_ERR_SSL;
  }
  SSL *ssl = client_create_ssl_conn(client, ctx, fd);
  if (!ssl) {
    client_push_event(client, FAITH_EVENT_ERROR, 0, 0, "TLS connection failed");
    return FAITH_ERR_SSL;
  }

  client->ssl = ssl;
  client->ssl_ctx = ctx;

  return FAITH_OK;
}

static faith_status_code_t encode_ping(uint8_t *out_buf, size_t *out_size,
                                       size_t buf_cap_in_bytes, uint64_t nonce,
                                       uint64_t sent_at_ms) {
  if (!out_buf || !out_size)
    return FAITH_ERR_INVALID;

  const size_t ping_size = sizeof(uint64_t) * 2;

  if (buf_cap_in_bytes < ping_size)
    return FAITH_ERR_OVERFLOW;

  _FH_CHECK_RETURN(faith_write_u64_be(out_buf, nonce));
  _FH_CHECK_RETURN(faith_write_u64_be(out_buf + sizeof(uint64_t), sent_at_ms));

  *out_size = ping_size;

  return FAITH_OK;
}

static faith_status_code_t decode_pong(const uint8_t *payload,
                                       size_t         payload_size,
                                       uint64_t      *pong_nonce,
                                       uint64_t      *server_time_ms) {
  const size_t pong_size = sizeof(uint64_t) * 2;

  if (payload == NULL || pong_nonce == NULL || server_time_ms == NULL)
    return FAITH_ERR_INVALID;

  if (payload_size != pong_size)
    return FAITH_ERR_BAD_FRAME;

  *pong_nonce = faith_read_u64_be(payload);
  *server_time_ms = faith_read_u64_be(payload + sizeof(uint64_t));

  return FAITH_OK;
}

static faith_status_code_t client_send_ping_sync(SSL *ssl, uint64_t nonce,
                                                 uint64_t sent_at_ms) {

  if (!ssl)
    return FAITH_ERR_INVALID;

  uint8_t payload[sizeof(uint64_t) * 2];
  size_t  payload_size = 0;

  _FH_CHECK_RETURN(
      encode_ping(payload, &payload_size, sizeof(payload), nonce, sent_at_ms));

  _FH_CHECK_RETURN(
      faith_write_frame_sync(ssl, FAITH_MSG_PING, payload, payload_size));

  return FAITH_OK;
}

static void *pinger(void *arg) {
  faith_client_t *client = arg;
  while (atomic_load(&client->connected) && client_is_running(client)) {
    uint64_t sent_at_ms = faith_now_ms();
    uint64_t nonce;
    _FH_CHECK(faith_random_bytes((uint8_t *)&nonce, sizeof(nonce)));
    if (_fh_rc != FAITH_OK) {
      goto fail;
    }

    pthread_mutex_lock(&client->ping_lock);
    client->ping_active = true;
    client->ping_sent_at_ms = sent_at_ms;
    client->ping_nonce = nonce;
    pthread_mutex_unlock(&client->ping_lock);

    if (g_log_enable_tracing)
      nob_log(INFO, "[client - pinger]: Sending PING to server...");

    pthread_mutex_lock(&client->write_lock);
    faith_status_code_t rc =
        client_send_ping_sync(client->ssl, nonce, sent_at_ms);
    pthread_mutex_unlock(&client->write_lock);

    if (rc != FAITH_OK) {
      goto fail;
    }

    if (g_log_enable_tracing)
      nob_log(INFO, "[client - pinger]: Sent PING to server successfully.");

    // 10 seconds after each ping
    _sleep_heartbeat_interval(client);

    continue;
  fail: {
    _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                "Failed to send ping"));
    atomic_store(&client->connected, false);
    break;
  }
  }

  return NULL;
}

static void client_clear_pending_device_link_request(faith_client_t *client) {
  if (!client)
    return;

  free(client->pending_device_link_req);
  client->pending_device_link_req = NULL;
}

static void client_handle_pong(faith_client_t *client, faith_frame_t *frame) {
  uint64_t pong_nonce;
  uint64_t server_time_ms;

  _FH_CHECK(decode_pong(frame->payload, frame->payload_size, &pong_nonce,
                        &server_time_ms));

  pthread_mutex_lock(&client->ping_lock);
  int ok = _fh_rc == FAITH_OK && pong_nonce == client->ping_nonce &&
           client->ping_active;
  uint64_t ping_sent_at_ms = client->ping_sent_at_ms;
  if (ok) {
    client->ping_active = false;
  }
  pthread_mutex_unlock(&client->ping_lock);

  if (ok) {
    if (g_log_enable_tracing)
      nob_log(INFO, "[client - pinger]: Heartbeat successful.");
    uint64_t now = faith_now_ms();
    _FH_CHECK(client_push_event(client, FAITH_EVENT_PONG, now - ping_sent_at_ms,
                                server_time_ms, NULL));
  } else {
    _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                "Invalid pong response from server"));
    atomic_store(&client->connected, false);
  }
}

static faith_status_code_t client_handle_envelope(faith_client_t *client,
                                                  faith_frame_t  *frame) {
  faith_envelope_t envl;
  _FH_CHECK(faith_decode_envelope(frame->payload, frame->payload_size, &envl));
  switch (envl.type) {
  case FAITH_ENVELOPE_HELLO_OK: {
    if (envl.body_size != 0)
      return FAITH_ERR_BAD_ENVELOPE;

    _FH_CHECK_RETURN(client_push_event_ex(
        client, FAITH_EVENT_AUTHORIZED, 0, 0,
        "The client was successfully authorized.", NULL, 0));

    break;
  }
  case FAITH_ENVELOPE_MSG_SEND: {
    if (!faith_client_id_equal(envl.recipient_id, client->ident.auth_id) ||
        faith_client_id_equal(envl.sender_id, FAITH_CLIENT_ID_NONE)) {
      nob_log(ERROR, "[client - reader] Got sent invalid MESSAGE envelope.");
      return _fh_rc;
    }

    if (envl.body_size > FAITH_MAX_MSG_SIZE) {
      nob_log(ERROR, "[client - reader] Got sent too large message.");
      return FAITH_ERR_OVERFLOW;
    }
    char *msg = malloc(envl.body_size + 1);
    if (!msg) {
      nob_log(
          ERROR,
          "[client - reader] Failed to allocate memory for received message.");
      return FAITH_ERR_NOMEM;
    }

    memcpy(msg, envl.body, envl.body_size);
    msg[envl.body_size] = '\0';

    _FH_CHECK_RETURN(client_push_event_ex(client, FAITH_EVENT_MESSAGE_RECEIVED,
                                          0, 0, NULL, msg, envl.body_size));

    break;
  }
  case FAITH_ENVELOPE_DEVICE_LINK_REQUEST: {
    /* Client-side rejection of multiple pending device link requests. TOOD:
     * This has to also be server enforced.*/
    if (client->pending_device_link_req != NULL) {
      if(g_log_enable_tracing) {
        nob_log(WARNING, "[client - reader] Rejecting new DEVICE_LINK_REQUEST; "
                         "A link request is already pending for this client.");
      }
      return FAITH_OK;
    }

    if (!faith_client_id_equal(envl.recipient_id, client->ident.auth_id)) {
      char recipient_id_hex[33];
      char auth_id_hex[33];
      _FH_CHECK_RETURN(
          faith_id128_to_hex(envl.recipient_id.bytes, recipient_id_hex));
      _FH_CHECK_RETURN(
          faith_id128_to_hex(client->ident.auth_id.bytes, auth_id_hex));

      nob_log(ERROR,
              "[client - reader] Got sent invalid DEVICE_LINK_REQUEST envelope. Expected "
              "envelope recipient_id to be %s but got %s.",
              auth_id_hex, recipient_id_hex);
      return _fh_rc;
    }

    // Allocate pending device link request
    faith_envl_stc_device_link_req_t *req = calloc(1, sizeof(*req));
    if (!req)
      return FAITH_ERR_NOMEM;

    {
      _FH_CHECK(
          faith_decode_device_link_req_body(envl.body, envl.body_size, req));
      if (_fh_rc != FAITH_OK) {
        free(req);
        return _fh_rc;
      }
    }

    client->pending_device_link_req = req;

    char msg[512];
    char device_id_hex[33];

    _FH_CHECK_RETURN(
        faith_id128_to_hex(req->device_id_new.bytes, device_id_hex));

    snprintf(msg, sizeof(msg),
             "A new device requested to log in (device_id=%s). Accept?",
             device_id_hex);

    _FH_CHECK_RETURN(client_push_event(
        client, FAITH_EVENT_DEVICE_LINK_REQUEST, 0, 0, msg)); 
    break;
  }
  case FAITH_ENVELOPE_DEVICE_AUTH_RESPONSE_ACK: {
    if (envl.body_size != 0)
      return FAITH_ERR_BAD_ENVELOPE;

    if (!client->pending_device_link_req)
      break;

    client_clear_pending_device_link_request(client);

    _FH_CHECK_RETURN(client_push_event(
        client, FAITH_EVENT_DEVICE_AUTH_RESPONSE_ACK, 0, 0,
        "The pending device-link request was answered by another authorized "
        "device."));
    break;
  }

  case FAITH_ENVELOPE_DEVICE_LINK_CANCELLED: {
    if (envl.body)
      return FAITH_ERR_BAD_ENVELOPE;
    if (!client->pending_device_link_req)
      break;

    client_clear_pending_device_link_request(client);

    _FH_CHECK_RETURN(client_push_event(
        client, FAITH_EVENT_DEVICE_LINK_CANCELLED, 0, 0,
        "The pending device-link request was cancelled because the requesting "
        "device disconnected."));
    break;
  }
  default:
    break;
  }

  return FAITH_OK;
}

static void *reader(void *arg) {
  faith_client_t *client = arg;
  while (atomic_load(&client->connected) && client_is_running(client)) {
    if (g_log_enable_tracing)
      nob_log(INFO, "[client - reader]: Waiting on server frame ...");

    faith_frame_t frame;
    if (faith_read_frame_sync(client->ssl, &frame) != FAITH_OK) {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "Failed to read server frame"));
      break;
    }

    switch (frame.msg_type) {
    case FAITH_MSG_PONG: {
      client_handle_pong(client, &frame);
      break;
    }
    case FAITH_MSG_ENVL: {
      printf("HANDLING ENVELOPE.\n");
      client_handle_envelope(client, &frame);
      break;
    }
    default:
      break;
    }

    if (g_log_enable_tracing)
      nob_log(INFO, "[client - reader]: Read server frame successfully.\n");

    faith_frame_free(&frame);
  }

  return NULL;
}

static void client_run_connected(faith_client_t *client) {
  if (!client || !client->ssl)
    return;

  atomic_store(&client->connected, true);

  if (pthread_create(&client->reader_thread, NULL, reader, client) != 0) {
    faith_client_destroy(client);
    return;
  }
  if (pthread_create(&client->pinger_thread, NULL, pinger, client) != 0) {
    faith_client_destroy(client);
    return;
  }

  pthread_join(client->reader_thread, NULL);

  atomic_store(&client->connected, false);

  pthread_join(client->pinger_thread, NULL);
}

static faith_status_code_t client_send_envelope(SSL                    *ssl,
                                                const faith_envelope_t *envl) {
  if (!ssl || !envl)
    return FAITH_ERR_INVALID;

  if (envl->body_size > UINT32_MAX - FAITH_ENVL_HEADER_SIZE)
    return FAITH_ERR_OVERFLOW;

  size_t cap = FAITH_ENVL_HEADER_SIZE + envl->body_size;

  uint8_t *payload = malloc(cap);
  if (!payload)
    return FAITH_ERR_NOMEM;

  size_t payload_size = 0;

  faith_status_code_t rc;
  {
    _FH_CHECK(faith_encode_envelope(payload, &payload_size, cap, envl));
    rc = _fh_rc;
  }

  if (rc == FAITH_OK) {
    _FH_CHECK(
        faith_write_frame_sync(ssl, FAITH_MSG_ENVL, payload, payload_size));
    rc = _fh_rc;
  }

  free(payload);
  return rc;
}

static faith_status_code_t
client_send_envelope_locked(faith_client_t         *client,
                            const faith_envelope_t *envl) {
  if (!client || !envl) {
    return FAITH_ERR_INVALID;
  }

  pthread_mutex_lock(&client->write_lock);

  if (!atomic_load(&client->connected) || !client->ssl) {
    pthread_mutex_unlock(&client->write_lock);
    return FAITH_ERR_NOT_CONNECTED;
  }

  _FH_CHECK(client_send_envelope(client->ssl, envl));
  faith_status_code_t rc = _fh_rc;

  if (rc != FAITH_OK) {
    nob_log(ERROR,
            "[client] Failed to send envelope (type=%s, body_size=%" PRIu32
            ") to server.",
            faith_envelope_name(envl->type), envl->body_size);
  }

  pthread_mutex_unlock(&client->write_lock);
  return rc;
}

static faith_status_code_t faith_client_send_hello(faith_client_t *client) {
  if (!client)
    return FAITH_ERR_INVALID;

  uint8_t body[FAITH_ENVL_HELLO_BODY_SIZE];

  // HELLO {
  // header: {
  // sender_id: auth_id
  // }
  //  body: {
  //    device_id,
  //    public_key,
  //    client_nonce
  //  }
  // }
  //

  size_t offset = 0;
  size_t device_id_size = sizeof(client->ident.device_id.bytes);
  /* 1. Serialize device ID */
  memcpy(body, client->ident.device_id.bytes, device_id_size);
  offset += device_id_size;

  /* 2. Serialize public key*/
  memcpy(body + offset, client->ident.public_key,
         sizeof(client->ident.public_key));

  offset += sizeof(client->ident.public_key);

  /* 3. Serialize nonce */
  uint64_t nonce;
  _FH_CHECK_RETURN(faith_random_bytes((uint8_t *)&nonce, sizeof(nonce)));

  _FH_CHECK_RETURN(faith_write_u64_be(body + offset, nonce));

  /* Store nonce in temporary client state */
  client->nonce_tmp = nonce;

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_HELLO;

  memcpy(envl.sender_id.bytes, client->ident.auth_id.bytes,
         sizeof(envl.sender_id));

  envl.body = body;
  envl.body_size = sizeof(body);

  return client_send_envelope_locked(client, &envl);
}

static faith_status_code_t
client_handle_challenge(faith_client_t   *client,
                        faith_envelope_t *challenge_envl) {
  if (!client || !challenge_envl)
    return FAITH_ERR_INVALID;

  int ok = challenge_envl->type == FAITH_ENVELOPE_CHALLENGE &&
           challenge_envl->body &&
           challenge_envl->body_size == FAITH_ENVL_HELLO_CHALLENGE_BODY_SIZE;
  if (!ok) {
    nob_log(ERROR,
            "Unexpected or malformed server envelope response to HELLO "
            "envelope. Expected: "
            "type=FAITH_ENVELOPE_CHALLENGE, body_size=%" PRIu32
            ", Got: type=%s, body_size=%" PRIu32,
            FAITH_ENVL_HELLO_CHALLENGE_BODY_SIZE,
            faith_envelope_name(challenge_envl->type),
            challenge_envl->body_size);
    return FAITH_ERR_INVALID;
  }

  /* Bail if the CHALLENGE recipient ID does not equal the client auth ID */
  if (!faith_client_id_equal(challenge_envl->recipient_id,
                             client->ident.auth_id)) {
    char auth_id_hex[33];
    char recipient_id_hex[33];

    _FH_CHECK_RETURN(
        faith_id128_to_hex(client->ident.auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(faith_id128_to_hex(challenge_envl->recipient_id.bytes,
                                        recipient_id_hex));

    nob_log(ERROR,
            "Invalid Auth ID in CHALLENGE envelope. Recipient auth ID (%s) "
            "does not "
            "match our own one (%s).",
            recipient_id_hex, auth_id_hex);
    return FAITH_ERR_INVALID;
  }

  /* 1. Read CHALLENGE contents from server: 64 bit integer server nonce */

  uint64_t server_nonce = faith_read_u64_be(challenge_envl->body);

  /* 2. Construct message buffer for signature generation
   *    Message Buffer {
   *      client_id,
   *      public_key,
   *      client_nonce,
   *      server_nonce
   *    } */

  faith_signature_hello_handshake_t sign_msg = {0};

  sign_msg.auth_id = client->ident.auth_id;
  sign_msg.device_id = client->ident.device_id;

  memcpy(sign_msg.public_key, client->ident.public_key,
         FAITH_ED25519_PUBLIC_KEY_SIZE);

  sign_msg.client_nonce = client->nonce_tmp;
  sign_msg.server_nonce = server_nonce;

  uint8_t msg_buf[FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE];
  {
    _FH_CHECK(faith_gen_sign_buf_hello_handshake(msg_buf, sizeof(msg_buf),
                                                 &sign_msg));

    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR,
              "Failed to generate HELLO handshake signing message buffer.");
      return _fh_rc;
    }
  }

  /* 3. Generating the 64 byte cryptographic signature from the message buffer
   */
  uint8_t signature[FAITH_ED25519_SIGNATURE_SIZE] = {0};
  size_t  signature_size = 0;
  {

    /* This returns FAITH_ERR_SSL if signature_size !=
     * FAITH_ED25519_SIGNATURE_SIZE */
    _FH_CHECK(faith_gen_signature(client->ident.keypair, signature,
                                  &signature_size, msg_buf, sizeof(msg_buf)));
    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR, "Failed to generate signature for CHALLENGE_RESPONSE.");
      return _fh_rc;
    }
  }

  /* 4. Send CHALLENGE_RESPONSE back to server. Contains 64 byte client
   * signature in envelope body*/

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_CHALLENGE_RESPONSE;
  envl.sender_id = client->ident.auth_id;

  envl.body = signature;
  envl.body_size = sizeof(signature);

  return client_send_envelope_locked(client, &envl);
}

static faith_status_code_t client_make_handshake(faith_client_t *client) {
  if (!client)
    return FAITH_ERR_INVALID;

  if (g_log_enable_tracing)
    nob_log(INFO, "[client] Sending HELLO...");

  faith_status_code_t _fh_result = FAITH_OK;

  /* 1. Send HELLO envelope to the server */
  _FH_CHECK_RETURN(faith_client_send_hello(client));

  /* If successful, push CONNECTED event */
  _FH_CHECK_RETURN(
      client_push_event(client, FAITH_EVENT_CONNECTED, 0, 0, "The client successfully connected to the server."));

  if (g_log_enable_tracing)
    nob_log(INFO, "[client] Sent HELLO.");

  /* 2. Wait for server response by synchronously reading the next server
   * frame. An evelope frame of type CHALLENGE is expected. */
  if (g_log_enable_tracing)
    nob_log(INFO, "[client] Waiting for server CHALLENGE response ...");

  faith_frame_t frame = {0};

  _FH_CHECK_RETURN(faith_read_frame_sync(client->ssl, &frame));

  if (frame.msg_type != FAITH_MSG_ENVL)
    _FH_RETURN_DEFER(FAITH_ERR_INVALID);

  faith_envelope_t envl;
  _FH_CHECK_DEFER(
      faith_decode_envelope(frame.payload, frame.payload_size, &envl));

  /* 3. Handle the CHALLENGE envelope sent by the server. This verifies the type
   * of the envelope (ensures CHALLENGE). */
  _FH_CHECK_DEFER(client_handle_challenge(client, &envl));

  faith_frame_free(&frame);

  /* ========================== */
  /* Read next frame */
  /* ========================== */

  _FH_CHECK_RETURN(faith_read_frame_sync(client->ssl, &frame));

  _FH_CHECK_DEFER(
      faith_decode_envelope(frame.payload, frame.payload_size, &envl));

  switch (envl.type) {
  case FAITH_ENVELOPE_HELLO_OK: {
    /* Authorization successful */
    if (g_log_enable_tracing)
      nob_log(INFO,
              "[client] Got HELLO_OK response. Server acknowledged client "
              "successfully.");

    _FH_CHECK_RETURN(
        client_push_event(client, FAITH_EVENT_AUTHORIZED, 0, 0,
                          "The client was successfully authorized."));
    _FH_RETURN_DEFER(FAITH_OK);
  }
  case FAITH_ENVELOPE_DEVICE_AUTH_PENDING: {
    /* Authorization pending */
    if (g_log_enable_tracing)
      nob_log(
          INFO,
          "Device authorization pending. Waiting for an already authorization "
          "device to respond to the request.");

    _FH_CHECK_RETURN(client_push_event(
        client, FAITH_EVENT_DEVICE_AUTH_PENDING, 0, 0,
        "Client is waiting for device-link response by authorized device."));
    _FH_RETURN_DEFER(FAITH_OK);
  }
  default:
    nob_log(ERROR,
            "Received invalid server envelope response to "
            "CHALLENGE_RESPONSE. Got %s",
            faith_envelope_name(envl.type));
    _FH_RETURN_DEFER(FAITH_ERR_INVALID);
  }

  return FAITH_OK;

defer:
  faith_frame_free(&frame);
  return _fh_result;
}

static void *faith_client_thread_routine(void *arg) {
  faith_client_t *client = arg;
  uint32_t        backoff_ms = 250;

  while (client_is_running(client)) {
    {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_CONNECTING, 0, 0, NULL));
    }

    /* Init SSL for client connection (SSL* and SSL_CTX*) */
    {
      _FH_CHECK(client_init_ssl(client));
      if (_fh_rc != FAITH_OK) {
        ERR_print_errors_fp(stderr);
        goto fail;
      }
    }

    atomic_store(&client->connected, true);

    backoff_ms = 250;

    /* Make protocol handshake with server */
    {
      _FH_CHECK(client_make_handshake(client));
      if (_fh_rc != FAITH_OK) {
        goto fail;
      }
    }

    /* Main client loop, starts reader_thread and pinger_thread */
    client_run_connected(client);

    atomic_store(&client->connected, false);

    /* Send DISCONNECTED event when client closed */
    {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "connection closed"));
      if (_fh_rc != FAITH_OK) {
        goto fail;
      }
    }

  fail:
    client_cleanup_connection(client);

    if (client_is_running(client)) {
      _sleep_ms(backoff_ms);
      backoff_ms = client_next_backoff_ms(backoff_ms);
    }

    continue;
  }

  return NULL;
}

static void ignore_sigpipe(void) {
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));
  sa.sa_handler = SIG_IGN;

  sigemptyset(&sa.sa_mask);
  sa.sa_flags = 0;

  sigaction(SIGPIPE, &sa, NULL);
}

faith_status_code_t faith_client_init_global(int log_enable_tracing) {
  ignore_sigpipe();

  g_log_enable_tracing = log_enable_tracing;

  return FAITH_OK;
}


static bool hex_char_to_nibble(char c, uint8_t *out) {
  if (!out)
    return false;

  if (c >= '0' && c <= '9') {
    *out = (uint8_t)(c - '0');
    return true;
  }

  if (c >= 'a' && c <= 'f') {
    *out = (uint8_t)(c - 'a' + 10);
    return true;
  }

  if (c >= 'A' && c <= 'F') {
    *out = (uint8_t)(c - 'A' + 10);
    return true;
  }

  return false;
}

static bool client_id_from_hex(const char *hex, faith_client_id_t *out) {
  if (!hex || !out)
    return false;

  if (strlen(hex) != FAITH_CLIENT_ID_SIZE * 2)
    return false;

  faith_client_id_t id = {0};

  for (size_t i = 0; i < FAITH_CLIENT_ID_SIZE; ++i) {
    uint8_t hi = 0;
    uint8_t lo = 0;

    if (!hex_char_to_nibble(hex[i * 2 + 0], &hi))
      return false;

    if (!hex_char_to_nibble(hex[i * 2 + 1], &lo))
      return false;

    id.bytes[i] = (uint8_t)((hi << 4) | lo);
  }

  *out = id;
  return true;
}


static faith_status_code_t client_new_identity(client_identity_t *o_ident) {
  if (!o_ident)
    return FAITH_ERR_INVALID;

  /* Generate 128 bit random device & auth identities */

  client_id_from_hex("887f50a5b53f93c3b983b10bb9d74a10", &o_ident->auth_id);

  _FH_CHECK_RETURN(faith_random_bytes(o_ident->device_id.bytes, sizeof(o_ident->device_id.bytes)));

  /* Generate client identity keypair */
  _FH_CHECK_RETURN(faith_gen_ed25519_keypair(
      &o_ident->keypair, o_ident->private_key, o_ident->public_key));

  if(g_log_enable_tracing) {
    char auth_id_hex[33];
    char device_id_hex[33];
    _FH_CHECK_RETURN(faith_id128_to_hex(o_ident->auth_id.bytes, auth_id_hex));
    _FH_CHECK_RETURN(
        faith_id128_to_hex(o_ident->device_id.bytes, device_id_hex));
    nob_log(INFO, "[faith] Generated new client identity (auth_id=%s, device_id=%s).",
        auth_id_hex, device_id_hex);
  }

  return FAITH_OK;
}

faith_client_t *faith_client_create(const faith_client_config_t *cfg) {
  if (!cfg || !cfg->host)
    return NULL;

  faith_client_t *client = calloc(1, sizeof(*client));
  if (!client)
    return NULL;

  nob_set_log_handler(faith_log_handler);

  client->sockfd = -1;
  client->event_fd = -1;

  snprintf(client->host, sizeof(client->host), "%s", cfg->host);
  client->port = cfg->port;

  if (cfg->server_name) {
    snprintf(client->server_name, sizeof(client->server_name), "%s",
             cfg->server_name);
  }

  if (cfg->ca_file) {
    snprintf(client->ca_file, sizeof(client->ca_file), "%s", cfg->ca_file);
  }

  client->insecure_skip_verify = cfg->insecure_skip_verify;

  if (pthread_mutex_init(&client->lock, NULL) != 0)
    goto fail_client;

  if (pthread_mutex_init(&client->write_lock, NULL) != 0)
    goto fail_lock;

  if (pthread_mutex_init(&client->ping_lock, NULL) != 0)
    goto fail_write_lock;

  client->event_fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
  if (client->event_fd < 0)
    goto fail_ping_lock;

  _FH_CHECK(client_new_identity(&client->ident));
  if (_fh_rc != FAITH_OK) {
    goto fail_event_fd;
  }

  return client;

fail_event_fd:
  close(client->event_fd);

fail_ping_lock:
  pthread_mutex_destroy(&client->ping_lock);

fail_write_lock:
  pthread_mutex_destroy(&client->write_lock);

fail_lock:
  pthread_mutex_destroy(&client->lock);

fail_client:
  free(client);
  return NULL;
}

void faith_client_destroy(faith_client_t *client) {
  if (!client)
    return;

  faith_client_stop(client);

  pthread_mutex_lock(&client->lock);
  if (client->sockfd >= 0) {
    close(client->sockfd);
    client->sockfd = -1;
  }
  pthread_mutex_unlock(&client->lock);

  if (client->event_fd >= 0) {
    close(client->event_fd);
    client->event_fd = -1;
  }

  pthread_mutex_destroy(&client->write_lock);
  pthread_mutex_destroy(&client->ping_lock);
  pthread_mutex_destroy(&client->lock);

  free(client);
  client = NULL;
}

faith_status_code_t faith_client_start(faith_client_t *client) {
  if (!client)
    return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);

  if (client->running) {
    pthread_mutex_unlock(&client->lock);
    return FAITH_ERR_ALREADY_STARTED;
  }

  client->running = 1;

  if (pthread_create(&client->thread, NULL, faith_client_thread_routine,
                     client) != 0) {

    client->running = 0;
    pthread_mutex_unlock(&client->lock);
    return FAITH_ERR_THREAD;
  }
  pthread_mutex_unlock(&client->lock);

  return FAITH_OK;
}

faith_status_code_t faith_client_stop(faith_client_t *client) {
  if (!client) {
    return FAITH_ERR_INVALID;
  }

  if (pthread_equal(pthread_self(), client->thread)) {
    nob_log(ERROR, "faith_client_stop called from client thread."
                   "Use faith_client_cleanup_connection instead.");
    return FAITH_ERR_INVALID;
  }

  pthread_mutex_lock(&client->lock);

  int was_running = client->running;
  client->running = 0;
  int fd = client->sockfd;

  pthread_mutex_unlock(&client->lock);

  if (was_running && fd >= 0) {
    shutdown(fd, SHUT_RDWR);
  }

  if (was_running) {
    int err = pthread_join(client->thread, NULL);
    if (err != 0) {
      nob_log(ERROR,
              "Failed to join client thread during client stop: "
              "pthread_join returned %d.",
              err);
      return FAITH_ERR_THREAD;
    }
  }

  client_cleanup_connection(client);

  return FAITH_OK;
}

faith_status_code_t
faith_client_send_message(faith_client_t   *client,
                          faith_client_id_t recipient_auth_id,
                          const char       *msg) {
  if (!client ||
      faith_client_id_equal(recipient_auth_id, FAITH_CLIENT_ID_NONE) || !msg)
    return FAITH_ERR_INVALID;

  /* This does not include the null terminator of the string. */
  size_t msg_size = strlen(msg);

  if (msg_size == 0)
    return FAITH_ERR_INVALID;

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_MSG_SEND;
  envl.recipient_id = recipient_auth_id;
  envl.body_size = msg_size;
  envl.body = (uint8_t *)msg;

  return client_send_envelope_locked(client, &envl);
}

int faith_client_event_fd(faith_client_t *client) {
  if (!client)
    return -1;
  return client->event_fd;
}

faith_status_code_t faith_client_next_event(faith_client_t *client,
                                            faith_event_t  *out) {
  if (!client || !out)
    return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);

  if (client->ev_queue_len == 0) {
    pthread_mutex_unlock(&client->lock);
    return FAITH_ERR_UNDERFLOW;
  }

  *out = client->ev_queue[client->ev_queue_front];

  client->ev_queue_front = (client->ev_queue_front + 1) % MAX_QUEUED_EVENTS;
  client->ev_queue_len--;

  pthread_mutex_unlock(&client->lock);
  return FAITH_OK;
}

faith_status_code_t faith_client_free_event(faith_event_t *ev) {
  if (!ev)
    return FAITH_ERR_INVALID;

  free(ev->chat_message);

  memset(ev, 0, sizeof(*ev));

  return FAITH_OK;
}

static faith_status_code_t 
client_gen_device_auth_respone_signature(faith_client_t *client,
                                         const faith_envl_stc_device_link_req_t* req,
                                         uint8_t o_signature[FAITH_ED25519_SIGNATURE_SIZE],
                                         size_t* o_signature_size,
                                         faith_device_link_response_type_t type) {
  if(!client || !req || !o_signature || !o_signature_size) return FAITH_ERR_INVALID;

  /* Construct message buffer for signature generation
   *    Message Buffer {
   *      auth_id,
   *      device_id_new,
   *      public_key_new_device,
   *      code,
   *      expires_at_ms,
   *      device_id_approving (device ID of the approving device),
   *      type (response decision)
   *    } */

  faith_signature_device_link_response_t sign_msg = {0};
  sign_msg.auth_id = req->auth_id;
  sign_msg.device_id_new = req->device_id_new;

  memcpy(sign_msg.public_key_new_device, req->public_key_new_device,
         FAITH_ED25519_PUBLIC_KEY_SIZE);
  memcpy(sign_msg.code, req->code, sizeof(req->code));

  sign_msg.expires_at_ms = req->expires_at_ms;
  sign_msg.device_id_responding = client->ident.device_id;

  sign_msg.type = type; 

  uint8_t msg_buf[FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE];
  {
    _FH_CHECK(faith_gen_sign_buf_device_link_response(
        msg_buf, sizeof(msg_buf), &sign_msg));

    if (_fh_rc != FAITH_OK) {
      nob_log(ERROR, "Failed to generate signing message buffer for device "
                     "auth response.");

      return _fh_rc;
    }
  }

  /* Generating the 64 byte cryptographic signature from the message buffer */

  /* This returns FAITH_ERR_SSL if signature_size !=
   * FAITH_ED25519_SIGNATURE_SIZE */
  _FH_CHECK(faith_gen_signature(client->ident.keypair, o_signature,
                                o_signature_size, msg_buf, sizeof(msg_buf)));
  if (_fh_rc != FAITH_OK) {
    nob_log(ERROR, "Failed to generate signature for DEVICE_AUTH_APPROE.");
    return _fh_rc;
  }

  return FAITH_OK;
}


faith_status_code_t
faith_client_approve_pending_device_auth(faith_client_t *client) {
  if (!client)
    return FAITH_ERR_INVALID;

  faith_envl_stc_device_link_req_t *req = client->pending_device_link_req;
  if (!req) {
    nob_log(ERROR, "[client] Cannot approve pending device authorization; No "
                   "authorization pending.");
    return FAITH_ERR_INVALID;
  }

  uint8_t signature[FAITH_ED25519_SIGNATURE_SIZE] = {0};
  size_t  signature_size = 0;

  faith_status_code_t _fh_result = FAITH_OK;
  _FH_CHECK_DEFER(client_gen_device_auth_respone_signature(
      client, req, signature, &signature_size, FAITH_DEVICE_LINK_APPROVE));

  /* Serialize faith_envl_cts_device_link_response_t */

  faith_envl_cts_device_link_response_t response_body = {0};
  response_body.device_id_new = req->device_id_new;
  memcpy(response_body.signature_response, signature, signature_size);

  faith_body_size_t body_size = 0;
  uint8_t           body[FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE];

  _FH_CHECK_DEFER(faith_encode_device_link_response_body(
      body, &body_size, sizeof(body), &response_body));

  faith_envelope_t approval_envl = {0};
  approval_envl.type = FAITH_ENVELOPE_DEVICE_AUTH_APPROVE;

  approval_envl.body = body;
  approval_envl.body_size = body_size;

  _FH_CHECK_DEFER(client_send_envelope_locked(client, &approval_envl));

  client_clear_pending_device_link_request(client);

  return FAITH_OK;

defer: {
  /* This frees the allocated pending device link request */
  _FH_CHECK(faith_client_deny_pending_device_auth(client));

  _fh_result = _fh_result == FAITH_OK ? _fh_rc : _fh_result;
  return _fh_result;
}
}

faith_status_code_t
faith_client_deny_pending_device_auth(faith_client_t *client) {
  if (!client)
    return FAITH_ERR_INVALID;

  faith_envl_stc_device_link_req_t *req = client->pending_device_link_req;
  if (!req) {
    nob_log(ERROR, "[client] Cannot deny pending device authorization; No "
                   "authorization pending.");
    return FAITH_ERR_INVALID;
  }

  uint8_t signature[FAITH_ED25519_SIGNATURE_SIZE] = {0};
  size_t  signature_size = 0;

  faith_status_code_t _fh_result = FAITH_OK;
  _FH_CHECK_DEFER(client_gen_device_auth_respone_signature(
      client, req, signature, &signature_size, FAITH_DEVICE_LINK_DENY));

  /* Serialize faith_envl_cts_device_link_response_t */

  faith_envl_cts_device_link_response_t response_body = {0};
  response_body.device_id_new = req->device_id_new;
  memcpy(response_body.signature_response, signature, signature_size);

  faith_body_size_t body_size = 0;
  uint8_t           body[FAITH_ENVL_CTS_DEVICE_LINK_RESPONSE_BODY_SIZE];

  faith_encode_device_link_response_body(body, &body_size, sizeof(body),
                                         &response_body);

  faith_envelope_t approval_envl = {0};
  approval_envl.type = FAITH_ENVELOPE_DEVICE_AUTH_DENY;

  approval_envl.body = body;
  approval_envl.body_size = body_size;

  _FH_CHECK_DEFER(client_send_envelope_locked(client, &approval_envl));

defer:
  client_clear_pending_device_link_request(client);

  return _fh_result;
}
