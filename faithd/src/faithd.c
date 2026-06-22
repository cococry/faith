#define _GNU_SOURCE

#include <openssl/err.h>
#include <openssl/rand.h>
#include <openssl/ssl.h>

#include <sys/socket.h> 
#include <sys/types.h> 
#include <sys/epoll.h> 
#include <sys/types.h>

#include <netinet/in.h> 
#include <arpa/inet.h>

#include <stdio.h> 
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>
#include <stdatomic.h>
#include <stdlib.h> 
#include <unistd.h>
#include <inttypes.h> 
#include <errno.h>
#include <signal.h>

#define NOB_IMPLEMENTATION
#define NOB_STRIP_PREFIX
#include "../nob.h" 

#include "shared.h"

#define PORT        4433 
#define MAX_EVENTS  10 

struct server_cfg_t {
  int                     verbose_logging;
};

struct server_state_t {
  int                     listenfd;
  int                     epoll_fd;
  SSL_CTX*                ssl_ctx;
  
  atomic_uint_fast64_t    next_client_id;

  struct server_cfg_t     cfg;
};

enum client_state_t {
  CLIENT_HANDSHAKE,
  CLIENT_OPEN,
  CLIENT_CLOSING
};

enum read_frame_result_t {
  READ_FRAME_OK,
  READ_FRAME_GOT_BYTES,
  READ_FRAME_WANT_READ,
  READ_FRAME_WANT_WRITE,
  READ_FRAME_CLOSED,
  READ_FRAME_ERROR
};

struct client_t {
  uint64_t              id;
  int                   fd;
  SSL*                  ssl;
  enum client_state_t   state;

  uint32_t              evs_mask_want;

  uint8_t*              out_buf;
  size_t                out_size;
  size_t                out_off;

  uint8_t*              in_buf;
  size_t                in_size;
  size_t                in_cap;
  size_t                in_off;
};

static volatile sig_atomic_t shutdown_requested = 0;

static void handle_shutdown_signal(int sig)
{
  (void)sig;
  shutdown_requested = 1;
}

static int set_nonblocking(int fd) {
  // get flags from file descriptor
  int flags;
  if((flags = fcntl(fd, F_GETFL,0)) < 0) return 1; 
  // add nonblocking flag  
  if(fcntl(fd, F_SETFL,flags | O_NONBLOCK) < 0) return 1; 
  return 0;
}

static faith_status_code_t close_client(int epoll_fd, struct client_t* cl) {
  if(!cl) return FAITH_ERR_INVALID;

  epoll_ctl(epoll_fd, EPOLL_CTL_DEL, cl->fd, NULL);

  if(cl->ssl) {
    SSL_set_shutdown(cl->ssl, SSL_SENT_SHUTDOWN | SSL_RECEIVED_SHUTDOWN);
    SSL_free(cl->ssl);
  }

  if(cl->fd >= 0)
    close(cl->fd);

  if(cl->out_buf != NULL) {
    free(cl->out_buf);
    cl->out_buf = NULL;
  }
  
  if(cl->in_buf != NULL) {
    free(cl->in_buf);
    cl->in_buf = NULL;
  }
  
  nob_log(INFO, "[client=%" PRIu64 " fd=%i]: Closed client", cl->id, cl->fd);

  free(cl);
  cl = NULL;
    

  return FAITH_OK;
}

static int modify_client_ev_mask(int epoll_fd, struct client_t* cl, uint32_t mask) {
  if(!cl) return -1;

  struct epoll_event ev;

  memset(&ev, 0, sizeof(ev));
  ev.events = mask;
  ev.data.ptr = cl;

  cl->evs_mask_want = mask;
  
  return epoll_ctl(epoll_fd, EPOLL_CTL_MOD, cl->fd, &ev);
}

