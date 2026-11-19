#include "conversation.h"

#include <faith-proto/codec/signatures.h>

#include "../db/device_repo.h"
#include "../delivery/events.h"
#include "dispatch.h"

#include <third_party/stb_ds.h>

typedef struct {
  bool                    uncertain;
  bool                    accepted;
  uint64_t                client_id;
  faith_auth_id_t         owner;
  faith_auth_id_t         recipient;
  faith_device_id_t       device;
  faith_conversation_id_t conversation;
  faith_command_id_t      command_id;
  faith_command_type_t    type;
  uint8_t                 payload[FAITH_CONVERSATION_RESPONSE_SIZE];
  db_events_job_t        *events;
} conversation_job_t;

static bool _command(faith_db_t *db, const char *sql);
static faith_status_code_t _record_request(faith_db_t *db,
                                          conversation_job_t *job, bool *notify);
static faith_status_code_t _record_response(faith_db_t *db,
                                           conversation_job_t *job, bool *notify);
static faith_status_code_t _run(faith_db_t *db, void *data);
static void _complete(void *owner, void *data, faith_status_code_t status);
static void _destroy(void *data);
static faith_status_code_t _submit(server_state_t *s, client_conn_t *cl,
                                   const faith_envl_cts_command_t *cmd,
                                   const faith_auth_id_t *peer);

static bool _command(faith_db_t *db, const char *sql) {
  PGresult *res = PQexec(db->conn, sql);
  bool      ok = res && PQresultStatus(res) == PGRES_COMMAND_OK;
  PQclear(res);

  return ok;
}

static faith_status_code_t _record_request(faith_db_t *db,
                                          conversation_job_t *job, bool *notify) {
  _FH_CHECK_RETURN(conv_claim(db, job->conversation.bytes, &job->owner));

  const char *values[] = {(const char *)job->conversation.bytes,
                          (const char *)job->recipient.bytes};
  const int lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE};
  const int formats[] = {1, 1};
  PGresult *res = PQexecParams(
      db->conn,
      "INSERT INTO conversation_requests (conversation_id, recipient_id) "
      "VALUES ($1, $2) ON CONFLICT DO NOTHING RETURNING 1",
      2, NULL, values, lengths, formats, 0);
  faith_status_code_t rc = FAITH_ERR_IO;

  if (res && PQresultStatus(res) == PGRES_TUPLES_OK) {
    *notify = PQntuples(res) == 1;
    rc = FAITH_OK;
  } else if (res) {
    const char *state = PQresultErrorField(res, PG_DIAG_SQLSTATE);

    if (state && strcmp(state, "23503") == 0)
      rc = FAITH_ERR_NOT_FOUND;
  }

  PQclear(res);

  return rc;
}

static faith_status_code_t _record_response(faith_db_t *db,
                                           conversation_job_t *job, bool *notify) {
  const char *values[] = {(const char *)job->conversation.bytes,
                          (const char *)job->owner.bytes,
                          (const char *)job->recipient.bytes,
                          (const char *)job->device.bytes,
                          job->accepted ? "true" : "false"};
  const int lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE,
                         FAITH_AUTH_ID_SIZE, FAITH_DEVICE_ID_SIZE, 0};
  const int formats[] = {1, 1, 1, 1, 0};
  PGresult *res = PQexecParams(
      db->conn,
      "UPDATE conversation_requests r SET accepted = $5 FROM conversations c "
      "WHERE c.conversation_id = $1 AND c.owner_id = $2 "
      "AND r.conversation_id = c.conversation_id AND r.recipient_id = $3 "
      "AND r.accepted IS NULL AND EXISTS (SELECT 1 FROM devices "
      "WHERE auth_id = $3 AND device_id = $4 AND revoke_at IS NULL) RETURNING 1",
      5, NULL, values, lengths, formats, 0);
  faith_status_code_t rc = FAITH_ERR_IO;

  if (res && PQresultStatus(res) == PGRES_TUPLES_OK) {
    *notify = PQntuples(res) == 1;
    rc = *notify ? FAITH_OK : FAITH_ERR_UNAUTHORIZED;
  }

  PQclear(res);

  return rc;
}

