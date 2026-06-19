#include "client.h"
#include <arpa/inet.h>
#include <assert.h>
#include <netinet/in.h>
#include <openssl/err.h>
#include <openssl/ssl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/eventfd.h>
#include <sys/time.h>
#include <errno.h>
#include <openssl/rand.h>

#define NOB_IMPLEMENTATION
#define NOB_STRIP_PREFIX
#include "../../nob.h" 

#define MAX_QUEUED_EVENTS 512 

struct faith_client {
  char            host[256];
  uint16_t        port;

  char            server_name[256];
  char            ca_file[512];
  int             insecure_skip_verify;

  int             running;
  pthread_t       thread;

  int             sockfd;
  int             event_fd;

  pthread_mutex_t lock;

  faith_event_t   ev_queue[MAX_QUEUED_EVENTS];
  uint16_t        ev_queue_front;
  uint16_t        ev_queue_back;
  uint16_t        ev_queue_len;
};

static faith_status_code_t faith_push_client_event(faith_client_t* client, faith_event_type_t type, 
    uint64_t value0, uint64_t value1, const char* message) {
  if(!client) return FAITH_ERR_INVALID;

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
  if(message != NULL) {
    snprintf(ev.message, sizeof(ev.message), "%s", message);
  } else {
    ev.message[0] = '\0';
  }

  client->ev_queue[client->ev_queue_back] = ev;
  client->ev_queue_back = (client->ev_queue_back + 1) % MAX_QUEUED_EVENTS;
  client->ev_queue_len++;

  uint64_t one = 1;
  (void)write(client->event_fd, &one, sizeof(one));

  pthread_mutex_unlock(&client->lock);

  return FAITH_OK;
}

static int faith_client_is_running(faith_client_t* client) {
  int running;

  pthread_mutex_lock(&client->lock);
  running = client->running;
  pthread_mutex_unlock(&client->lock);

  return running;
}

static int faith_init_client_sock(const char* host, int port)
{
  int sockfd = -1;
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

  if (connect(sockfd, (struct sockaddr* )&serveraddr, sizeof(serveraddr)) < 0) {
    nob_log(ERROR, "Connection to server failed: %s", strerror(errno));
    close(sockfd);
    return -1;
  }

  nob_log(INFO, "Client connected");

  return sockfd;
}

static void faith_sleep_ms(unsigned ms) {
  struct timespec ts;

  ts.tv_sec = ms / 1000;
  ts.tv_nsec = (long)(ms % 1000) * 1000000L;

  while (nanosleep(&ts, &ts) < 0 && errno == EINTR);
}

static uint32_t faith_next_backoff_ms(uint32_t current) {
  if (current < 250)
    return 250;

  if (current >= 30000)
    return 30000;

  current *= 2;

  if (current > 30000)
    return 30000;

  return current;
}

