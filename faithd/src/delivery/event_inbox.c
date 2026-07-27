#include "event_inbox.h"

#include "../../third_party/stb_ds.h"
#include <stdint.h>

void device_event_inbox_init(device_event_inbox_t *o_inbox) {
  if (!o_inbox)
    return;

  o_inbox->events = NULL;
  o_inbox->last_acked_seq = UINT64_MAX;
  o_inbox->next_seq = 0;
}

faith_status_code_t
device_event_inbox_queue_event(device_event_inbox_t         *inbox,
                               const faith_envl_stc_event_t *event) {
  if (!inbox || !event)
    return FAITH_ERR_INVALID;
  arrput(inbox->events, *event);

  return FAITH_OK;
}

faith_status_code_t
device_event_inbox_advance_seq(device_event_inbox_t *inbox) {
  if (!inbox)
    return FAITH_ERR_INVALID;

  /* Dont allow UINT64_MAX */
  if (inbox->next_seq >= UINT64_MAX - 1)
    return FAITH_ERR_OVERFLOW;

  inbox->last_sent_seq = inbox->next_seq;
  inbox->next_seq++;

  return FAITH_OK;
}

faith_status_code_t device_event_inbox_ack_seq(device_event_inbox_t *inbox,
                                               uint64_t acked_seq) {
  if (!inbox)
    return FAITH_ERR_INVALID;

  /* Duplicate or stale ACK */
  if (acked_seq <= inbox->last_acked_seq)
    return FAITH_OK;

  if (acked_seq >= inbox->next_seq)
    return FAITH_ERR_INVALID;

  inbox->last_acked_seq = acked_seq;

  return FAITH_OK;
}

faith_status_code_t device_event_inbox_destroy(device_event_inbox_t *inbox) {
  if (!inbox)
    return FAITH_ERR_INVALID;

  arrfree(inbox->events);

  return FAITH_OK;
}
