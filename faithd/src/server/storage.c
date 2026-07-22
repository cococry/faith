#include "storage.h"

#include "../../third_party/stb_ds.h"

#include "../protocol.h"

faith_status_code_t
storage_store_msg_request(storage_state_t             *st,
                          const faith_request_id_t    *request_id,
                          const storage_msg_request_t *request) {
  if (!st || !request_id || !request)
    return FAITH_ERR_INVALID;

  /* This also handles the extremely unlikely case of a randomly generated
   * server request ID colliding. */
  if (hmgetp_null(st->msg_requests, *request_id))
    return FAITH_ERR_ALREADY_EXISTS;

  storage_msg_request_t *stored = malloc(sizeof(*stored));
  if (!stored)
    return FAITH_ERR_NOMEM;

  *stored = *request;

  hmput(st->msg_requests, *request_id, stored);

  return FAITH_OK;
}

faith_status_code_t
storage_remove_msg_request(storage_state_t          *st,
                           const faith_request_id_t *request_id) {
  if (!st || !request_id)
    return FAITH_ERR_INVALID;

  stored_msg_request_t *entry = hmgetp_null(st->msg_requests, *request_id);

  if (!entry)
    return FAITH_ERR_NOT_FOUND;

  free(entry->value);
  entry->value = NULL;

  hmdel(st->msg_requests, *request_id);

  return FAITH_OK;
}

faith_status_code_t
storage_get_msg_request(storage_state_t          *st,
                        const faith_request_id_t *request_id,
                        storage_msg_request_t   **out) {
  if (!st || !request_id || !out)
    return FAITH_ERR_INVALID;

  *out = NULL;

  ptrdiff_t index = hmgeti(st->msg_requests, *request_id);

  if (index < 0)
    return FAITH_ERR_NOT_FOUND;

  /* The request was found but not correctly allocated */
  if (!st->msg_requests[index].value)
    return FAITH_ERR_INVALID;

  *out = st->msg_requests[index].value;

  return FAITH_OK;
}