static SSL_CTX* faith_client_create_ssl_ctx(faith_client_t* client) {
  SSL_CTX* ctx = NULL;

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

static faith_status_code_t faith_encode_ping(
    uint8_t* out_buf, size_t* out_size, 
    size_t buf_cap_in_bytes,
    uint64_t nonce, uint64_t sent_at_ms
    ) {
  if(!out_buf) return FAITH_ERR_INVALID;

  const size_t ping_size =  sizeof(uint64_t) * 2;

  if (buf_cap_in_bytes < ping_size) 
    return FAITH_ERR_OVERFLOW;

  _FH_CHECK_RETURN(faith_write_u64_be(out_buf, nonce));
  _FH_CHECK_RETURN(faith_write_u64_be(out_buf + sizeof(uint64_t), sent_at_ms));

  *out_size = ping_size;

  return FAITH_OK;
}

static faith_status_code_t faith_decode_pong(const uint8_t* payload, size_t payload_size, uint64_t* pong_nonce, uint64_t* server_time_ms) {
  const size_t pong_size =  sizeof(uint64_t) * 2;

  if (payload == NULL || pong_nonce == NULL || server_time_ms == NULL)
    return FAITH_ERR_INVALID;

  if (payload_size != pong_size) 
    return FAITH_ERR_BAD_FRAME;

  *pong_nonce = faith_read_u64_be(payload); 
  *server_time_ms = faith_read_u64_be(payload + sizeof(uint64_t)); 

  return FAITH_OK;
} 

static faith_status_code_t faith_client_send_ping_ssl(SSL* ssl, uint64_t nonce, uint64_t sent_at_ms) {
  const size_t buf_cap_in_bytes = sizeof(uint64_t) * 2;
  uint8_t payload[buf_cap_in_bytes];
  size_t payload_size = 0;

  _FH_CHECK_RETURN(faith_encode_ping(payload, &payload_size, buf_cap_in_bytes,
        nonce, sent_at_ms)); 

  NOB_ASSERT(payload_size == buf_cap_in_bytes);

  return faith_write_frame_ssl(ssl, FAITH_MSG_PING, payload, payload_size);
}

static void faith_client_run_connected(faith_client_t *client, SSL *ssl) {
  while (faith_client_is_running(client)) {
    uint64_t sent_at_ms = faith_now_ms();
    uint64_t nonce;
    if (RAND_bytes((unsigned char *)&nonce, sizeof(nonce)) != 1) {
      nob_log(ERROR, "Failed to generate random bytes with OpenSSL RAND_bytes()\n"); 
      break;
    }

    if (faith_client_send_ping_ssl(ssl, nonce, sent_at_ms) != FAITH_OK) {
      faith_push_client_event(client, FAITH_EVENT_DISCONNECTED, 0, 0, "Failed to send ping");
      break;
    }

    faith_frame_t frame;
    if (faith_read_frame_ssl(ssl, &frame) != FAITH_OK) {
      faith_push_client_event(client, FAITH_EVENT_DISCONNECTED, 0, 0, "Failed to read server frame");
      break;
    }

    if (frame.msg_type == FAITH_MSG_PONG) {
      uint64_t pong_nonce;
      uint64_t server_time_ms;

      if (faith_decode_pong(frame.payload, frame.payload_size, &pong_nonce, &server_time_ms) == FAITH_OK &&
          pong_nonce == nonce) {
        uint64_t now = faith_now_ms();
        faith_push_client_event(client, FAITH_EVENT_PONG, now - sent_at_ms, server_time_ms, NULL);
      }
    }

    faith_frame_free(&frame);

    faith_sleep_ms(10000);
  }
}

static void* faith_client_thread_routine(void* arg) {
  faith_client_t* client = arg;

  uint32_t backoff_ms = 250;

  while (faith_client_is_running(client)) {
    _FH_CHECK(faith_push_client_event(client, FAITH_EVENT_CONNECTING, 0, 0, NULL));

    int fd = faith_init_client_sock(client->host, client->port);

    if (fd < 0) {
      faith_push_client_event(client, FAITH_EVENT_DISCONNECTED, 0, 0, "Connect failed");
      faith_sleep_ms(backoff_ms);
      backoff_ms = faith_next_backoff_ms(backoff_ms);
      continue;
    }

    client->sockfd = fd;

    SSL_CTX* ctx = faith_client_create_ssl_ctx(client);
    if (!ctx) {
      close(fd);
      client->sockfd = -1;

      faith_push_client_event(client, FAITH_EVENT_ERROR, 0, 0, "TLS context failed");
      faith_sleep_ms(backoff_ms);
      backoff_ms = faith_next_backoff_ms(backoff_ms);
      continue;
    }

    SSL* ssl = SSL_new(ctx);
    if (ssl == NULL) {
      ERR_print_errors_fp(stderr);
      SSL_CTX_free(ctx);

      close(fd);
      client->sockfd = -1;
      continue;
    }

    if (SSL_set_fd(ssl, fd) != 1) {
      ERR_print_errors_fp(stderr);
      SSL_free(ssl);
      SSL_CTX_free(ctx);

      close(fd);
      client->sockfd = -1;
      continue;
    }

    if (!client->insecure_skip_verify && client->server_name[0] != '\0') {
      if (SSL_set_tlsext_host_name(ssl, client->server_name) != 1) {
        ERR_print_errors_fp(stderr);
        SSL_free(ssl);
        SSL_CTX_free(ctx);

        close(fd);
        client->sockfd = -1;
        continue;
      }

      if (SSL_set1_host(ssl, client->server_name) != 1) {
        ERR_print_errors_fp(stderr);
        SSL_free(ssl);
        SSL_CTX_free(ctx);

        close(fd);
        client->sockfd = -1;
        continue;
      }
    }

    if (SSL_connect(ssl) <= 0) {
      nob_log(ERROR, "TLS handshake failed");
      ERR_print_errors_fp(stderr);
      SSL_free(ssl);
      SSL_CTX_free(ctx);

      close(fd);
      client->sockfd = -1;
      continue;
    }

    _FH_CHECK(faith_push_client_event(client, FAITH_EVENT_CONNECTED, 0, 0, NULL));

    faith_client_run_connected(client, ssl);

    SSL_shutdown(ssl);
    SSL_free(ssl);
    SSL_CTX_free(ctx);

    close(fd);
    client->sockfd = -1;

    _FH_CHECK(faith_push_client_event(client, FAITH_EVENT_DISCONNECTED, 0, 0, "connection closed"));

    faith_sleep_ms(backoff_ms);
    backoff_ms = faith_next_backoff_ms(backoff_ms);
  }

  return NULL;
}

faith_client_t* faith_client_create(const faith_client_config_t* cfg) {
  if (!cfg || !cfg->host)
    return NULL;

  faith_client_t* client = calloc(1, sizeof(*client));
  if (!client)
    return NULL;

  client->sockfd = -1;
  client->event_fd = -1;

  snprintf(client->host, sizeof(client->host), "%s", cfg->host);
  client->port = cfg->port;

  if (cfg->server_name)
    snprintf(client->server_name, sizeof(client->server_name), "%s", cfg->server_name);

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

  return client;
}

void faith_client_destroy(faith_client_t* client) {
  if (!client)
    return;

  if (client->sockfd >= 0) {
    close(client->sockfd);
    client->sockfd = -1;
  }

  if (client->event_fd >= 0) {
    close(client->event_fd);
    client->event_fd = -1;
  }

  pthread_mutex_destroy(&client->lock);
  free(client);
}

faith_status_code_t faith_client_start(faith_client_t* client) {
  if(!client) return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);

  if (client->running) {
    pthread_mutex_unlock(&client->lock);
    return FAITH_ERR_ALREADY_STARTED;
  }

  client->running = 1;

  if(pthread_create(&client->thread, NULL,  
        faith_client_thread_routine, client) != 0) {

    client->running = 0;
    pthread_mutex_unlock(&client->lock);
    return FAITH_ERR_THREAD;
  }
  pthread_mutex_unlock(&client->lock);

  return FAITH_OK; 
}

faith_status_code_t faith_client_stop(faith_client_t* client) {
  if (!client) return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);
  int was_running = client->running;
  client->running = 0;
  pthread_mutex_unlock(&client->lock);

  if (was_running) { 
    // unblock SSL/socket reads
    shutdown(client->sockfd, SHUT_RDWR);
    pthread_join(client->thread, NULL);
  }

  return FAITH_OK;
}

int faith_client_event_fd(faith_client_t* client) {
  return client->event_fd;
}

faith_status_code_t faith_client_next_event(faith_client_t* client, faith_event_t* out) {
  if(!client || !out) return FAITH_ERR_INVALID;

  pthread_mutex_lock(&client->lock);

  if(client->ev_queue_len == 0) {
    pthread_mutex_unlock(&client->lock);
    return FAITH_ERR_UNDERFLOW;
  }

  *out = client->ev_queue[client->ev_queue_front];

  client->ev_queue_front = (client->ev_queue_front + 1) % MAX_QUEUED_EVENTS;
  client->ev_queue_len--;

  pthread_mutex_unlock(&client->lock);
  return FAITH_OK;
}

