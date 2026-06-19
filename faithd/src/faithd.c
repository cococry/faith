#include <openssl/err.h>
#include <openssl/rand.h>
#include <openssl/ssl.h>
#include <string.h>
#include <unistd.h>
#include <stdio.h> 
#include <netdb.h> 
#include <netinet/in.h> 
#include <stdlib.h> 
#include <sys/socket.h> 
#include <sys/types.h> 
#include <unistd.h> 
#include <errno.h>
#include <signal.h>
#include <arpa/inet.h>

#define NOB_IMPLEMENTATION
#define NOB_STRIP_PREFIX
#include "../nob.h" 

#include "shared.h"

#define PORT 4433 


struct server_state_t {
  int sockfd;
  SSL_CTX* ssl_ctx;
};

static volatile sig_atomic_t shutdown_requested = 0;

static void handle_shutdown_signal(int sig)
{
  (void)sig;
  shutdown_requested = 1;
}

void server_destroy(struct server_state_t* s) {
  if (!s) return; 

  nob_log(INFO, "Destroying server context..."); 

  if (s->sockfd >= 0) {
    if (close(s->sockfd) < 0) {
      nob_log(ERROR, "close() failed: %s", strerror(errno)); 
    }
    s->sockfd = -1;
  }

  if (s->ssl_ctx) {
    SSL_CTX_free(s->ssl_ctx);
    s->ssl_ctx = NULL;
  }

  nob_log(INFO, "Destroyed server context."); 
}

static faith_status_code_t decode_ping(
    const uint8_t* payload, size_t payload_size, 
    uint64_t* ping_nonce, uint64_t* server_time_ms) {

  const size_t ping_size =  sizeof(uint64_t) * 2;

  if (payload == NULL || ping_nonce == NULL || server_time_ms == NULL)
    return FAITH_ERR_INVALID;

  if (payload_size != ping_size)
    return FAITH_ERR_BAD_FRAME;

  *ping_nonce = faith_read_u64_be(payload); 
  *server_time_ms = faith_read_u64_be(payload + sizeof(uint64_t)); 

  return FAITH_OK;
}

static faith_status_code_t encode_pong(
    uint8_t* out_buf, size_t* out_size, 
    size_t buf_cap_in_bytes,
    uint64_t nonce, uint64_t sent_at_ms
    ) {
  if(!out_buf) return FAITH_ERR_INVALID;

  const size_t pong_size =  sizeof(uint64_t) * 2;

  if (buf_cap_in_bytes < pong_size) 
    return FAITH_ERR_OVERFLOW;

  _FH_CHECK_RETURN(faith_write_u64_be(out_buf, nonce));
  _FH_CHECK_RETURN(faith_write_u64_be(out_buf + sizeof(uint64_t), sent_at_ms));

  *out_size = pong_size;

  return FAITH_OK;
}

static faith_status_code_t server_send_pong_ssl(SSL* ssl, uint64_t nonce, uint64_t sent_at_ms) {
  const size_t buf_cap_in_bytes = sizeof(uint64_t) * 2;
  uint8_t payload[buf_cap_in_bytes];
  size_t payload_size = 0;

  _FH_CHECK_RETURN(encode_pong(payload, &payload_size, buf_cap_in_bytes,
        nonce, sent_at_ms)); 

  NOB_ASSERT(payload_size == buf_cap_in_bytes);

  return faith_write_frame_ssl(ssl, FAITH_MSG_PONG, payload, payload_size);
}

static faith_status_code_t run_connected(SSL *ssl) {
  for (;;) {
    faith_frame_t frame;

    faith_status_code_t rc = faith_read_frame_ssl(ssl, &frame);
    if (rc != FAITH_OK)
      return rc;
    
    fprintf(stderr, "server got msg_type=%u payload_size=%zu\n",
        frame.msg_type, frame.payload_size);

    if (frame.msg_type == FAITH_MSG_PING) {
      uint64_t nonce;
      uint64_t client_sent_at_ms;

      rc = decode_ping(frame.payload, frame.payload_size,
          &nonce, &client_sent_at_ms);

      if (rc != FAITH_OK) {
        faith_frame_free(&frame);
        return rc;
      }
      
      (void)client_sent_at_ms;

      if (rc == FAITH_OK) {
        uint64_t server_sent_at_ms = faith_now_ms();
        fprintf(stderr, "sending pong nonce=%llu\n",
            (unsigned long long)nonce);
        rc = server_send_pong_ssl(ssl, nonce, server_sent_at_ms);
        if (rc != FAITH_OK) {
          faith_frame_free(&frame);
          return rc;
        }
        
        fprintf(stderr, "server_send_pong_ssl rc=%d\n", (int)rc);
      }
    }

    faith_frame_free(&frame);
  }
}

static void handle_client(SSL *ssl)
{
  faith_status_code_t rc = run_connected(ssl);

  if (rc == FAITH_ERR_CLOSED) {
    nob_log(INFO, "client closed TLS connection");
  } else {
    nob_log(ERROR, "client loop ended: %s (%d)",
            faith_status_code_name(rc), (int)rc);
  }
}