void server_destroy(struct server_state_t* s) {
  if (!s) return; 

  nob_log(INFO, "Destroying server context..."); 

  if (s->listenfd >= 0) {
    if (close(s->listenfd) < 0) {
      nob_log(ERROR, "close() on listen FD failed: %s", strerror(errno)); 
    }
    s->listenfd = -1;
  }

  if(s->epoll_fd >= 0) {
    if (close(s->epoll_fd) < 0) {
      nob_log(ERROR, "close() epoll FD failed: %s", strerror(errno)); 
    }
    s->epoll_fd = -1;
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

static faith_status_code_t enqueue_input_bytes(struct client_t* cl, const uint8_t* bytes, size_t n_bytes,
    const struct server_cfg_t* cfg) { 
  if(!cl || !cfg) return FAITH_ERR_INVALID;

  if(cfg->verbose_logging) {
    nob_log(
        INFO, "[client=%" PRIu64 " fd=%i] Trying to enqueue %li incoming bytes...", 
        cl->id, cl->fd, n_bytes);
  }

  if(n_bytes == 0) {
    if(cfg->verbose_logging) {
      nob_log(
          WARNING, "[client=%" PRIu64 " fd=%i] Tried to enqueue zero length input bytes",
          cl->id, cl->fd);
    }
    return FAITH_OK;
  }

  if (cl->in_size > SIZE_MAX - n_bytes) 
    return FAITH_ERR_INVALID;

  size_t needed = cl->in_size + n_bytes;

  if(needed > cl->in_cap) {
    size_t new_cap = cl->in_cap ? cl->in_cap : 4096;

    while(new_cap < needed) {
      if(new_cap > SIZE_MAX / 2) 
        return FAITH_ERR_INVALID;
      new_cap *= 2;
    }

    uint8_t* p = realloc(cl->in_buf, new_cap); 
    if(!p) return FAITH_ERR_NOMEM;

    cl->in_buf = p;
    cl->in_cap = new_cap;
  }

  memcpy(cl->in_buf + cl->in_size, bytes, n_bytes);
  cl->in_size += n_bytes;

  if(cfg->verbose_logging) {
    nob_log(
        INFO, "[client=%" PRIu64 " fd=%i] Successfully enqueued %li incoming bytes...", 
        cl->id, cl->fd, n_bytes);
  }

  return FAITH_OK;
}

static faith_status_code_t enqueue_output_bytes(struct client_t* cl, const uint8_t* bytes, size_t n_bytes) {
  if(!cl) return FAITH_ERR_INVALID;
  
  if(n_bytes == 0) return FAITH_OK;

  if (cl->out_size > SIZE_MAX - n_bytes)
    return FAITH_ERR_INVALID;

  uint8_t* p = realloc(cl->out_buf, cl->out_size + n_bytes);
  if(!p) return FAITH_ERR_NOMEM;
  
  cl->out_buf = p;

  memcpy(cl->out_buf + cl->out_size, bytes, n_bytes);
  cl->out_size += n_bytes; 

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

static faith_status_code_t server_send_pong_ssl(struct client_t* cl, uint64_t nonce, uint64_t sent_at_ms) {
  uint8_t payload[sizeof(uint64_t) * 2];
  size_t payload_size = 0;

  _FH_CHECK_RETURN(encode_pong(payload, &payload_size, sizeof(payload),
        nonce, sent_at_ms)); 

  NOB_ASSERT(payload_size == sizeof(payload));

  size_t wire_size = FAITH_HEADER_SIZE + payload_size;
  uint8_t* wire_data = malloc(wire_size); 
  if (!wire_data) return FAITH_ERR_NOMEM;

  size_t frame_size = sizeof(uint16_t) + sizeof(uint16_t) + payload_size;
  faith_write_u32_be(wire_data, frame_size);
  faith_write_u16_be(wire_data + sizeof(uint32_t), FAITH_PROTO_VERSION);
  faith_write_u16_be(wire_data + sizeof(uint32_t) + sizeof(uint16_t), FAITH_MSG_PONG);

  memcpy(wire_data + FAITH_HEADER_SIZE, payload, payload_size);
  
  faith_status_code_t fc = enqueue_output_bytes(cl, wire_data, wire_size); 

  if(fc != FAITH_OK) {
    nob_log(ERROR, "enqueue_output_bytes()) failed: %s (%d)",
        faith_status_code_name(fc), (int)fc);   
  }

  free(wire_data);

  return FAITH_OK;
}

static faith_status_code_t handle_frame(int epfd, struct client_t* cl, faith_frame_t* frame) {
  if(!cl) return FAITH_ERR_INVALID;

  nob_log(INFO,
      "[client=%" PRIu64 " fd=%i] Server got msg_type=%s payload_size=%zu",
      cl->id,
      cl->fd,
      faith_frame_msg_name(frame->msg_type),
      frame->payload_size);

  if (frame->msg_type == FAITH_MSG_PING) {
    uint64_t nonce;
    uint64_t client_sent_at_ms;

    _FH_CHECK_RETURN(decode_ping(
        frame->payload,
        frame->payload_size,
        &nonce,
        &client_sent_at_ms));

    nob_log(INFO,
        "[client=%" PRIu64 " fd=%i] server got PING: nonce=%lu, client_sent_at_ms=%lu",
        cl->id,
        cl->fd,
        nonce,
        client_sent_at_ms);

    uint64_t server_sent_at_ms = faith_now_ms();

    _FH_CHECK_RETURN(server_send_pong_ssl(cl, nonce, server_sent_at_ms));

    modify_client_ev_mask(
        epfd,
        cl,
        EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLERR | EPOLLHUP);
    
    nob_log(INFO,
        "[client=%" PRIu64 " fd=%i] Server sent PONG to client. nonce=%lu, server_sent_at_ms=%lu",
        cl->id,
        cl->fd,
        nonce,
        server_sent_at_ms);
  }

  return FAITH_OK;
}

static enum read_frame_result_t read_more_ssl_bytes(struct client_t* cl, const struct server_cfg_t* cfg) {
  if(!cl || !cfg) return READ_FRAME_ERROR;

  uint8_t tmp[4096];

  int nread = SSL_read(cl->ssl, tmp, sizeof(tmp));

  if(nread > 0) {
    faith_status_code_t rc = enqueue_input_bytes(cl, tmp, (size_t)nread, cfg);

    if(rc != FAITH_OK)
      return READ_FRAME_ERROR;

    return READ_FRAME_GOT_BYTES;
  }

  int err = SSL_get_error(cl->ssl, nread);

  if (err == SSL_ERROR_WANT_READ)
    return READ_FRAME_WANT_READ;

  if (err == SSL_ERROR_WANT_WRITE)
    return READ_FRAME_WANT_WRITE;

  if (err == SSL_ERROR_ZERO_RETURN)
    return READ_FRAME_CLOSED;

  return READ_FRAME_ERROR;
}

static faith_status_code_t try_parse_frame_from_buffer(const uint8_t* buf,
                                                       size_t size,
                                                       faith_frame_t* frame,
                                                       size_t* consumed_out,
                                                       const struct server_cfg_t* cfg) {
  if (!frame || !consumed_out)
    return FAITH_ERR_INVALID;

  if (!buf) {
    if (size == 0) {
      nob_log(WARNING, "No frame data read, frame size is zero.");
      return FAITH_ERR_INCOMPLETE;
    }
    return FAITH_ERR_INVALID;
  }

  if(size < FAITH_HEADER_SIZE) {
    if(cfg->verbose_logging)
      nob_log(INFO, "Frame is incomplete, only %li bytes long.", size);
    return FAITH_ERR_INCOMPLETE;
  }

  uint32_t frame_size = faith_read_u32_be(buf + 0);
  
  if (frame_size < FAITH_HEADER_SIZE - (sizeof(uint16_t) + sizeof(uint16_t)))
    return FAITH_ERR_BAD_FRAME;

  if (frame_size > FAITH_MAX_FRAME_LEN)
    return FAITH_ERR_FRAME_TOO_LARGE;

  size_t total_frame_size = sizeof(uint32_t) + frame_size;

  if (size < total_frame_size) {
    return FAITH_ERR_INCOMPLETE;
  }
  
  uint16_t proto_ver  = faith_read_u16_be(buf + sizeof(uint32_t));
  uint16_t msg_type   = faith_read_u16_be(buf + sizeof(uint32_t) + sizeof(uint16_t));
  
  if (proto_ver != FAITH_PROTO_VERSION)
    return FAITH_ERR_UNSUPPORTED_VER;

  frame->proto_ver    = proto_ver;
  frame->msg_type     = msg_type;
  frame->frame_size   = frame_size;
  frame->payload_size = frame_size - (sizeof(proto_ver) + sizeof(msg_type));

  if(frame->payload_size > 0) {
    frame->payload = malloc(frame->payload_size);
    if(!frame->payload) return FAITH_ERR_NOMEM;
    
    memcpy(frame->payload, buf + FAITH_HEADER_SIZE, frame->payload_size);
  }

  *consumed_out = frame_size + sizeof(uint32_t);

  return FAITH_OK;
}

static enum read_frame_result_t try_read_one_frame(struct client_t* cl, 
    faith_frame_t* frame, const struct server_cfg_t* cfg) {
  while(1) {
    size_t consumed;

    if(cfg->verbose_logging) {
      nob_log(INFO,
          "[client=%" PRIu64 " fd=%i] Trying to parse frame buffer...", 
          cl->id,
          cl->fd);
    }

    faith_status_code_t rc = try_parse_frame_from_buffer(cl->in_buf, cl->in_size, frame, &consumed, cfg);

    if(rc == FAITH_OK) {

      if(cfg->verbose_logging) {
        nob_log(INFO,
            "[client=%" PRIu64 " fd=%i] Successfully parsed full frame from buffer (%li bytes)", 
            cl->id,
            cl->fd,
            consumed);
      }
      memmove(cl->in_buf, cl->in_buf + consumed, cl->in_size - consumed);
      cl->in_size -= consumed;

      return READ_FRAME_OK;
    }

    if (rc == FAITH_ERR_INCOMPLETE) {
      /* Not enough bytes yet, so read more decrypted TLS data. */
      if(cfg->verbose_logging) {
        nob_log(INFO,
            "[client=%" PRIu64 " fd=%i] Frame incomplete, reading more bytes...", 
            cl->id,
            cl->fd
            );
      }
      enum read_frame_result_t rr = read_more_ssl_bytes(cl, cfg);

      if (rr == READ_FRAME_GOT_BYTES) {
        if(cfg->verbose_logging) {
          nob_log(INFO,
              "[client=%" PRIu64 " fd=%i] Got new bytes, parsing frame again...", 
              cl->id,
              cl->fd
              );
        }
        continue;
      }

      if (rr == READ_FRAME_WANT_READ) {
        if (cfg->verbose_logging) {
          nob_log(INFO,
              "[client=%" PRIu64 " fd=%i] SSL_read needs to wait for socket to be readable", 
              cl->id,
              cl->fd);
        }
        return rr;
      }

      if (rr == READ_FRAME_WANT_WRITE) {
        if (cfg->verbose_logging) {
          nob_log(INFO,
              "[client=%" PRIu64 " fd=%i] SSL_read needs to wait for socket to be writable", 
              cl->id,
              cl->fd);
        }
        return rr;
      }

      if (rr == READ_FRAME_CLOSED) {
        nob_log(INFO,
            "[client=%" PRIu64 " fd=%i] Connection closed while reading incomplete frame.",
            cl->id,
            cl->fd);
        return rr;
      }

      nob_log(ERROR,
          "[client=%" PRIu64 " fd=%i] Error while reading incomplete frame.",
          cl->id,
          cl->fd);

      return rr;
    }

    nob_log(ERROR,
        "[client=%" PRIu64 " fd=%i] Failed to read frame", 
        cl->id,
        cl->fd
        );

    return READ_FRAME_ERROR;
  }
}

static int drive_client_read(int epfd, struct client_t* cl, const struct server_cfg_t* cfg) {
  for (;;) {
    faith_frame_t frame;

    if(cfg->verbose_logging) {
      nob_log(INFO, "[client=%" PRIu64 " fd=%i]: HANDLING NEW CLIENT FRAME.",
          cl->id, cl->fd);
    }
    enum read_frame_result_t rr = try_read_one_frame(cl, &frame, cfg);

    if (rr == READ_FRAME_OK) {
      int ok = handle_frame(epfd, cl, &frame);
      faith_frame_free(&frame);

      if (ok < 0)
        return -1;

      if(cfg->verbose_logging) {
        nob_log(INFO, "[client=%" PRIu64 " fd=%i]: SUCCESS HANDLING CLIENT FRAME.",
            cl->id, cl->fd);
      }

      if (cl->out_buf && cl->out_off < cl->out_size) {
        return 0;
      }
      continue;
    }

    if (rr == READ_FRAME_WANT_READ) {
      uint32_t mask = EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP;

      if (cl->out_buf && cl->out_off < cl->out_size) {
        mask |= EPOLLOUT;
      }

      modify_client_ev_mask(epfd, cl, mask);
      return 0;
    }

    if (rr == READ_FRAME_WANT_WRITE) {
      modify_client_ev_mask(epfd, cl,
          EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLERR | EPOLLHUP);
      return 0;
    }

    if (rr == READ_FRAME_CLOSED)
      return -1;

    nob_log(ERROR, "[client=%" PRIu64 " fd=%i]: Failed to read client frame.",
        cl->id, cl->fd);

    return -1;
  }
}

static void accept_clients(struct server_state_t* s) {
  while (1) {
    struct sockaddr_in addr;
    socklen_t addrsz = sizeof(addr);

    int client_fd = accept4(
        s->listenfd, (struct sockaddr*)&addr, 
        &addrsz, SOCK_NONBLOCK);

    if (client_fd < 0) {
      // no clients left to accept
      if(errno == EAGAIN || errno == EWOULDBLOCK) return;

      nob_log(ERROR, "accept4() failed: %s", strerror(errno));
      return;
    }

    struct client_t* cl = calloc(1, sizeof(*cl));
    if(!cl) {
      nob_log(WARNING, "failed to allocate memory with calloc() for pending client connection."); 
      close(client_fd);
      continue;
    }
    cl->id = atomic_fetch_add(&s->next_client_id, 1);

    nob_log(INFO, "accepted new client id=%" PRIu64 " fd=%i", cl->id, client_fd);


    cl->fd = client_fd;
    cl->state = CLIENT_HANDSHAKE;

    // create SSL object for client
    cl->ssl = SSL_new(s->ssl_ctx);
    if(!cl->ssl) {
      nob_log(ERROR, "SSL_new() failed for client connection (FD: %i)", client_fd); 
      close(client_fd);
      free(cl);
      continue;
    }
    
    // set file descriptor of ssl object
    SSL_set_fd(cl->ssl, cl->fd);
    SSL_set_accept_state(cl->ssl);

    struct epoll_event ev;
    memset(&ev, 0, sizeof(ev));

    ev.events = EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP;
    ev.data.ptr = cl;
    cl->evs_mask_want = ev.events;

    if (epoll_ctl(s->epoll_fd, EPOLL_CTL_ADD, client_fd, &ev) < 0) {
      nob_log(ERROR, "epoll_ctl() failed for client (FD: %i): %s", client_fd, strerror(errno)); 
      close_client(s->epoll_fd, cl);
      continue;
    }
  }
}

static faith_status_code_t drive_tls_handshake(int epoll_fd, struct client_t* cl) {
  int rc = SSL_accept(cl->ssl);

  uint32_t base_mask = EPOLLRDHUP | EPOLLERR | EPOLLHUP;

  if(rc == 1) {
    cl->state = CLIENT_OPEN;
    base_mask |= EPOLLIN;

    modify_client_ev_mask(
        epoll_fd, cl, 
        base_mask);

    return FAITH_OK;
  }

  int err = SSL_get_error(cl->ssl, rc);

  if (err == SSL_ERROR_WANT_READ) {
    base_mask |= EPOLLIN;

    modify_client_ev_mask(
        epoll_fd, cl, 
        base_mask); 

    return FAITH_OK;
  }

  if (err == SSL_ERROR_WANT_WRITE) {
    base_mask |= EPOLLOUT;

    modify_client_ev_mask(
        epoll_fd, cl, 
        base_mask); 

    return FAITH_OK;
  }

  nob_log(ERROR, "TLS handshake failed");
  ERR_print_errors_fp(stderr);

  return FAITH_ERR_IO;
}

static faith_status_code_t flush_client_output(int epoll_fd, struct client_t* cl) {

  if(!cl || (!cl->out_buf && cl->out_size > 0)) return FAITH_ERR_INVALID;
  if(!cl->out_buf) return FAITH_OK;

  while(cl->out_off < cl->out_size) {
    size_t remaining = cl->out_size - cl->out_off;

    int nwrite = SSL_write(cl->ssl, cl->out_buf + cl->out_off, (int)remaining); 

    if(nwrite > 0) {
      cl->out_off += (size_t)nwrite;
      continue;
    }

    int err = SSL_get_error(cl->ssl, nwrite);

    if(err == SSL_ERROR_WANT_WRITE || err == SSL_ERROR_WANT_READ) {
      uint32_t mask =  EPOLLRDHUP | EPOLLERR | EPOLLHUP | 
        (err == SSL_ERROR_WANT_WRITE ? EPOLLOUT : EPOLLIN);
      if(modify_client_ev_mask(
            epoll_fd, cl, 
            mask) < 0) {
        return FAITH_ERR_IO;
      }
      return 0;
    }

    nob_log(ERROR, "SSL_write failed");
    ERR_print_errors_fp(stderr);

    return FAITH_ERR_IO;
  }

  free(cl->out_buf);
  cl->out_buf = NULL;
  cl->out_size = 0;
  cl->out_off = 0;

  if(modify_client_ev_mask(
        epoll_fd, cl, 
        EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP) < 0) {
    return FAITH_ERR_IO;
  }

  return FAITH_OK;
}

int loop(struct server_state_t* s) {
  struct epoll_event events[MAX_EVENTS];

  while (!shutdown_requested) {
    int n = epoll_wait(s->epoll_fd, events, MAX_EVENTS, -1);

    if (n < 0) {
      if (errno == EINTR)
        continue;

      nob_log(ERROR, "epoll_wait() failed: %s", strerror(errno));

      return 1;
    }

    for (int i = 0; i < n; i++) {
      uint32_t revents = events[i].events;

      if (events[i].data.fd == s->listenfd) {
        accept_clients(s);
        continue;
      }

      struct client_t *c = events[i].data.ptr;

      if (revents & (EPOLLERR | EPOLLHUP | EPOLLRDHUP)) {
        close_client(s->epoll_fd, c);
        continue;
      }

      int dead = 0;

      if (c->state == CLIENT_HANDSHAKE) {
        if (drive_tls_handshake(s->epoll_fd, c) < 0)
          dead = 1;
      } else if (c->state == CLIENT_OPEN) {
        if ((revents & EPOLLIN) && drive_client_read(s->epoll_fd, c, &s->cfg) < 0)
          dead = 1;

        if (!dead && (revents & EPOLLOUT) && flush_client_output(s->epoll_fd, c) < 0)
          dead = 1;
      }

      if (dead)
        close_client(s->epoll_fd, c);
    }
  }

  server_destroy(s);
  
  return 0;
}

static int init_ssl(SSL_CTX** ctx)
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

static int init_listen_sock(void) {
  struct sockaddr_in servaddr; 

  int listenfd = -1;

  listenfd = socket(AF_INET, SOCK_STREAM, 0); 
  if (listenfd == -1) { 
    nob_log(ERROR, "socket creation failed"); 
    return -1;
  } 

  if(set_nonblocking(listenfd) < 0) {
    nob_log(ERROR, "failed to set socket to O_NONBLOCK"); 
    close(listenfd);
    return 1;
  }

  nob_log(INFO, "Socket successfully created"); 

  memset(&servaddr, 0, sizeof(servaddr)); 

  servaddr.sin_family = AF_INET; 
  servaddr.sin_addr.s_addr = htonl(INADDR_ANY); 
  servaddr.sin_port = htons(PORT); 

  int yes = 1;
  if (setsockopt(listenfd, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes)) < 0) {
    nob_log(ERROR, "setsockopt(SO_REUSEADDR) failed: %s", strerror(errno));
    close(listenfd);
    return -1;
  }

  if ((bind(listenfd, (struct sockaddr*)&servaddr, sizeof(servaddr))) != 0) { 
    nob_log(ERROR, "socket bind failed"); 
    close(listenfd);
    return -1;
  } 

  nob_log(INFO, "Socket successfully bound"); 

  if ((listen(listenfd, SOMAXCONN)) != 0) { 
    nob_log(ERROR, "listen() failed: %s", strerror(errno)); 
    close(listenfd);
    return -1;
  }

  nob_log(INFO, "Server listening.."); 

  return listenfd;
}

static int init_epoll_fd(int listenfd)  {
  int epoll_fd = epoll_create1(EPOLL_CLOEXEC);
  if (epoll_fd == -1) {
    nob_log(ERROR, "epoll_create1(EPOLL_CLOEXEC) failed: %s", strerror(errno));
    return -1;
  }
  struct epoll_event ev = {0};

  ev.events = EPOLLIN;
  ev.data.fd = listenfd;

  if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, listenfd, &ev) == -1) {
    nob_log(ERROR, "Failed to add listening FD to epoll FD with epoll_ctl: %s", strerror(errno));
    close(epoll_fd);
    return -1;
  }

  return epoll_fd;
}

