#pragma once

#include "../codec/envelopes.h"
#include "../codec/msg.h"
#include "../core/core.h"

#include "../auth/envelopes.h"
#include "../server/envelopes.h"

#include "commands.h"

#include "helpers.h"

#include <stdint.h>
#include <stdlib.h>

#define FAITH_MAX_PAYLOAD_SIZE 256
#define FAITH_MAX_FRAME_LEN    512

#define FAITH_FRAME_LENGTH_SIZE sizeof(uint32_t)

#define FAITH_FRAME_METADATA_SIZE                                              \
  (_FAITH_BODY_SIZE(sizeof(uint16_t) /* protocol version */ +                  \
                    sizeof(uint16_t) /* message type */))

#define FAITH_FRAME_HEADER_SIZE                                                \
  (_FAITH_BODY_SIZE(FAITH_FRAME_LENGTH_SIZE + FAITH_FRAME_METADATA_SIZE))

#define FAITH_ENVL_HEADER_SIZE                                                 \
  (sizeof(uint32_t) /* envelope type */ + FAITH_AUTH_ID_SIZE /* sender id */ + \
   FAITH_AUTH_ID_SIZE /* recipient id  */ +                                    \
   sizeof(faith_body_size_t) /* body size */)

typedef struct {
  uint32_t frame_size;
  uint16_t proto_ver;
  uint16_t msg_type;

  uint8_t          *payload;
  faith_body_size_t payload_size;
} faith_frame_t;

faith_status_code_t faith_encode_frame(uint8_t *out_buf, size_t *out_size,
                                       size_t               buf_cap_in_bytes,
                                       const faith_frame_t *in);

faith_status_code_t faith_decode_frame(const uint8_t *payload,
                                       size_t payload_size, faith_frame_t *out);

void faith_free_frame(faith_frame_t *frame);

faith_status_code_t faith_encode_ping(uint8_t                *out_buf,
                                      faith_body_size_t      *out_size,
                                      size_t                  buf_cap_in_bytes,
                                      const faith_msg_ping_t *in);

faith_status_code_t faith_decode_ping(const uint8_t    *payload,
                                      size_t            payload_size,
                                      faith_msg_ping_t *out);

faith_status_code_t faith_encode_pong(uint8_t                *out_buf,
                                      faith_body_size_t      *out_size,
                                      size_t                  buf_cap_in_bytes,
                                      const faith_msg_pong_t *in);

faith_status_code_t faith_decode_pong(const uint8_t    *payload,
                                      size_t            payload_size,
                                      faith_msg_pong_t *out);

faith_status_code_t faith_encode_envelope(uint8_t *out_buf, size_t *out_size,
                                          size_t buf_cap_in_bytes,
                                          const faith_envelope_t *in);

faith_status_code_t faith_decode_envelope(const uint8_t    *payload,
                                          size_t            payload_size,
                                          faith_envelope_t *out);

faith_status_code_t
faith_encode_device_link_req_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_device_link_req_t *in);

faith_status_code_t
faith_decode_device_link_req_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_device_link_req_t *out);

faith_status_code_t faith_encode_device_link_response_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_cts_device_link_response_t *in);

faith_status_code_t faith_decode_device_link_response_body(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_envl_cts_device_link_response_t *out);

faith_status_code_t faith_encode_client_disconnect_body(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_envl_stc_client_disconnect_t *in);

faith_status_code_t
faith_decode_client_disconnect_body(const uint8_t    *payload,
                                    faith_body_size_t payload_size,
                                    faith_envl_stc_client_disconnect_t *out);

faith_status_code_t faith_encode_hello_body(uint8_t           *out_buf,
                                            faith_body_size_t *out_size,
                                            size_t             buf_cap_in_bytes,
                                            const faith_envl_cts_hello_t *in);

faith_status_code_t faith_decode_hello_body(const uint8_t    *payload,
                                            faith_body_size_t payload_size,
                                            faith_envl_cts_hello_t *out);

faith_status_code_t
faith_encode_hello_challenge_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                  size_t buf_cap_in_bytes,
                                  const faith_envl_stc_hello_challenge_t *in);

faith_status_code_t
faith_decode_hello_challenge_body(const uint8_t    *payload,
                                  faith_body_size_t payload_size,
                                  faith_envl_stc_hello_challenge_t *out);

faith_status_code_t
faith_encode_command_body(uint8_t *out_buf, faith_body_size_t *out_size,
                          size_t                          buf_cap_in_bytes,
                          const faith_envl_cts_command_t *in);

faith_status_code_t faith_decode_command_body(const uint8_t    *payload,
                                              faith_body_size_t payload_size,
                                              faith_envl_cts_command_t *out);

faith_status_code_t
faith_encode_command_result_body(uint8_t *out_buf, faith_body_size_t *out_size,
                                 size_t buf_cap_in_bytes,
                                 const faith_envl_stc_command_result_t *in);

faith_status_code_t
faith_decode_command_result_body(const uint8_t                   *payload,
                                 faith_body_size_t                payload_size,
                                 faith_envl_stc_command_result_t *out);

faith_status_code_t faith_encode_event_body(uint8_t           *out_buf,
                                            faith_body_size_t *out_size,
                                            size_t             buf_cap_in_bytes,
                                            const faith_envl_stc_event_t *in);

faith_status_code_t faith_decode_event_body(const uint8_t    *payload,
                                            faith_body_size_t payload_size,
                                            faith_envl_stc_event_t *out);
