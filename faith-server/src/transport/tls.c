#include "tls.h"
#include <openssl/ssl.h>

#include "../logging/logging.h"

#define _MODULE_NAME "[transport/tls]: "

void tls_init_global(void) {
  SSL_library_init();
  SSL_load_error_strings();
  OpenSSL_add_ssl_algorithms();
}

faith_status_code_t tls_init(const tls_config_t *cfg, tls_context_t *o_tls) {
  if (!cfg)
    return FAITH_ERR_INVALID;

  *o_tls = (tls_context_t){.cfg = *cfg, .ctx = NULL};

  faith_status_code_t _fh_result = 0;

  SSL_CTX *ctx = NULL;
  ctx = SSL_CTX_new(TLS_server_method());
  if (ctx == NULL) {
    nob_log(ERROR, _MODULE_NAME "Failed to create server SSL_CTX");
    _FH_RETURN_DEFER(FAITH_ERR_NOMEM);
  }

  if (!SSL_CTX_set_min_proto_version(ctx, TLS1_2_VERSION)) {
    nob_log(ERROR,
            _MODULE_NAME "Failed to set the minimum TLS protocol version");
    _FH_RETURN_DEFER(FAITH_ERR_SSL);
  }

  SSL_CTX_set_options(ctx, cfg->options);

  if (SSL_CTX_use_certificate_chain_file(ctx, cfg->chain_file) <= 0) {
    nob_log(ERROR,
            _MODULE_NAME "Failed to load the server certificate chain file");
    _FH_RETURN_DEFER(FAITH_ERR_CRYPTO);
  }

  if (SSL_CTX_use_PrivateKey_file(ctx, cfg->pkey_file, SSL_FILETYPE_PEM) <= 0) {
    nob_log(ERROR, _MODULE_NAME "Failed loading the server private key file, "
                                "possible key/cert mismatch?");
    _FH_RETURN_DEFER(FAITH_ERR_CRYPTO);
  }

  static const char cache_id[] = "faithd-server";

  SSL_CTX_set_session_id_context(ctx, (void *)cache_id, sizeof(cache_id));
  SSL_CTX_set_session_cache_mode(ctx, SSL_SESS_CACHE_SERVER);
  SSL_CTX_sess_set_cache_size(ctx, cfg->cache_size);
  SSL_CTX_set_timeout(ctx, cfg->timeout);
  SSL_CTX_set_verify(ctx, cfg->verification_mode, NULL);

  nob_log(INFO, _MODULE_NAME "OpenSSL initialized.");

  (*o_tls).ctx = ctx;

  return FAITH_OK;

defer:
  ERR_print_errors_fp(stderr);
  if (ctx) {
    SSL_CTX_free(ctx);
  }
  return _fh_result;
}

faith_status_code_t tls_destroy(tls_context_t *tls) {
  if (!tls)
    return FAITH_ERR_INVALID;

  if (tls->ctx != NULL) {
    SSL_CTX_free(tls->ctx);
  }

  memset(tls, 0, sizeof(*tls));

  return FAITH_OK;
}

faith_status_code_t tls_new_with_fd(tls_context_t *tls, int fd,
                                    tls_state_fd_t *o_state) {
  if (!tls || fd < 0 || !o_state)
    return FAITH_ERR_INVALID;

  *o_state = (tls_state_fd_t){.fd = fd, .ssl = NULL};

  SSL *ssl = SSL_new(tls->ctx);
  if (!ssl) {
    nob_log(ERROR, _MODULE_NAME "SSL_new() failed for connection (FD: %i)", fd);
    return FAITH_ERR_SSL;
  }

  SSL_set_fd(ssl, fd);
  SSL_set_accept_state(ssl);

  (*o_state).ssl = ssl;

  if (g_verbose_logging) {
    nob_log(
        INFO,
        _MODULE_NAME
        "Initialized SSL and set accept state for TLS connection with fd: %i",
        fd);
  }

  return FAITH_OK;
}

int tls_accept(tls_state_fd_t *state) {
  if (!state || !state->ssl)
    return INT_MAX;

  ERR_clear_error();

  int rc = SSL_accept(state->ssl);

  if (rc == 1) {
    if (g_verbose_logging) {
      nob_log(INFO,
              _MODULE_NAME "TLS handshake completed for connection with fd: %i",
              state->fd);
    }

    return SSL_ERROR_NONE;
  }

  int err = SSL_get_error(state->ssl, rc);

  if (err == SSL_ERROR_WANT_READ) {
    if (g_verbose_logging)
      nob_log(INFO, _MODULE_NAME "TLS handshake wants read for fd: %i",
              state->fd);
  } else if (err == SSL_ERROR_WANT_WRITE) {
    if (g_verbose_logging)
      nob_log(INFO, _MODULE_NAME "TLS handshake wants write for fd: %i",
              state->fd);
  } else {
    nob_log(ERROR, _MODULE_NAME "SSL_accept failed: rc=%i ssl_error=%i fd=%i",
            rc, err, state->fd);
  }

  return err;
}

int tls_read(tls_state_fd_t *state, void *buf, int len, int *o_nread) {
  if (!state || !state->ssl || !buf || !o_nread || len <= 0)
    return INT_MAX;

  *o_nread = 0;

  ERR_clear_error();

  int rc = SSL_read(state->ssl, buf, len);

  if (rc > 0) {
    *o_nread = rc;
    return SSL_ERROR_NONE;
  }

  return SSL_get_error(state->ssl, rc);
}

int tls_write(tls_state_fd_t *state, const void *buf, int nwrite,
              int *o_nwritten) {
  if (!state || !state->ssl || !buf || !o_nwritten || nwrite <= 0)
    return INT_MAX;

  *o_nwritten = 0;

  ERR_clear_error();
  int rc = SSL_write(state->ssl, buf, nwrite);

  if (rc > 0) {
    *o_nwritten = rc;
    return SSL_ERROR_NONE;
  }
  return SSL_get_error(state->ssl, *o_nwritten);
}

faith_status_code_t tls_shutdown(tls_state_fd_t *state) {
  if (!state)
    return FAITH_ERR_INVALID;
  if (!state->ssl)
    return FAITH_ERR_SSL;

  /* Frees the local TLS state. It does not perform a graceful
   * TLS shutdown exchange with the peer. */
  SSL_set_shutdown(state->ssl, SSL_SENT_SHUTDOWN | SSL_RECEIVED_SHUTDOWN);
  SSL_free(state->ssl);

  memset(state, 0, sizeof(*state));

  return FAITH_OK;
}

int tls_BIO_ctrl_pending(const tls_state_fd_t *state) {
  if (!state)
    return INT_MAX;
  return BIO_ctrl_pending(SSL_get_wbio(state->ssl));
}
