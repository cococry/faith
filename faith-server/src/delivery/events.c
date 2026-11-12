#include "events.h"

#include "../auth/handshake.h"
#include "../db/events_repo.h"
#include "../server/client_io.h"
#include "../server/client_lifecycle.h"
#include "event_inbox.h"
#include <faith-proto/codec/codec.h>

#include <third_party/stb_ds.h>

#define DIV_UP(x, y) (((x) + (y) - 1) / (y))

#define _MODULE_NAME "delivery/events"

static faith_status_code_t
_queue_event_online_user(server_state_t *s, client_conn_t *cl,
                         const faith_envl_stc_event_t *event);

static faith_status_code_t
_queue_event_batch_envl(server_state_t *s, client_conn_t *cl,
                        const faith_envl_stc_event_batch_t *batch_envl);

static faith_status_code_t _open_stream(server_state_t *s, client_conn_t *cl,
                                        client_device_session_data_t *sess);

static void                _event_complete(void *owner, void *data,
                                           faith_status_code_t status);
static faith_status_code_t _submit_event(server_state_t  *s,
                                         db_events_job_t *job);

static faith_status_code_t
_queue_event_online_user(server_state_t *s, struct client_conn_t *cl,
                         const faith_envl_stc_event_t *event) {
  if (!s || !cl || !event)
    return FAITH_ERR_INVALID;

  faith_status_code_t _fh_result = FAITH_OK;

  size_t   cap = FAITH_ENVL_STC_EVENT_BODY_SIZE_FIXED + event->data_size;
  uint8_t *body = malloc(cap);
  faith_body_size_t body_size = 0;

  if (!body)
    return FAITH_ERR_NOMEM;

  _FH_CHECK_DEFER(faith_encode_event_body(body, &body_size, cap, event));

  faith_envelope_t envl = {0};
  envl.type = FAITH_ENVELOPE_EVENT;
  envl.recipient_id = cl->ident.auth_id;
  envl.body = body;
  envl.body_size = body_size;

  _FH_CHECK_DEFER(server_queue_envelope_or_mark_dead(s, cl, &envl));

defer:
  free(body);

  return _fh_result;
}

