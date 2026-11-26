#include "../include/faith-client/mls.h"
#include "faith-proto/core/crypto.h"
#include <openssl/evp.h>

faith_mls_client_t *
faith_mls_client_create(const faith_auth_id_t   *auth_id,
                        const faith_device_id_t *device_id) {
  if (!auth_id || !device_id)
    return NULL;

  faith_mls_client_t *mls_client = calloc(1, sizeof(*mls_client));
  if (!mls_client) {
    return NULL;
  }

  _FH_CHECK(faith_gen_ed25519_keypair(&mls_client->signing_key.pair,
                                      mls_client->signing_key.pub,
                                      mls_client->signing_key.prv));

  mls_client->auth_id = *auth_id;
  mls_client->device_id = *device_id;

  return mls_client;
}

void faith_mls_client_destroy(faith_mls_client_t *mls_client) {
  if(!mls_client) return;

  EVP_PKEY_free(mls_client->signing_key.pair);
  OPENSSL_cleanse(mls_client->signing_key.prv, sizeof(mls_client->signing_key.prv));
  free(mls_client);
}
