#include "client.h"
#include <arpa/inet.h>
#include <assert.h>
#include <errno.h>
#include <netinet/in.h>
#include <openssl/err.h>
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
#define NOB_STRIP_PREFIX
#include "../../nob.h"

#define MAX_QUEUED_EVENTS 512

static int g_log_enable_tracing = true;

typedef struct { 
  EVP_PKEY* keypair;
  uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  uint8_t private_key[FAITH_ED25519_PRIVATE_KEY_SIZE];

  client_id_t auth_id;
  device_id_t device_id;
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

  SSL *ssl;

  uint16_t proto_ver;

  client_identity_t ident;
};

static faith_status_code_t
client_push_event_ex(faith_client_t *client, faith_event_type_t type,
                     uint64_t value0, uint64_t value1, const char *message,
                     char *chat_message, size_t chat_message_size) {
  if (!client)
    return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);

  /* Enqueue event */

  if (client->ev_queue_len == MAX_QUEUED_EVENTS) {
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
    if (RAND_bytes((unsigned char *)&nonce, sizeof(nonce)) != 1) {
      nob_log(ERROR,
              "Failed to generate random bytes with OpenSSL RAND_bytes()");
      break;
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
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "Failed to send ping"));
      atomic_store(&client->connected, false);
      break;
    }

    if (g_log_enable_tracing)
      nob_log(INFO, "[client - pinger]: Sent PING to server successfully.");

    // 10 seconds after each ping
    _sleep_heartbeat_interval(client);
  }

  return NULL;
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

static void client_handle_envl(faith_client_t *client, faith_frame_t *frame) {
  faith_envelope_t envl;
  _FH_CHECK(faith_decode_envelope(frame->payload, frame->payload_size, &envl));

  switch (envl.type) {
  case FAITH_ENVELOPE_MSG_SEND: {
    if (!faith_client_id_equal(envl.recipient_id, client->ident.auth_id) ||
        _fh_rc != FAITH_OK ||
        faith_client_id_equal(envl.sender_id, FAITH_CLIENT_ID_NONE)) {
      nob_log(ERROR, "[client - reader] Got sent invalid message envelope.");
      break;
    }

    if (envl.body_size > FAITH_MAX_MSG_SIZE) {
      nob_log(ERROR, "[client - reader] Got sent too large message.");
      break;
    }
    char *msg = malloc(envl.body_size + 1);
    if (!msg) {
      nob_log(
          ERROR,
          "[client - reader] Failed to allocate memory for received message.");
      break;
    }

    memcpy(msg, envl.body, envl.body_size);
    msg[envl.body_size] = '\0';

    _FH_CHECK(client_push_event_ex(client, FAITH_EVENT_MESSAGE_RECEIVED, 0, 0,
                                   NULL, msg, envl.body_size));

    break;
  }
  default:
    break;
  }
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
      client_handle_envl(client, &frame);
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
  //    public_key
  //  }
  // }
  // 
  size_t offset = 0;
  size_t device_id_size = sizeof(client->ident.device_id.bytes);
  memcpy(body, client->ident.device_id.bytes,
         device_id_size);
  offset += device_id_size;

  memcpy(body + offset, client->ident.public_key, sizeof(client->ident.public_key));
  offset += sizeof(client->ident.public_key);

  uint64_t nonce;
  if (RAND_bytes((unsigned char *)&nonce, sizeof(nonce)) != 1) {
    nob_log(ERROR, "Failed to generate auth_id with OpenSSL RAND_bytes()");
    return FAITH_ERR_SSL;
  }
  printf("CLIENT NONCE: %lu\n", nonce);

  _FH_CHECK_RETURN(faith_write_u64_be(body + offset, nonce)); 

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_HELLO;

  memcpy(envl.sender_id.bytes, client->ident.auth_id.bytes,
         sizeof(envl.sender_id));

  envl.body = body;
  printf("Envl body size: %zu\n", sizeof(body));
  envl.body_size = sizeof(body);

  char auth_id_hex[33];
  char device_id_hex[33];
  _FH_CHECK_RETURN(faith_id128_to_hex(client->ident.auth_id.bytes, auth_id_hex));
  _FH_CHECK_RETURN(faith_id128_to_hex(client->ident.device_id.bytes, device_id_hex));

  printf("CLIENT AUTH ID: %s\n", auth_id_hex);
  printf("DEVICE AUTH ID: %s\n", device_id_hex);

  printf("PUBKEY =====================\n");
  for(size_t i = 0; i < FAITH_ED25519_PUBLIC_KEY_SIZE; i++) {
    printf("%02x", (unsigned int)client->ident.public_key[i]);
  }
  printf("\n");
  printf("===========================\n");
    

  pthread_mutex_lock(&client->write_lock);
  _FH_CHECK(client_send_envelope(client->ssl, &envl));
  pthread_mutex_unlock(&client->write_lock);

  return _fh_rc;
}

