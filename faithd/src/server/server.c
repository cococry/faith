#define _GNU_SOURCE

#include "server.h"
#include "../logging/logging.h"
#include "client_io.h"
#include "client_lifecycle.h"

#include <netinet/in.h>
#include <openssl/ssl.h>
#include <signal.h>
#include <sys/socket.h>

#include "../../third_party/nob.h"
#include "../../third_party/stb_ds.h"

static const char *client_state_name(client_state_t state);
static void        accept_clients(server_state_t *s);
static void        handle_client_event(server_state_t *s, client_conn_t *cl,
                                       reactor_events_t events, bool shutting_down);
static void        handle_shutdown_signal(int sig);
static int         install_signal_handlers(void);
static faith_status_code_t init_listener(int port);
static void                begin_shutdown(server_state_t *s);

static volatile sig_atomic_t shutdown_requested = 0;

static const char *client_state_name(client_state_t state) {
  switch (state) {
#define X(name, value)                                                         \
  case name:                                                                   \
    return #name;
    CLIENT_STATES(X)
#undef X
  default:
    return "FAITH_EVENT_UNKNOWN";
  }
}

static void accept_clients(server_state_t *s) {
  while (1) {

    struct sockaddr_storage addr;
    socklen_t               addr_size = sizeof addr;

    int client_fd = accept4(s->listen_source.fd, (struct sockaddr *)&addr,
                            &addr_size, SOCK_NONBLOCK | SOCK_CLOEXEC);

    if (client_fd < 0) {
      /* No clients left to accept */
      if (errno == EAGAIN || errno == EWOULDBLOCK)
        return;

      if (errno == EINTR)
        continue;

      nob_log(ERROR, "accept4() failed: %s", strerror(errno));
      return;
    }

    _FH_CHECK(server_client_adopt_fd(s, client_fd));
  }
}

static void handle_client_event(server_state_t *s, client_conn_t *cl,
                                reactor_events_t events, bool shutting_down) {
  if (!s || !cl)
    return;

  if (events & (REACTOR_CLOSED | REACTOR_ERROR)) {
    _FH_CHECK(server_close_client(s, &cl));
    return;
  }

  if (cl->closing) {
    _FH_CHECK(server_close_client(s, &cl));
    return;
  }

  bool dead = false;

  if (shutting_down || cl->close_after_flush) {
    if (conn_output_empty(&cl->conn)) {
      _FH_CHECK(server_close_client(s, &cl));
      return;
    }

    if (events & (REACTOR_READABLE | REACTOR_WRITABLE)) {
      if (server_flush_client_output(s, cl) != FAITH_OK)
        dead = true;
    }
  } else if (cl->state == CLIENT_HANDSHAKE) {
    if (events & (REACTOR_READABLE | REACTOR_WRITABLE)) {
      if (server_drive_tls_handshake(s, cl) != FAITH_OK)
        dead = true;
    }
  } else if (cl->state == CLIENT_OPEN || cl->state == CLIENT_WAIT_FOR_HELLO ||
             cl->state == CLIENT_WAIT_FOR_CHALLENGE_RESPONSE ||
             cl->state == CLIENT_WAIT_FOR_DEVICE_LINK_RESPONSE) {
    if ((events & REACTOR_READABLE) &&
        server_drive_client_read(s, cl) != FAITH_OK) {
      dead = true;
    }

    if (!dead && !cl->closing && (events & REACTOR_WRITABLE) &&
        server_flush_client_output(s, cl) != FAITH_OK) {
      dead = true;
    }
  }

  if (dead || cl->closing) {
    _FH_CHECK(server_close_client(s, &cl));
    return;
  }

  if (cl->close_after_flush && conn_output_empty(&cl->conn)) {
    _FH_CHECK(server_close_client(s, &cl));
  }
}

static void handle_shutdown_signal(int sig) {
  (void)sig;
  shutdown_requested = 1;
}

static int install_signal_handlers(void) {
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

static faith_status_code_t init_listener(int port) {
  struct sockaddr_in servaddr;

  int listenfd = -1;

  listenfd = socket(AF_INET, SOCK_STREAM, 0);
  if (listenfd == -1) {
    nob_log(ERROR, "Socket creation failed");
    return -1;
  }

  // get flags from file descriptor
  int flags;
  if ((flags = fcntl(listenfd, F_GETFL, 0)) < 0) {
    nob_log(ERROR, "Failed to set socket to O_NONBLOCK");
    return -1;
  }
  // add nonblocking flag
  if (fcntl(listenfd, F_SETFL, flags | O_NONBLOCK) < 0) {
    nob_log(ERROR, "Failed to set socket to O_NONBLOCK");
    return -1;
  }

  nob_log(INFO, "Socket successfully created");

  memset(&servaddr, 0, sizeof(servaddr));

  servaddr.sin_family = AF_INET;
  servaddr.sin_addr.s_addr = htonl(INADDR_ANY);
  servaddr.sin_port = htons(port);

  int yes = 1;
  if (setsockopt(listenfd, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes)) < 0) {
    nob_log(ERROR, "setsockopt(SO_REUSEADDR) failed: %s", strerror(errno));
    close(listenfd);
    return -1;
  }

  if ((bind(listenfd, (struct sockaddr *)&servaddr, sizeof(servaddr))) != 0) {
    nob_log(ERROR, "Socket bind failed on port %i: %s", port, strerror(errno));
    close(listenfd);
    return -1;
  }

  nob_log(INFO, "Socket successfully bound");

  if ((listen(listenfd, SOMAXCONN)) != 0) {
    nob_log(ERROR, "listen() failed: %s", strerror(errno));
    close(listenfd);
    return -1;
  }

  nob_log(INFO, "Server listening on %i...", port);

  return listenfd;
}