static int install_signal_handlers(void)
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

static void print_usage(const char* prog) {
  printf(
      "Usage: %s [options]\n"
      "\n"
      "Options:\n"
      "  -v, --verbose    Enable verbose logging\n"
      "  -h, --help       Show this help message\n",
      prog);
}

static int parse_args(int argc, char** argv, struct server_state_t* s) {
  if (!s) return 1;

  for (int i = 1; i < argc; i++) {
    const char* arg = argv[i];

    if (strcmp(arg, "-v") == 0 || strcmp(arg, "--verbose") == 0) {
      s->cfg.verbose_logging = 1;
      continue;
    }

    if (strcmp(arg, "-h") == 0 || strcmp(arg, "--help") == 0) {
      print_usage(argv[0]);
      return 2;
    }

    fprintf(stderr, "Unknown option: %s\n\n", arg);
    print_usage(argv[0]);
    return 1;
  }

  return 0;
}

int main(int argc, char** argv) {
  SSL_library_init();
  SSL_load_error_strings();
  OpenSSL_add_ssl_algorithms();

  struct server_state_t s = {
    .listenfd = -1,
    .epoll_fd = -1,
    .ssl_ctx = NULL,
    .next_client_id = 1,
    .cfg = {0}
  };

  int arg_rc = parse_args(argc, argv, &s);
  if (arg_rc == 2) {
    return 0;
  }
  if (arg_rc != 0) {
    return 1;
  }

  if (install_signal_handlers() != 0) exit(1);
  if (init_ssl(&s.ssl_ctx) != 0) {
    server_destroy(&s);
    exit(1);
  }
  if ((s.listenfd = init_listen_sock()) < 0) {
    server_destroy(&s);
    exit(1);
  }

  if ((s.epoll_fd = init_epoll_fd(s.listenfd)) < 0) {
    server_destroy(&s);
    exit(1);
  }

  return loop(&s);
}
