#include "events.h"

#include "../codec/protocol.h"
#include "../server/client_io.h"
#include "event_inbox.h"

#include "../../third_party/stb_ds.h"

#define DIV_UP(x, y) (((x) + (y) - 1) / (y))

#define _MODULE_NAME "delivery/events"

static faith_status_code_t
queue_event_online_user(server_state_t *s, struct client_conn_t *cl,
                        device_event_inbox_t         *inbox,
                        const faith_envl_stc_event_t *event) {
  if (!s || !cl || !inbox || !event)
    return FAITH_ERR_INVALID;

  faith_status_code_t _fh_result = FAITH_OK;

  size_t   cap = FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + event->data_size;
  uint8_t *body = malloc(cap);
  faith_body_size_t body_size = 0;
  _FH_CHECK_DEFER(faith_encode_event_body(body, &body_size, cap, event));

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_EVENT;
  envl.recipient_id = cl->ident.auth_id;
  envl.body = body;
  envl.body_size = body_size;

  _FH_CHECK_DEFER(server_queue_envelope_or_mark_dead(s, cl, &envl));

  _FH_CHECK_DEFER(device_event_inbox_advance_seq(inbox));

defer:
  free(body);
  return _fh_result;
}

static faith_status_code_t
queue_event_offline_user(device_event_inbox_t         *inbox,
                         const faith_envl_stc_event_t *event) {
  if (!inbox || !event)
    return FAITH_ERR_INVALID;

  _FH_CHECK_RETURN(device_event_inbox_push_event(inbox, event));
  return FAITH_OK;
}

static faith_status_code_t
queue_event_batch_envl(server_state_t *s, struct client_conn_t *cl,
                       const faith_envl_stc_event_batch_t *batch_envl) {
  if (!batch_envl)
    return FAITH_ERR_INVALID;

  size_t batch_data_cap =
      FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED + batch_envl->events_data_size;
  uint8_t *body = malloc(batch_data_cap);
  if (!body) {
    return FAITH_ERR_NOMEM;
  }

  faith_status_code_t _fh_result = FAITH_OK;
  faith_body_size_t   body_size = 0;
  _FH_CHECK_DEFER(faith_encode_event_batch_body(body, &body_size,
                                                batch_data_cap, batch_envl));

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_EVENT_BATCH;
  envl.recipient_id = cl->ident.auth_id;
  envl.body = body;
  envl.body_size = body_size;

  _FH_CHECK_DEFER(server_queue_envelope_or_mark_dead(s, cl, &envl));

defer:
  free(body);
  return _fh_result;
}

faith_status_code_t delivery_queue_event(server_state_t          *s,
                                         const faith_auth_id_t   *auth_id,
                                         const faith_device_id_t *device_id,
                                         const faith_event_codec_type_t type,
                                         uint8_t                       *data,
                                         faith_body_size_t data_size) {
  if (!auth_id || !device_id || (!data && data_size != 0) ||
      (data && data_size == 0))
    return FAITH_ERR_INVALID;

  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(
      sess_registry_get_session(&s->rt, auth_id, device_id, &sess));

  if (!sess)
    return FAITH_ERR_NOT_FOUND;

  bool online = sess->conn != NULL;

  if (online && !sess->conn->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  faith_envl_stc_event_t event = {0};
  event.type = type;
  event.data = data;
  event.data_size = data_size;
  event.seq_num = sess->inbox.next_seq;

  if (online) {
    _FH_CHECK_RETURN(
        queue_event_online_user(s, sess->conn, &sess->inbox, &event));
    return FAITH_OK;
  }

  _FH_CHECK_RETURN(queue_event_offline_user(&sess->inbox, &event));
  return FAITH_OK;
}

faith_status_code_t delivery_queue_pending_events(server_state_t *s,
                                                  client_conn_t  *cl) {
  if (!s || !cl) {
    return FAITH_ERR_INVALID;
  }
  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(sess_registry_get_session(&s->rt, &cl->ident.auth_id,
                                             &cl->ident.device_id, &sess));

  if (!sess)
    return FAITH_ERR_NOT_FOUND;

  size_t n_events = arrlen(sess->inbox.events);

  size_t n_fitting = 0;
  for (size_t offset = 0; offset < n_events; offset += n_fitting) {
    size_t remaining = n_events - offset;

    faith_envl_stc_event_t *batch = &sess->inbox.events[offset];

    size_t n_in_batch = remaining < FAITH_EVENT_BATCH_MAX_EVENTS
                            ? remaining
                            : FAITH_EVENT_BATCH_MAX_EVENTS;

    const size_t batch_overhead =
        FAITH_ENVL_STC_EVENT_BATCH_BODY_SIZE_FIXED + FAITH_ENVL_HEADER_SIZE;

    if (FAITH_MAX_STC_PAYLOAD_SIZE < batch_overhead) {
      return FAITH_ERR_TOO_LARGE;
    }

    const size_t max_events_data_size =
        FAITH_MAX_STC_PAYLOAD_SIZE - batch_overhead;

    faith_body_size_t events_data_size = 0;
    n_fitting = 0;

    _FH_CHECK_RETURN(faith_codec_event_batch_data_size_fit(
        batch, n_in_batch, &events_data_size, &n_fitting,
        max_events_data_size));

    if (n_fitting == 0 || n_fitting > n_in_batch) {
      nob_log(ERROR,
              "[%s] Event data is too large or codec returned invalid count.",
              _MODULE_NAME);
      return FAITH_ERR_TOO_LARGE;
    }

    faith_envl_stc_event_batch_t batch_envl = {0};
    batch_envl.events_data_size = events_data_size;
    batch_envl.events = batch;
    batch_envl.n_events = (uint16_t)n_fitting;

    _FH_CHECK_RETURN(queue_event_batch_envl(s, cl, &batch_envl));
  }

  return FAITH_OK;
}

faith_status_code_t delivery_handle_event_acked(server_state_t   *s,
                                                client_conn_t    *cl,
                                                faith_envelope_t *envl) {
  if (!s || !cl || !envl)
    return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_EVENT_ACK)
    return FAITH_ERR_BAD_ENVELOPE;

  if (!cl->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(sess_registry_get_session(&s->rt, &cl->ident.auth_id,
                                             &cl->ident.device_id, &sess));

  if (!sess)
    return FAITH_ERR_NOT_FOUND;

  faith_envl_cts_event_ack_t ack = {0};
  _FH_CHECK_RETURN(
      faith_decode_event_ack_body(envl->body, envl->body_size, &ack));

  _FH_CHECK_RETURN(device_event_inbox_remove_until(&sess->inbox, ack.seq_num));

  return FAITH_OK;
}
