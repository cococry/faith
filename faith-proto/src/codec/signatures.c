#include "codec/signatures.h"

#include "codec/helpers.h"

faith_status_code_t faith_gen_sign_buf_hello_handshake(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_hello_handshake_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id.bytes,
                      sizeof(in->auth_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->device_id.bytes,
                      sizeof(in->device_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->public_key,
                      sizeof(in->public_key));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->client_nonce);

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->server_nonce);

  FAITH_ENCODE_EPILOGUE(FAITH_SIGNATURE_HELLO_HANDSHAKE_SIZE, !=);

  return FAITH_OK;
}

faith_status_code_t faith_gen_sign_buf_device_link_response(
    uint8_t *out_buf, size_t *out_size, size_t buf_cap_in_bytes,
    const faith_signature_device_link_response_t *in) {

  FAITH_ENCODE_PROLOGUE(FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE);

  size_t offset = 0;

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->auth_id.bytes,
                      sizeof(in->auth_id.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_new.bytes, sizeof(in->device_id_new.bytes));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->public_key_new_device,
                      sizeof(in->public_key_new_device));

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset, in->code,
                      sizeof(in->code));

  FAITH_ENCODE_U64_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             in->expires_at_ms);

  FAITH_APPEND_RETURN(out_buf, buf_cap_in_bytes, offset,
                      in->device_id_responding.bytes,
                      sizeof(in->device_id_responding.bytes));

  FAITH_ENCODE_U32_BE_RETURN(out_buf, buf_cap_in_bytes, offset,
                             (uint32_t)in->type);

  return offset == FAITH_SIGNATURE_DEVICE_LINK_RESPONSE_SIZE
             ? FAITH_OK
             : FAITH_ERR_INVALID;
}