int loop(struct server_state_t* s) 
{
  int result = 0;
  struct sockaddr_in client;

  while (!shutdown_requested) {
    socklen_t len = sizeof(client);

    int connfd = accept(s->sockfd, (struct sockaddr*)&client, &len); 
    if (connfd < 0) {
      if (errno == EINTR) {
        if (shutdown_requested) break;
        continue;
      }

      nob_log(ERROR, "server accept() failed: %s", strerror(errno)); 
      // sets result
      return_defer(1);
    }

    nob_log(INFO, "New client connection accepted."); 

    SSL *ssl;
    /* Associate a new SSL handle with the new connection */
    if ((ssl = SSL_new(s->ssl_ctx)) == NULL) {
      ERR_print_errors_fp(stderr);
      nob_log(ERROR, "failed creating SSL handle for new connection");
      close(connfd);
      continue;
    }

    SSL_set_fd(ssl, connfd);

    /* Attempt an SSL handshake with the client */
    if (SSL_accept(ssl) <= 0) {
      ERR_print_errors_fp(stderr);
      nob_log(ERROR, "failed performing SSL handshake with client");
      SSL_free(ssl);
      close(connfd);
      continue;
    }

    handle_client(ssl);

    SSL_shutdown(ssl);
    SSL_free(ssl);
    close(connfd);
  }

defer:
  server_destroy(s);
  return result;
}

int init_ssl(SSL_CTX** ctx)
{
  int result = 0;

  *ctx = SSL_CTX_new(TLS_server_method());
  if (*ctx == NULL) {
    nob_log(ERROR, "failed to create server SSL_CTX");
    return_defer(1);
  }

  if (!SSL_CTX_set_min_proto_version(*ctx, TLS1_2_VERSION)) {
    nob_log(ERROR, "failed to set the minimum TLS protocol version");
    return_defer(1);
  }

  long opts = SSL_OP_IGNORE_UNEXPECTED_EOF
    | SSL_OP_NO_RENEGOTIATION
    | SSL_OP_SERVER_PREFERENCE;

  SSL_CTX_set_options(*ctx, opts);

  if (SSL_CTX_use_certificate_chain_file(*ctx, "chain.pem") <= 0) {
    nob_log(ERROR, "failed to load the server certificate chain file");
    return_defer(1);
  }

  if (SSL_CTX_use_PrivateKey_file(*ctx, "pkey.pem", SSL_FILETYPE_PEM) <= 0) {
    nob_log(ERROR, "failed loading the server private key file, "
        "possible key/cert mismatch?");
    return_defer(1);
  }

  static const char cache_id[] = "faithd-server";

  SSL_CTX_set_session_id_context(*ctx, (void *)cache_id, sizeof(cache_id));
  SSL_CTX_set_session_cache_mode(*ctx, SSL_SESS_CACHE_SERVER);
  SSL_CTX_sess_set_cache_size(*ctx, 1024);
  SSL_CTX_set_timeout(*ctx, 3600);
  SSL_CTX_set_verify(*ctx, SSL_VERIFY_NONE, NULL);

  nob_log(INFO, "SSL initialized.");

  return 0;

defer:
  ERR_print_errors_fp(stderr);
  if(*ctx) {
    SSL_CTX_free(*ctx);
    *ctx = NULL;
  }
  return result;

}

int init_server_sock() {
  struct sockaddr_in servaddr; 

  int sockfd = -1;

  sockfd = socket(AF_INET, SOCK_STREAM, 0); 
  if (sockfd == -1) { 
    nob_log(ERROR, "socket creation failed"); 
    return 1;
  } 

  nob_log(INFO, "Socket successfully created"); 

  memset(&servaddr, 0, sizeof(servaddr)); 

  servaddr.sin_family = AF_INET; 
  servaddr.sin_addr.s_addr = htonl(INADDR_ANY); 
  servaddr.sin_port = htons(PORT); 

  int yes = 1;
  if (setsockopt(sockfd, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes)) < 0) {
    nob_log(ERROR, "setsockopt(SO_REUSEADDR) failed: %s", strerror(errno));
    close(sockfd);
    return -1;
  }

  if ((bind(sockfd, (struct sockaddr*)&servaddr, sizeof(servaddr))) != 0) { 
    nob_log(ERROR, "socket bind failed"); 
    close(sockfd);
    return -1;
  } 

  nob_log(INFO, "Socket successfully bound"); 

  if ((listen(sockfd, 5)) != 0) { 
    nob_log(ERROR, "listen() failed: %s", strerror(errno)); 
    close(sockfd);
    return -1;
  }

  nob_log(INFO, "Server listening.."); 

  return sockfd;
}

int install_signal_handlers(void)
{
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));

  sa.sa_handler = handle_shutdown_signal;
  sigemptyset(&sa.sa_mask);

  if (sigaction(SIGINT, &sa, NULL) < 0) {
    nob_log(ERROR, "sigaction(SIGINT) failed: %s", strerror(errno));
    return 1;
  }

  if (sigaction(SIGTERM, &sa, NULL) < 0) {
    nob_log(ERROR, "sigaction(SIGTERM) failed: %s", strerror(errno));
    return 1;
  }

  return 0;
}

int main(void) {
  struct server_state_t s = {
    .sockfd = -1,
    .ssl_ctx = NULL,
  };

  if (install_signal_handlers() != 0) exit(1);
  if (init_ssl(&s.ssl_ctx) != 0) {
    server_destroy(&s);
    exit(1);
  }
  if ((s.sockfd = init_server_sock()) < 0) {
    server_destroy(&s);
    exit(1);
  }

  return loop(&s);
}
