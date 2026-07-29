#pragma once

#include <faith-proto/codec/events.h>

#include <stdint.h>

typedef struct {
  uint64_t next_seq;
  uint64_t last_acked_seq;
  uint64_t last_sent_seq;

  faith_envl_stc_event_t *events;
} device_event_inbox_t;

void device_event_inbox_init(device_event_inbox_t *o_inbox);

faith_status_code_t
device_event_inbox_push_event(device_event_inbox_t         *inbox,
                              const faith_envl_stc_event_t *event);

faith_status_code_t device_event_inbox_advance_seq(device_event_inbox_t *inbox);

faith_status_code_t device_event_inbox_remove_until(device_event_inbox_t *inbox,
                                                    uint64_t until_seq);

faith_status_code_t device_event_inbox_destroy(device_event_inbox_t *inbox);