static faith_status_code_t _run(faith_db_t *db, void *data) {
  conversation_job_t *job = data;

  if (!_command(db, "BEGIN"))
    return FAITH_ERR_IO;

  bool request = job->type == FAITH_COMMAND_CONVERSATION_REQUEST;
  bool notify = false;
  faith_status_code_t rc = request ? _record_request(db, job, &notify)
                                   : _record_response(db, job, &notify);

  if (rc == FAITH_OK && notify) {
    const faith_auth_id_t *target = request ? &job->recipient : &job->owner;
    db_user_device_list_t devices = {0};
    rc = db_device_repo_get_device_list(db, target, &devices);

    if (rc == FAITH_OK && !devices.n_devices)
      rc = FAITH_ERR_NOT_FOUND;

    for (size_t i = 0; i < devices.n_devices && rc == FAITH_OK; i++) {
      db_events_job_t event = {
          .operation = DB_EVENTS_INSERT,
          .in_transaction = true,
          .auth_id = *target,
          .device_id = devices.devices[i].device_id,
          .event = {.type = request ? FAITH_EVENT_CONVERSATION_REQUEST
                                    : FAITH_EVENT_CONVERSATION_RESPONSE,
                    .data = job->payload,
                    .data_size = request ? FAITH_CONVERSATION_REQUEST_SIZE
                                         : FAITH_CONVERSATION_RESPONSE_SIZE},
      };
      rc = db_events_repo_execute(db, &event);

      if (rc == FAITH_OK)
        arrput(job->events, event);
    }

    db_device_repo_destroy_device_list(&devices);
  }

  if (rc == FAITH_OK) {
    if (!_command(db, "COMMIT")) {
      job->uncertain = true;
      rc = FAITH_ERR_IO;
    }

  } else if (!_command(db, "ROLLBACK")) {
    job->uncertain = true;
  }

  return rc;
}

static void _complete(void *owner, void *data, faith_status_code_t status) {
  server_state_t *s = owner;
  conversation_job_t *job = data;

  if (status == FAITH_OK || job->uncertain) {
    for (size_t i = 0; i < arrlenu(job->events); i++)
      delivery_event_complete(s, &job->events[i], status);
  }

  client_conn_t *cl = s->clients;

  while (cl && cl->conn.id != job->client_id)
    cl = cl->next;

  if (!cl || cl->closing || cl->state != CLIENT_OPEN)
    return;

  command_queue_result(
      s, cl, &job->command_id, job->type,
      status == FAITH_OK               ? FAITH_COMMAND_ERR_NONE
      : status == FAITH_ERR_NOT_FOUND  ? FAITH_COMMAND_ERR_USER_NOT_FOUND
      : status == FAITH_ERR_UNAUTHORIZED ? FAITH_COMMAND_ERR_UNAUTHORIZED
                                       : FAITH_COMMAND_ERR_INTERNAL_ERROR,
      status == FAITH_OK ? FAITH_COMMAND_RESULT_ACCEPTED
                         : FAITH_COMMAND_RESULT_REJECTED);
}

static void _destroy(void *data) {
  conversation_job_t *job = data;
  arrfree(job->events);
  free(job);
}

static faith_status_code_t _submit(server_state_t *s, client_conn_t *cl,
                                   const faith_envl_cts_command_t *cmd,
                                   const faith_auth_id_t *peer) {
  conversation_job_t *job = calloc(1, sizeof(*job));

  if (!job)
    return FAITH_ERR_NOMEM;

  bool request = cmd->type == FAITH_COMMAND_CONVERSATION_REQUEST;
  job->client_id = cl->conn.id;
  job->command_id = cmd->cmd_id;
  job->type = cmd->type;
  job->owner = request ? cl->ident.auth_id : *peer;
  job->recipient = request ? *peer : cl->ident.auth_id;
  job->device = cl->ident.device_id;
  job->accepted = !request && cmd->payload[FAITH_CONVERSATION_REQUEST_SIZE];
  memcpy(job->conversation.bytes, cmd->payload, FAITH_CONVERSATION_ID_SIZE);
  memcpy(job->payload, job->conversation.bytes, FAITH_CONVERSATION_ID_SIZE);
  memcpy(job->payload + FAITH_CONVERSATION_ID_SIZE, cl->ident.auth_id.bytes,
         FAITH_AUTH_ID_SIZE);
  job->payload[FAITH_CONVERSATION_REQUEST_SIZE] = job->accepted;

  db_async_job_t task = {
      .run = _run, .complete = _complete, .destroy = _destroy, .data = job};
  faith_status_code_t rc = db_async_submit(&s->db_pool, &task);

  if (rc != FAITH_OK)
    _destroy(job);

  return rc;
}

