#include "core/crypto.h"
#include <openssl/evp.h>
#include <openssl/rand.h>

#define _MODULE_NAME "core/crypto"

faith_status_code_t faith_random_bytes(uint8_t *o_buf, int num) {
  if (RAND_bytes(o_buf, num) != 1) {
    nob_log(ERROR, "Failed to generate auth_id with OpenSSL RAND_bytes()");
    return FAITH_ERR_SSL;
  }
  return FAITH_OK;
}

faith_status_code_t
faith_gen_ed25519_keypair(void   *handle,
                          uint8_t private_key[FAITH_ED25519_PRIVATE_KEY_SIZE],
                          uint8_t public_key[FAITH_ED25519_PUBLIC_KEY_SIZE]) {
  if (!handle || !private_key || !public_key)
    return FAITH_ERR_INVALID;

  faith_status_code_t status = FAITH_OK;

  EVP_PKEY    **out_keypair = (EVP_PKEY **)handle;
  EVP_PKEY     *keypair = NULL;
  EVP_PKEY_CTX *ctx = NULL;

  ctx = EVP_PKEY_CTX_new_id(EVP_PKEY_ED25519, NULL);
  if (!ctx) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  if (EVP_PKEY_keygen_init(ctx) != 1) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  if (EVP_PKEY_keygen(ctx, &keypair) != 1) {
    status = FAITH_ERR_CRYPTO;
    goto cleanup;
  }

  {
    size_t priv_len = 0;

    if (EVP_PKEY_get_raw_private_key(keypair, NULL, &priv_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }

    if (priv_len != FAITH_ED25519_PRIVATE_KEY_SIZE) {
      nob_log(ERROR, "[%s] Generated private key has unexpected byte size: %zu",
              _MODULE_NAME, priv_len);
      status = FAITH_ERR_INVALID;
      goto cleanup;
    }

    if (EVP_PKEY_get_raw_private_key(keypair, private_key, &priv_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }
  }

  {
    size_t pub_len = 0;

    if (EVP_PKEY_get_raw_public_key(keypair, NULL, &pub_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }

    if (pub_len != FAITH_ED25519_PUBLIC_KEY_SIZE) {
      nob_log(ERROR, "[%s] Generated public key has unexpected byte size: %zu",
              _MODULE_NAME, pub_len);
      status = FAITH_ERR_INVALID;
      goto cleanup;
    }

    if (EVP_PKEY_get_raw_public_key(keypair, public_key, &pub_len) != 1) {
      status = FAITH_ERR_CRYPTO;
      goto cleanup;
    }
  }

  *out_keypair = keypair;
  keypair = NULL;

cleanup:
  EVP_PKEY_free(keypair);
  EVP_PKEY_CTX_free(ctx);
  return status;
}

faith_status_code_t faith_gen_signature(EVP_PKEY *keypair, uint8_t *o_signature,
                                        size_t        *o_signature_size,
                                        const uint8_t *msg_input,
                                        size_t         msg_size) {
  if (!keypair || !o_signature || !o_signature_size ||
      (!msg_input && msg_size != 0)) {
    return FAITH_ERR_INVALID;
  }

  EVP_MD_CTX *ctx = EVP_MD_CTX_new();
  if (!ctx)
    return FAITH_ERR_NOMEM;

  faith_status_code_t result = FAITH_ERR_CRYPTO;

  if (EVP_DigestSignInit(ctx, NULL, NULL, NULL, keypair) != 1)
    goto defer;

  size_t required_size = 0;

  if (EVP_DigestSign(ctx, NULL, &required_size, msg_input, msg_size) != 1) {
    goto defer;
  }

  if (required_size != FAITH_ED25519_SIGNATURE_SIZE)
    goto defer;

  size_t produced_size = required_size;

  if (EVP_DigestSign(ctx, o_signature, &produced_size, msg_input, msg_size) !=
      1) {
    goto defer;
  }

  if (produced_size != FAITH_ED25519_SIGNATURE_SIZE)
    goto defer;

  *o_signature_size = produced_size;
  result = FAITH_OK;

defer:
  EVP_MD_CTX_free(ctx);
  return result;
}

faith_status_code_t faith_verify_signature(EVP_PKEY      *public_key,
                                           const uint8_t *msg_input,
                                           size_t         msg_size,
                                           const uint8_t *signature,
                                           size_t         signature_size) {
  if (!public_key || (!msg_input && msg_size != 0) || !signature ||
      signature_size != FAITH_ED25519_SIGNATURE_SIZE) {
    return FAITH_ERR_INVALID;
  }

  EVP_MD_CTX *ctx = EVP_MD_CTX_new();
  if (!ctx)
    return FAITH_ERR_NOMEM;

  faith_status_code_t _fh_result = FAITH_ERR_CRYPTO;

  if (EVP_DigestVerifyInit(ctx, NULL, NULL, NULL, public_key) != 1)
    _FH_RETURN_DEFER(_fh_result);

  int rc =
      EVP_DigestVerify(ctx, signature, signature_size, msg_input, msg_size);

  if (rc == 1)
    _fh_result = FAITH_OK;
  else if (rc == 0)
    _fh_result = FAITH_ERR_NOT_EQUAL;
  else
    _fh_result = FAITH_ERR_CRYPTO;

defer:
  EVP_MD_CTX_free(ctx);
  return _fh_result;
}

faith_status_code_t faith_verify_signature_raw_pubkey(
    const uint8_t  public_key[FAITH_ED25519_PUBLIC_KEY_SIZE],
    const uint8_t *msg_input, size_t msg_size, const uint8_t *signature,
    size_t signature_size) {
  if (!public_key || (!msg_input && msg_size != 0) || !signature ||
      signature_size != FAITH_ED25519_SIGNATURE_SIZE) {
    return FAITH_ERR_INVALID;
  }

  EVP_PKEY *pkey = EVP_PKEY_new_raw_public_key(
      EVP_PKEY_ED25519, NULL, public_key, FAITH_ED25519_PUBLIC_KEY_SIZE);

  if (!pkey)
    return FAITH_ERR_CRYPTO;

  faith_status_code_t rc = faith_verify_signature(pkey, msg_input, msg_size,
                                                  signature, signature_size);

  EVP_PKEY_free(pkey);
  return rc;
}
