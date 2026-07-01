#include "client.h"
#include <arpa/inet.h>
#include <assert.h>
#include <errno.h>
#include <netinet/in.h>
#include <openssl/err.h>
#include <openssl/rand.h>
#include <openssl/ssl.h>
#include <pthread.h>
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

  faith_event_t ev_queue[MAX_QUEUED_EVENTS];
  uint16_t      ev_queue_front;
  uint16_t      ev_queue_back;
  uint16_t      ev_queue_len;

  SSL *ssl;

  uint16_t proto_ver;

  // TODO: temporary
  client_id_t client_id;
  device_id_t device_id;
};

static faith_status_code_t client_push_event(faith_client_t    *client,
                                             faith_event_type_t type,
                                             uint64_t value0, uint64_t value1,
                                             const char *message) {
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

  nob_log(INFO, "Client connected");

  return sockfd;
}

static void _sleep_ms(unsigned ms) {
  struct timespec ts;

  ts.tv_sec = ms / 1000;
  ts.tv_nsec = (long)(ms % 1000) * 1000000L;

  while (nanosleep(&ts, &ts) < 0 && errno == EINTR)
    ;
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
    nob_log(WARNING, "TLS certificate verification is disabled");
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

  const size_t buf_cap_in_bytes = sizeof(uint64_t) * 2;
  uint8_t     *payload = malloc(buf_cap_in_bytes);
  if (!payload)
    return FAITH_ERR_NOMEM;

  size_t payload_size = 0;

  {
    _FH_CHECK(encode_ping(payload, &payload_size, buf_cap_in_bytes, nonce,
                          sent_at_ms));
    if (_fh_rc != FAITH_OK) {
      free(payload);
      return _fh_rc;
    }
  }

  _FH_CHECK(faith_write_frame_sync(ssl, FAITH_MSG_PING, payload, payload_size));

  if (_fh_rc != FAITH_OK) {
    free(payload);
    return _fh_rc;
  }

  free(payload);
  return FAITH_OK;
}

static void client_run_connected(faith_client_t *client, SSL *ssl) {
  while (client_is_running(client)) {
    uint64_t sent_at_ms = faith_now_ms();
    uint64_t nonce;
    if (RAND_bytes((unsigned char *)&nonce, sizeof(nonce)) != 1) {
      nob_log(ERROR,
              "Failed to generate random bytes with OpenSSL RAND_bytes()");
      break;
    }

    nob_log(INFO, "[client]: Sending PING to server...");

    if (client_send_ping_sync(ssl, nonce, sent_at_ms) != FAITH_OK) {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "Failed to send ping"));
      break;
    }

    nob_log(INFO, "[client]: Sent PING to server.");

    nob_log(INFO, "[client] Waiting for server PONG response ...");

    faith_frame_t frame;
    if (faith_read_frame_sync(ssl, &frame) != FAITH_OK) {
      printf("Server down.\n");
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "Failed to read server frame"));
      break;
    }

    nob_log(INFO, "[client] Got server response: %s",
            faith_frame_msg_name(frame.msg_type));

    if (frame.msg_type == FAITH_MSG_PONG) {
      uint64_t pong_nonce;
      uint64_t server_time_ms;

      _FH_CHECK(decode_pong(frame.payload, frame.payload_size, &pong_nonce,
                            &server_time_ms));

      if (_fh_rc == FAITH_OK && pong_nonce == nonce) {
        uint64_t now = faith_now_ms();
        _FH_CHECK(client_push_event(client, FAITH_EVENT_PONG, now - sent_at_ms,
                                    server_time_ms, NULL));
      } else {
        _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                    "Invalid pong response from server"));
      }
    } else {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "Unexpected server response to ping."));
    }

    nob_log(INFO, "[client] Frame successful.");

    faith_frame_free(&frame);

    // 10 seconds after ping/pong
    _sleep_ms(10000);
  }
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

  uint8_t body[sizeof(uint32_t)] = {0};

  _FH_CHECK_RETURN(faith_write_u32_be(body, client->device_id));

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_HELLO;
  envl.sender_id = client->client_id;
  envl.body = body;
  envl.body_size = sizeof(body);

  _FH_CHECK_RETURN(client_send_envelope(client->ssl, &envl));

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
    }

    nob_log(INFO, "Sending HELLO...");
    {
      _FH_CHECK(faith_client_send_hello(client));
      if (_fh_rc == FAITH_OK) {
        nob_log(INFO, "Sent HELLO.");
      } else {
        client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                          "Failed to send HELLO");
        goto fail;
      }
    }

    nob_log(INFO, "[client] Waiting for server HELLO_OK response ...");

    faith_frame_t frame = {0};

    if (faith_read_frame_sync(ssl, &frame) != FAITH_OK) {
      client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                        "Failed to read HELLO response");
      goto fail;
    }

    if (frame.msg_type != FAITH_MSG_ENVL) {
      faith_frame_free(&frame);
      client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                        "Unexpected HELLO response");
      goto fail;
    }

    faith_envelope_t envl;
    faith_decode_envelope(frame.payload, frame.payload_size, &envl);

    bool ok = envl.type == FAITH_ENVELOPE_HELLO_OK && envl.body_size == 0;
    if (!ok) {
      faith_frame_free(&frame);
      client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                        "Unexpected HELLO envelope response");
      goto fail;
    }

    faith_frame_free(&frame);
    
    nob_log(INFO, "[client] Got HELLO_OK response. Server acknowledged client successfully"); 

    client_run_connected(client, ssl);

    {
      _FH_CHECK(client_push_event(client, FAITH_EVENT_DISCONNECTED, 0, 0,
                                  "connection closed"));
    }

  fail:
    ERR_print_errors_fp(stderr);

    if (ssl) {
      SSL_shutdown(ssl);
      SSL_free(ssl);
      ssl = NULL;
    }

    if (ctx) {
      SSL_CTX_free(ctx);
      ctx = NULL;
    }

    close(fd);

    pthread_mutex_lock(&client->lock);
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

faith_status_code_t faith_client_init_global(void) {
  ignore_sigpipe();

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

  if (cfg->server_name)
    snprintf(client->server_name, sizeof(client->server_name), "%s",
             cfg->server_name);

  if (cfg->ca_file)
    snprintf(client->ca_file, sizeof(client->ca_file), "%s", cfg->ca_file);

  client->insecure_skip_verify = cfg->insecure_skip_verify;

  if (pthread_mutex_init(&client->lock, NULL) != 0) {
    free(client);
    return NULL;
  }

  client->event_fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
  if (client->event_fd < 0) {
    pthread_mutex_destroy(&client->lock);
    free(client);
    return NULL;
  }

  // TODO: temporary
  client->client_id = cfg->client_id;
  client->device_id = cfg->device_id;

  return client;
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

faith_status_code_t faith_client_send_messege(faith_client_t *client,
                                              client_id_t     recipient_auth_id,
                                              const char     *msg) {
  (void)client;
  (void)recipient_auth_id;
  (void)msg;
  return FAITH_OK;
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
