#pragma once

#include "../application/conversation.h"
#include "../auth/structs.h"
#include "core.h"
#include "helpers.h"

#define FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED                                   \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* sequence number */ +                    \
                   sizeof(uint32_t) /* type */ +                               \
                   sizeof(faith_body_size_t) /* data size */                   \
  )

#define FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED                             \
  _FAITH_BODY_SIZE(sizeof(uint16_t) /* number of events*/ +                    \
                   sizeof(uint32_t) /* events data size */)

#define FAITH_ENVL_CTS_EVENT_ACK_BODY_SIZE                                     \
  _FAITH_BODY_SIZE(sizeof(uint64_t) /* sequence number */)

#define FAITH_EVENTS_CODEC(X) X(FAITH_EVENT_CONVERSATION_CREATED, 0)

#define FAITH_EVENT_BATCH_MAX_EVENTS 256u

typedef enum {
#define X(name, value) name = value,
  FAITH_EVENTS_CODEC(X)
#undef X
} faith_event_codec_type_t;

typedef struct {
  /* The server-generated sequence number
   * of this event */
  uint64_t seq_num;

  faith_event_codec_type_t type;

  faith_body_size_t data_size;
  uint8_t          *data;
} faith_envl_stc_event_t;

typedef struct {
  uint16_t n_events;

  faith_body_size_t       events_data_size;
  faith_envl_stc_event_t *events;
} faith_envl_stc_event_batch_t;

typedef struct {
  /* The server-generated sequence number
   * of the event that was acknowledged */
  uint64_t seq_num;
} faith_envl_cts_event_ack_t;

typedef struct {
  faith_conversation_id_t conversation_id;
} faith_event_conversation_created_t;

#define FAITH_EVENT_CONVERSATION_CREATED_DATA_SIZE                             \
  _FAITH_BODY_SIZE(FAITH_CONVERSATION_ID_SIZE)

faith_status_code_t faith_encode_event_conversation_created(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_event_conversation_created_t *in);

faith_status_code_t faith_decode_event_conversation_created(
    const uint8_t *payload, faith_body_size_t payload_size,
    faith_event_conversation_created_t *out);

const char *faith_event_codec_type_name(faith_event_codec_type_t type);
