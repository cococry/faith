#pragma once

#include "../core/core.h"

#include <openssl/err.h>

typedef struct {
  long        options;
  const char *chain_file;
  const char *pkey_file;

  long timeout;
  long cache_size;

  int verification_mode;
} tls_config_t;

typedef struct {
  SSL_CTX     *ctx;
  tls_config_t cfg;
} tls_context_t;

typedef struct {
  SSL *ssl;
  int  fd;
} tls_state_fd_t;

void tls_init_global(void);

faith_status_code_t tls_init(const tls_config_t *cfg, tls_context_t *o_tls);

faith_status_code_t tls_new_with_fd(tls_context_t *tls, int fd,
                                    tls_state_fd_t *o_state);

/* INT_MAX is returned for invalid argument errors. A return value of INT_MAX
 * does not mean there was an SSL error.*/
int tls_accept(tls_state_fd_t *state);

/* INT_MAX is returned for invalid argument errors. A return value of INT_MAX
 * does not mean there was an SSL error.*/
int tls_read(tls_state_fd_t *state, void *buf, int nread, int *o_nread);

/* INT_MAX is returned for invalid argument errors. A return value of INT_MAX
 * does not mean there was an SSL error.*/
int tls_write(tls_state_fd_t *state, const void *buf, int nwrite,
              int *o_nwritten);

/* INT_MAX is returned for invalid argument errors. A return value of INT_MAX
 * does not mean there was an SSL error.*/
int tls_BIO_ctrl_pending(const tls_state_fd_t *state);

faith_status_code_t tls_shutdown(tls_state_fd_t *state);

faith_status_code_t tls_destroy(tls_context_t *tls);