static faith_status_code_t
_queue_event_batch_envl(server_state_t *s, struct client_conn_t *cl,
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

static faith_status_code_t _open_stream(server_state_t *s, client_conn_t *cl,
                                        client_device_session_data_t *sess) {
  if (auth_handshake_ready(s, cl, &cl->ident.auth_id, &cl->ident.device_id) !=
      FAITH_OK) {
    return FAITH_ERR_IO;
  }

  uint8_t                    resume[8];
  faith_body_size_t          size = 0;
  faith_envl_cts_event_ack_t base = {
      .seq_num = sess->inbox.last_acked_seq == UINT64_MAX
                     ? 0
                     : sess->inbox.last_acked_seq + 1};

  faith_encode_event_ack_body(resume, &size, sizeof(resume), &base);
  cl->event_send_next = base.seq_num;
  cl->event_last_sent = UINT64_MAX;
  faith_envelope_t envl = {.type = FAITH_ENVELOPE_EVENT_RESUME,
                           .recipient_id = cl->ident.auth_id,
                           .body = resume,
                           .body_size = size};

  if (server_queue_envelope_or_mark_dead(s, cl, &envl) != FAITH_OK ||
      delivery_queue_pending_events(s, cl) != FAITH_OK)
    return FAITH_ERR_IO;

  return FAITH_OK;
}

static void _event_complete(void *owner, void *data,
                            faith_status_code_t status) {
  server_state_t  *s = owner;
  db_events_job_t *job = data;

  if (job->operation == DB_EVENTS_EXPIRE) {
    s->event_cleanup_pending = false;

    if (status != FAITH_OK)
      return;

    for (size_t i = 0; i < arrlenu(job->expired); i++) {
      db_expired_inbox_t           *entry = &job->expired[i];
      client_device_session_data_t *expired = NULL;
      sess_registry_get_session(&s->rt, &entry->auth_id, &entry->device_id,
                                &expired);

      if (!expired || !expired->events_loaded)
        continue;

      size_t before = expired->inbox.bytes;
      device_event_inbox_remove_until(&expired->inbox, entry->until);
      s->event_cached_bytes -= before - expired->inbox.bytes;

      if (expired->conn)
        server_client_queue_disconnect(
            s, expired->conn, FAITH_DISCONNECT_TEMPORARY_FAILURE,
            FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
            "Event retention window expired; reconnect to resume.");
    }

    return;
  }

  client_device_session_data_t *sess = NULL;
  sess_registry_get_session(&s->rt, &job->auth_id, &job->device_id, &sess);

  if (!sess)
    return;

  if (job->operation == DB_EVENTS_SYNC) {
    sess->sync_pending = false;

    if (status != FAITH_OK || !sess->events_loaded)
      return;

    for (size_t i = 0; i < arrlenu(job->inbox.events); i++) {
      faith_envl_stc_event_t *event = &job->inbox.events[i];

      if (event->seq_num < sess->inbox.next_seq)
        continue;

      if (event->seq_num > sess->inbox.next_seq) {
        sess->events_loaded = false;

        if (sess->conn)
          server_client_queue_disconnect(
              s, sess->conn, FAITH_DISCONNECT_TEMPORARY_FAILURE,
              FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
              "Event stream advanced; reconnect to recover.");

        return;
      }

      db_events_job_t insert = {.operation = DB_EVENTS_INSERT,
                                .auth_id = job->auth_id,
                                .device_id = job->device_id,
                                .event = *event};

      _event_complete(s, &insert, FAITH_OK);
    }

    return;
  }

  client_conn_t *cl = sess->conn;

  if (status != FAITH_OK) {
    sess->events_loaded = false;

    if (cl)
      server_client_queue_disconnect(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                                     FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                                     "Event database operation failed.");

    nob_log(ERROR, "[delivery/events] Database event operation failed: %s",
            faith_status_code_name(status));

    return;
  }

  if (job->operation == DB_EVENTS_LOAD) {
    if (!cl || cl->conn.id != job->client_id || cl->closing ||
        cl->close_after_flush)
      return;

    if (job->inbox.bytes >
        67108864u - s->event_cached_bytes + sess->inbox.bytes) {
      server_client_queue_disconnect(s, cl, FAITH_DISCONNECT_TEMPORARY_FAILURE,
                                     FAITH_CLIENT_RECONNECT_ALLOWED, 1000, 0,
                                     "Event cache is busy; retry later.");

      return;
    }

    s->event_cached_bytes -= sess->inbox.bytes;
    device_event_inbox_destroy(&sess->inbox);
    sess->inbox = job->inbox;
    s->event_cached_bytes += sess->inbox.bytes;
    job->inbox.events = NULL;

    sess->events_loaded = true;

    if (_open_stream(s, cl, sess) != FAITH_OK)
      cl->closing = 1;

  } else if (job->operation == DB_EVENTS_INSERT) {
    if (!sess->events_loaded || job->event.seq_num < sess->inbox.next_seq)
      return;

    if (job->event.seq_num > sess->inbox.next_seq) {
      if (!sess->sync_pending) {
        db_events_job_t *sync = calloc(1, sizeof(*sync));

        if (sync) {
          sync->operation = DB_EVENTS_SYNC;
          sync->auth_id = job->auth_id;
          sync->device_id = job->device_id;
          sync->event.seq_num = sess->inbox.next_seq;

          if (_submit_event(s, sync) == FAITH_OK)
            sess->sync_pending = true;
        }
      }

      return;
    }

    if (job->event.data_size + 16u > 67108864u - s->event_cached_bytes ||
        device_event_inbox_push_event(&sess->inbox, &job->event) != FAITH_OK) {
      sess->events_loaded = false;

      if (cl)
        server_client_queue_disconnect(s, cl, FAITH_DISCONNECT_INTERNAL_ERROR,
                                       FAITH_CLIENT_RECONNECT_ALLOWED, 0, 0,
                                       "Event stream needs recovery.");

      return;
    }

    s->event_cached_bytes += job->event.data_size + 16u;

    if (cl && cl->state == CLIENT_OPEN && !cl->closing &&
        !cl->close_after_flush) {
      if (cl->event_send_next == job->event.seq_num &&
          cl->conn.out.size < 131072) {
        if (_queue_event_online_user(s, cl, &job->event) == FAITH_OK) {
          cl->event_send_next = job->event.seq_num + 1;
          cl->event_last_sent = job->event.seq_num;
        }

      } else {
        delivery_queue_pending_events(s, cl);
      }
    }

  } else {
    size_t before = sess->inbox.bytes;
    device_event_inbox_remove_until(&sess->inbox, job->event.seq_num);
    s->event_cached_bytes -= before - sess->inbox.bytes;
  }
}

static faith_status_code_t _submit_event(server_state_t  *s,
                                         db_events_job_t *job) {
  db_async_job_t task = {.run = db_events_repo_execute,
                         .complete = _event_complete,
                         .destroy = db_events_repo_destroy,
                         .data = job};

  faith_status_code_t rc = db_async_submit(&s->db_pool, &task);

  if (rc != FAITH_OK)
    db_events_repo_destroy(job);

  return rc;
}

faith_status_code_t delivery_queue_event(server_state_t          *s,
                                         const faith_auth_id_t   *auth_id,
                                         const faith_device_id_t *device_id,
                                         const faith_event_codec_type_t type,
                                         uint8_t                       *data,
                                         faith_body_size_t data_size) {
  if (!s || !auth_id || !device_id || (!data && data_size != 0) ||
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

  db_events_job_t *job = calloc(1, sizeof(*job));

  if (!job)
    return FAITH_ERR_NOMEM;

  job->operation = DB_EVENTS_INSERT;
  job->auth_id = *auth_id;
  job->device_id = *device_id;
  job->event.type = type;
  job->event.data_size = data_size;

  if (data_size) {
    job->event.data = malloc(data_size);

    if (!job->event.data) {
      free(job);

      return FAITH_ERR_NOMEM;
    }

    memcpy(job->event.data, data, data_size);
  }

  return _submit_event(s, job);
}

faith_status_code_t delivery_load_events(server_state_t *s, client_conn_t *cl) {
  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(sess_registry_get_session(&s->rt, &cl->ident.auth_id,
                                             &cl->ident.device_id, &sess));

  if (!sess)
    return FAITH_ERR_NOT_FOUND;

  if (sess->events_loaded)
    return _open_stream(s, cl, sess);

  db_events_job_t *job = calloc(1, sizeof(*job));

  if (!job)
    return FAITH_ERR_NOMEM;

  job->operation = DB_EVENTS_LOAD;
  job->auth_id = cl->ident.auth_id;
  job->device_id = cl->ident.device_id;
  job->client_id = cl->conn.id;
  _FH_CHECK_RETURN(_submit_event(s, job));
  server_set_client_state(s, cl, CLIENT_WAIT_FOR_EVENTS);

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

  if (cl->closing || cl->close_after_flush || cl->state != CLIENT_OPEN ||
      cl->conn.out.size >= 131072)
    return FAITH_OK;

  size_t count = arrlenu(sess->inbox.events);
  size_t low = 0, high = count;

  while (low < high) {
    size_t middle = low + (high - low) / 2;

    if (sess->inbox.events[middle].seq_num < cl->event_send_next)
      low = middle + 1;
    else
      high = middle;
  }

  if (low == count)
    return FAITH_OK;

  faith_envl_stc_event_t *events = &sess->inbox.events[low];

  if (events[0].seq_num != cl->event_send_next)
    return FAITH_ERR_RESYNC_REQUIRED;

  size_t            remaining = count - low;
  uint16_t          n = remaining < 64 ? (uint16_t)remaining : 64;
  faith_body_size_t data_size = 0;
  size_t            fitting = 0;
  _FH_CHECK_RETURN(faith_codec_event_batch_data_size_fit(
      events, n, &data_size, &fitting, 131072 - 54));

  if (!fitting)
    return FAITH_ERR_TOO_LARGE;

  faith_envl_stc_event_batch_t batch = {
      .events = events, .n_events = fitting, .events_data_size = data_size};

  _FH_CHECK_RETURN(_queue_event_batch_envl(s, cl, &batch));
  cl->event_last_sent = events[fitting - 1].seq_num;
  cl->event_send_next = cl->event_last_sent + 1;

  return FAITH_OK;
}

faith_status_code_t delivery_handle_event_acked(server_state_t   *s,
                                                client_conn_t    *cl,
                                                faith_envelope_t *envl) {
  if (!s || !cl || !envl)
    return FAITH_ERR_INVALID;

  if (envl->type != FAITH_ENVELOPE_EVENT_ACK)
    return FAITH_ERR_BAD_ENVELOPE;

  if (!cl->authorized || cl->state != CLIENT_OPEN)
    return FAITH_ERR_UNAUTHORIZED;

  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(sess_registry_get_session(&s->rt, &cl->ident.auth_id,
                                             &cl->ident.device_id, &sess));

  if (!sess)
    return FAITH_ERR_NOT_FOUND;

  faith_envl_cts_event_ack_t ack = {0};
  _FH_CHECK_RETURN(
      faith_decode_event_ack_body(envl->body, envl->body_size, &ack));

  if (sess->inbox.last_acked_seq != UINT64_MAX &&
      ack.seq_num <= sess->inbox.last_acked_seq)
    return FAITH_OK;

  if (cl->event_last_sent == UINT64_MAX || ack.seq_num > cl->event_last_sent)
    return FAITH_ERR_BAD_ENVELOPE;

  db_events_job_t *job = calloc(1, sizeof(*job));

  if (!job)
    return FAITH_ERR_NOMEM;

  job->operation = DB_EVENTS_ACK;
  job->auth_id = cl->ident.auth_id;
  job->device_id = cl->ident.device_id;
  job->event.seq_num = ack.seq_num;

  return _submit_event(s, job);
}

void delivery_event_complete(server_state_t *s, db_events_job_t *job,
                             faith_status_code_t status) {
  _event_complete(s, job, status);
}

void delivery_expire_events(server_state_t *s) {
  if (s->event_cleanup_pending)
    return;

  db_events_job_t *job = calloc(1, sizeof(*job));

  if (!job)
    return;

  job->operation = DB_EVENTS_EXPIRE;

  if (_submit_event(s, job) == FAITH_OK)
    s->event_cleanup_pending = true;
}

void delivery_sync_events(server_state_t *s) {
  size_t         submitted = 0;
  client_conn_t *start = s->clients;

  while (start && start->conn.id != s->event_sync_cursor)
    start = start->next;
  client_conn_t *cl = start && start->next ? start->next : s->clients;

  while (cl && submitted < 32) {
    s->event_sync_cursor = cl->conn.id;

    if (cl->state == CLIENT_OPEN && !cl->closing && !cl->close_after_flush) {
      client_device_session_data_t *sess = NULL;
      sess_registry_get_session(&s->rt, &cl->ident.auth_id,
                                &cl->ident.device_id, &sess);

      if (sess && sess->events_loaded && !sess->sync_pending) {
        db_events_job_t *job = calloc(1, sizeof(*job));

        if (!job)
          return;
        job->operation = DB_EVENTS_SYNC;
        job->auth_id = cl->ident.auth_id;
        job->device_id = cl->ident.device_id;
        job->event.seq_num = sess->inbox.next_seq;

        if (_submit_event(s, job) != FAITH_OK)
          return;
        sess->sync_pending = true;
        submitted++;
      }
    }

    cl = cl->next;
  }
}
