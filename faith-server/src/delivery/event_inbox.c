#include "event_inbox.h"

#include <third_party/stb_ds.h>
#include <stdint.h>

#define _MODULE_NAME "delivery/event_inbox"

void device_event_inbox_init(device_event_inbox_t *o_inbox) {
  if (!o_inbox)
    return;

  o_inbox->events = NULL;
  o_inbox->last_acked_seq = UINT64_MAX;
  o_inbox->last_sent_seq = UINT64_MAX;
  o_inbox->next_seq = 0;
}

faith_status_code_t
device_event_inbox_push_event(device_event_inbox_t         *inbox,
                              const faith_envl_stc_event_t *event) {
  if (!inbox || !event)
    return FAITH_ERR_INVALID;

  /* Dont allow UINT64_MAX */
  if (event->seq_num == UINT64_MAX)
    return FAITH_ERR_OVERFLOW;

  if (inbox->next_seq == UINT64_MAX)
    return FAITH_ERR_OVERFLOW;

  if (inbox->last_sent_seq != UINT64_MAX && event->seq_num != inbox->next_seq) {
    nob_log(ERROR, "[%s] Tried to push event with invalid sequence order.\n",
        _MODULE_NAME);
    return FAITH_ERR_INVALID;
  }

  faith_envl_stc_event_t entry = *event;
  uint8_t               *data_copy = NULL;

  if (event->data_size > 0) {
    data_copy = malloc(event->data_size);
    if (!data_copy)
      return FAITH_ERR_NOMEM;

    memcpy(data_copy, event->data, event->data_size);
  }

  entry.data = data_copy;

  arrput(inbox->events, entry);

  inbox->last_sent_seq = inbox->next_seq;
  inbox->next_seq++;

  return FAITH_OK;
}

faith_status_code_t
device_event_inbox_advance_seq(device_event_inbox_t *inbox) {
  if (!inbox)
    return FAITH_ERR_INVALID;

  /* Dont allow UINT64_MAX */
  if (inbox->next_seq == UINT64_MAX)
    return FAITH_ERR_OVERFLOW;

  inbox->last_sent_seq = inbox->next_seq;
  inbox->next_seq++;

  return FAITH_OK;
}

faith_status_code_t device_event_inbox_remove_until(device_event_inbox_t *inbox,
                                                    uint64_t until_seq) {
  if (!inbox)
    return FAITH_ERR_INVALID;

  if (inbox->last_sent_seq == UINT64_MAX)
    return FAITH_ERR_BAD_ENVELOPE;

  if (until_seq > inbox->last_sent_seq)
    return FAITH_ERR_BAD_ENVELOPE;

  if (inbox->last_acked_seq != UINT64_MAX &&
      until_seq <= inbox->last_acked_seq) {
    return FAITH_OK;
  }

  size_t event_count = arrlen(inbox->events);

  size_t i = 0;
  for (; i < event_count; i++) {
    if (inbox->events[i].seq_num > until_seq)
      break;

    free(inbox->events[i].data);
    inbox->events[i].data = NULL;
  }

  if (i > 0)
    arrdeln(inbox->events, 0, i);

  inbox->last_acked_seq = until_seq;

  return FAITH_OK;
}

faith_status_code_t device_event_inbox_destroy(device_event_inbox_t *inbox) {
  if (!inbox)
    return FAITH_ERR_INVALID;

  arrfree(inbox->events);

  return FAITH_OK;
}