static void begin_shutdown(server_state_t *s) {
  struct client_conn_t *cl = s->clients;
  while (cl != NULL) {
    struct client_conn_t *next = cl->next;

    _FH_CHECK(server_client_queue_disconnect(
        s, cl, FAITH_DISCONNECT_SERVER_SHUTDOWN,
        FAITH_CLIENT_RECONNECT_FORBIDDEN, 0, 0, "The server has shut down"));
    cl = next;
  }
}

/*========================================== */
/* PUBLIC API - public api - public API */
/*==========================================*/

faith_status_code_t server_init(server_state_t *s, const server_config_t *cfg) {
  if (!s || !cfg)
    return FAITH_ERR_INVALID;

  s->cfg = *cfg;

  nob_set_log_handler(faith_log_handler);

  if (install_signal_handlers() != 0)
    exit(1);

  faith_status_code_t _fh_result = FAITH_OK;

  const tls_config_t tls_cfg = (tls_config_t){
      .options = SSL_OP_IGNORE_UNEXPECTED_EOF | SSL_OP_NO_RENEGOTIATION |
                 SSL_OP_SERVER_PREFERENCE,
      .chain_file = "chain.pem",
      .pkey_file = "pkey.pem",
      .timeout = 3600,
      .cache_size = 1024,
      .verification_mode = SSL_VERIFY_NONE,
  };

  _FH_CHECK_DEFER(tls_init(&tls_cfg, &s->tls));

  _FH_CHECK_DEFER(reactor_init(&s->reactor));
  int listen_fd = init_listener(s->cfg.listen_port);
  if (listen_fd < 0) {
    server_destroy(s);
    return 1;
  }

  s->listen_source = (reactor_source_t){.type = REACTOR_SOURCE_LISTENER,
                                        .fd = listen_fd,
                                        .user_data = NULL,
                                        .interests = REACTOR_READABLE};

  _FH_CHECK_DEFER(reactor_add(&s->reactor, &s->listen_source));

  return 0;
defer:
  server_destroy(s);
  return _fh_result;
}

faith_status_code_t server_loop(server_state_t *s) {
  bool     shutting_down = false;
  uint64_t shutdown_deadline_ms = 0;

  static reactor_event_data_t events[REACTOR_MAX_EVENTS];

  for (;;) {
    if (shutdown_requested && !shutting_down) {
      shutting_down = true;

      nob_log(INFO, "Server shutdown requested; notifying clients.");

      _FH_CHECK_SCOPED(reactor_remove(&s->reactor, &s->listen_source));

      begin_shutdown(s);

      shutdown_deadline_ms = faith_now_ms() + 3000;
    }

    if (shutting_down) {
      if (!s->clients)
        break;

      if (faith_now_ms() >= shutdown_deadline_ms) {
        nob_log(
            WARNING,
            "Shutdown flush deadline reached; forcing all clients to close.");

        while (s->clients != NULL) {
          struct client_conn_t *cl = s->clients;
          _FH_CHECK(server_close_client(s, &cl));
        }

        break;
      }
    }

    int timeout_ms = shutting_down ? 100 : -1;

    size_t n_events = 0;
    _FH_CHECK(reactor_wait(&s->reactor, timeout_ms, events, &n_events));

    for (size_t i = 0; i < n_events; ++i) {
      reactor_source_t *src = events[i].src;

      if (!src) {
        nob_log(ERROR, "reactor_wait() returned an event with no source");
        continue;
      }

      switch (src->type) {
      case REACTOR_SOURCE_LISTENER:
        if (!shutting_down) {
          accept_clients(s);
        }
        break;
      case REACTOR_SOURCE_CLIENT:
        if (!src->user_data) {
          nob_log(ERROR, "client reactor source has no user_data");
          continue;
        }
        handle_client_event(s, (struct client_conn_t *)src->user_data,
                            events[i].events, shutting_down);
        break;
      default:
        nob_log(ERROR, "Unknown reactor source type: %u", src->type);
        break;
      }
    }
  }

  return 0;
}

faith_status_code_t server_destroy(server_state_t *s) {
  if (!s)
    return FAITH_ERR_INVALID;

  nob_log(INFO, "Destroying server context...");

  while (s->clients) {
    struct client_conn_t *cl = s->clients;
    server_close_client(s, &cl);
  }

  _FH_CHECK_SCOPED(sess_registry_destroy(&s->rt));

  _FH_CHECK_SCOPED(reactor_destroy(&s->reactor));

  _FH_CHECK_SCOPED(tls_destroy(&s->tls));

  nob_log(INFO, "Destroyed server context.");

  return FAITH_OK;
}

void server_link_client(server_state_t *s, client_conn_t *cl) {
  if (!s || !cl)
    return;

  cl->prev = NULL;
  cl->next = s->clients;

  if (s->clients) {
    s->clients->prev = cl;
  }
  s->clients = cl;
}

void server_unlink_client(server_state_t *s, client_conn_t *cl) {
  if (!s || !cl)
    return;

  if (cl->prev) {
    cl->prev->next = cl->next;
  } else {
    s->clients = cl->next;
  }

  if (cl->next) {
    cl->next->prev = cl->prev;
  }
  cl->next = NULL;
  cl->prev = NULL;
}

void server_set_client_state(server_state_t *s, struct client_conn_t *cl,
                             client_state_t state) {
  /* TODO: remove <s> from parameter list*/
  (void)s;
  if (!cl)
    return;
  cl->state = state;

  if (g_verbose_logging) {
    nob_log(INFO, "[client=%" PRIu64 " fd=%i] Client changed state to %s",
            cl->conn.id, cl->conn.fd, client_state_name(state));
  }
}