faith_status_code_t
conv_claim(faith_db_t *db, const uint8_t id[FAITH_CONVERSATION_ID_SIZE],
           const faith_auth_id_t *owner) {
  const char *values[] = {(const char *)id, (const char *)owner->bytes};
  const int lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE};
  const int formats[] = {1, 1};
  PGresult *res = PQexecParams(
      db->conn,
      "INSERT INTO conversations (conversation_id, owner_id) VALUES ($1, $2) "
      "ON CONFLICT DO NOTHING",
      2, NULL, values, lengths, formats, 0);
  bool ok = res && PQresultStatus(res) == PGRES_COMMAND_OK;
  PQclear(res);

  if (!ok)
    return FAITH_ERR_IO;

  res = PQexecParams(db->conn,
                     "SELECT 1 FROM conversations "
                     "WHERE conversation_id = $1 AND owner_id = $2",
                     2, NULL, values, lengths, formats, 0);
  faith_status_code_t rc = FAITH_ERR_IO;

  if (res && PQresultStatus(res) == PGRES_TUPLES_OK)
    rc = PQntuples(res) == 1 ? FAITH_OK : FAITH_ERR_UNAUTHORIZED;

  PQclear(res);

  return rc;
}

faith_status_code_t
conv_handle_conversation_request(server_state_t *s, client_conn_t *cl,
                                 const faith_envl_cts_command_t *cmd) {
  if (cmd->payload_size != FAITH_CONVERSATION_REQUEST_SIZE)
    return FAITH_ERR_INVALID;

  faith_auth_id_t recipient;
  memcpy(recipient.bytes, cmd->payload + FAITH_CONVERSATION_ID_SIZE,
         FAITH_AUTH_ID_SIZE);

  if (faith_client_id_equal(recipient, cl->ident.auth_id))
    return FAITH_ERR_INVALID;

  return _submit(s, cl, cmd, &recipient);
}

faith_status_code_t
conv_handle_conversation_response(server_state_t *s, client_conn_t *cl,
                                  const faith_envl_cts_command_t *cmd) {
  if (cmd->payload_size != FAITH_CONVERSATION_SIGNED_RESPONSE_SIZE ||
      cmd->payload[FAITH_CONVERSATION_REQUEST_SIZE] > 1)
    return FAITH_ERR_INVALID;

  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(sess_registry_get_session(
      &s->rt, &cl->ident.auth_id, &cl->ident.device_id, &sess));

  if (!sess || sess->conn != cl)
    return FAITH_ERR_UNAUTHORIZED;

  uint8_t msg[FAITH_SIGNATURE_CONVERSATION_RESPONSE_SIZE];
  faith_gen_sign_buf_conversation_response(msg, cmd->payload,
                                          &cl->ident.auth_id, &cl->ident.device_id);

  if (faith_verify_signature_raw_pubkey(
          sess->ident.public_key, msg, sizeof(msg),
          cmd->payload + FAITH_CONVERSATION_RESPONSE_SIZE,
          FAITH_ED25519_SIGNATURE_SIZE) != FAITH_OK)
    return FAITH_ERR_UNAUTHORIZED;

  faith_auth_id_t owner;
  memcpy(owner.bytes, cmd->payload + FAITH_CONVERSATION_ID_SIZE,
         FAITH_AUTH_ID_SIZE);

  return _submit(s, cl, cmd, &owner);
}