static faith_status_code_t client_make_handshake(faith_client_t* client) {
  if(!client) return FAITH_ERR_INVALID;
   if (g_log_enable_tracing)
      nob_log(INFO, "[client] Sending HELLO...");

    {
      _FH_CHECK(faith_client_send_hello(client));
      if (_fh_rc == FAITH_OK) {
        if (g_log_enable_tracing)
          nob_log(INFO, "[client] Sent HELLO.");
      } else {
        client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                          "Failed to send HELLO");
       return _fh_rc; 
      }
    }

    if (g_log_enable_tracing)
      nob_log(INFO, "[client] Waiting for server HELLO_OK response ...");

    faith_frame_t frame = {0};

    _FH_CHECK(faith_read_frame_sync(client->ssl, &frame));
    if (_fh_rc != FAITH_OK) {
      client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                        "Failed to read HELLO response");
      return _fh_rc; 
    }

    if (frame.msg_type != FAITH_MSG_ENVL) {
      faith_frame_free(&frame);
      client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                        "Unexpected HELLO response");
      return FAITH_ERR_INVALID;
    }

    faith_envelope_t envl;
    {
      _FH_CHECK(
          faith_decode_envelope(frame.payload, frame.payload_size, &envl));
    }

    int ok = envl.type == FAITH_ENVELOPE_HELLO_OK && envl.body_size == 0;
    if (!ok) {
      faith_frame_free(&frame);
      client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                        "Unexpected HELLO envelope response");
      return FAITH_ERR_INVALID;
    }

    faith_frame_free(&frame);

    if (g_log_enable_tracing)
      nob_log(INFO,
              "[client] Got HELLO_OK response. Server acknowledged client "
              "successfully");

    return FAITH_OK;
}

static void *faith_client_thread_routine(void *arg) {
  faith_client_t *client = arg;

  uint32_t backoff_ms = 250;

  while (client_is_running(client)) {
    {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_CONNECTING, 0, 0, NULL));
    }

    int fd = client_init_sock(client->host, client->port);

    if (fd < 0) {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "Connect failed. Server down?"));
      _sleep_ms(backoff_ms);
      backoff_ms = client_next_backoff_ms(backoff_ms);
      continue;
    }

    pthread_mutex_lock(&client->lock);
    client->sockfd = fd;
    pthread_mutex_unlock(&client->lock);

    SSL_CTX *ctx = NULL;
    SSL     *ssl = NULL;

    ctx = client_create_ssl_ctx(client);
    if (!ctx) {
      client_push_event(client, FAITH_EVENT_ERROR, 0, 0, "TLS context failed");
      goto fail;
    }

    ssl = SSL_new(ctx);
    if (ssl == NULL) {
      nob_log(ERROR, "Failed to create SSL object for client");
      goto fail;
    }

    if (SSL_set_fd(ssl, fd) != 1) {
      nob_log(ERROR, "Failed to set FD of SSL connection for client");
      goto fail;
    }

    if (!client->insecure_skip_verify && client->server_name[0] != '\0') {
      if (SSL_set_tlsext_host_name(ssl, client->server_name) != 1) {
        nob_log(ERROR, "Failed to set TLS hostname to '%s'",
                client->server_name);
        goto fail;
      }

      if (SSL_set1_host(ssl, client->server_name) != 1) {
        nob_log(ERROR, "Failed to set TLS hostname to '%s'",
                client->server_name);
        goto fail;
      }
    }

    client->ssl = ssl;

    if (SSL_connect(ssl) <= 0) {
      nob_log(ERROR, "TLS handshake failed");
      goto fail;
    }
    backoff_ms = 250;

    {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_CONNECTED, 0, 0, NULL));
      if (_fh_rc != FAITH_OK) {
        goto fail;
      }
    }

    {
      _FH_CHECK(client_make_handshake(client));
      if (_fh_rc != FAITH_OK) {
        goto fail;
      }
    }

    client_run_connected(client);

    {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "connection closed"));
      if (_fh_rc != FAITH_OK) {
        goto fail;
      }
    }

  fail:
    ERR_print_errors_fp(stderr);

    if (ssl) {
      SSL_shutdown(ssl);
      SSL_free(ssl);
      ssl = NULL;
      client->ssl = NULL;
    }

    if (ctx) {
      SSL_CTX_free(ctx);
      ctx = NULL;
    }

    close(fd);

    pthread_mutex_lock(&client->lock);
    client->ssl = NULL;
    client->sockfd = -1;
    pthread_mutex_unlock(&client->lock);

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

static bool client_id_from_hex(const char *hex, client_id_t *out) {
  if (!hex || !out)
    return false;

  if (strlen(hex) != FAITH_CLIENT_ID_SIZE * 2)
    return false;

  client_id_t id = {0};

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
  _FH_CHECK_RETURN(faith_random_bytes(o_ident->auth_id.bytes,
                                      sizeof(o_ident->auth_id.bytes)));

  _FH_CHECK_RETURN(faith_random_bytes(o_ident->device_id.bytes,
                                      sizeof(o_ident->device_id.bytes)));

  /* Generate client identity keypair */
  _FH_CHECK_RETURN(faith_gen_ed25519_keypair(&o_ident->keypair, o_ident->private_key, o_ident->public_key));

  
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
  if(_fh_rc != FAITH_OK)  {
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
  if (!client)
    return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);
  int was_running = client->running;
  client->running = 0;
  pthread_mutex_unlock(&client->lock);

  if (was_running) {
    pthread_mutex_lock(&client->lock);
    int fd = client->sockfd;
    pthread_mutex_unlock(&client->lock);
    if (fd >= 0) {
      shutdown(fd, SHUT_RDWR);
    }

    pthread_join(client->thread, NULL);
  }

  return FAITH_OK;
}

faith_status_code_t faith_client_send_message(faith_client_t *client,
                                              client_id_t     recipient_auth_id,
                                              const char     *msg) {
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

  pthread_mutex_lock(&client->write_lock);
  faith_status_code_t rc = client_send_envelope(client->ssl, &envl);
  pthread_mutex_unlock(&client->write_lock);

  return rc;
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
