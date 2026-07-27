#include "events.h"

#include "../codec/protocol.h"
#include "../server/client_io.h"
#include "event_inbox.h"

faith_status_code_t delivery_queue_event(server_state_t *s, client_conn_t *cl,
                                       faith_event_codec_type_t type,
                                       uint8_t                 *data,
                                       faith_body_size_t        data_size) {
  if (!cl || (!data && data_size != 0) || (data && data_size == 0))
    return FAITH_ERR_INVALID;

  if(!cl->authorized) return FAITH_ERR_UNAUTHORIZED;

  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(
      sess_registry_get_session(&s->rt, &cl->auth_id, &cl->device_id, &sess));

  if(!sess) return FAITH_ERR_UNAUTHORIZED;


  faith_envl_stc_event_t event = {0};
  event.type = type;
  event.data = data;
  event.data_size = data_size;
  event.seq_num = sess->inbox.next_seq;

  faith_status_code_t _fh_result = FAITH_OK;

  size_t            cap = FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + data_size;
  uint8_t          *body = malloc(cap);
  faith_body_size_t body_size = 0;
  _FH_CHECK_DEFER(faith_encode_event_body(body, &body_size, cap, &event));

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_EVENT;
  envl.recipient_id = cl->auth_id;
  envl.body = body;
  envl.body_size = body_size;

  _FH_CHECK_DEFER(server_queue_envelope_or_mark_dead(s, cl, &envl));

  _FH_CHECK_DEFER(device_event_inbox_advance_seq(&sess->inbox));

defer:
  free(body);
  return _fh_result;
}

faith_status_code_t delivery_handle_event_acked(server_state_t   *s,
                                                client_conn_t    *cl,
                                                faith_envelope_t *envl) {
  if(!s || !cl || !envl) return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_EVENT_ACK)
    return FAITH_ERR_BAD_ENVELOPE;

  if (!cl->authorized)
    return FAITH_ERR_UNAUTHORIZED;

  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(
      sess_registry_get_session(&s->rt, &cl->auth_id, &cl->device_id, &sess));

  if(!sess) return FAITH_ERR_UNAUTHORIZED;

  faith_envl_cts_event_ack_t ack = {0};
  _FH_CHECK_RETURN(
      faith_decode_event_ack_body(envl->body, envl->body_size, &ack));

  device_event_inbox_ack_seq(&sess->inbox, ack.seq_num);

  return FAITH_OK;
}
