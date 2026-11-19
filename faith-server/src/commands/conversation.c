#include "conversation.h"

#include <faith-proto/codec/group.h>
#include <faith-proto/codec/signatures.h>

#include "../db/device_repo.h"
#include "../delivery/events.h"
#include "dispatch.h"

#include <third_party/stb_ds.h>

typedef struct {
  bool                    uncertain;
  bool                    accepted;
  bool                    force;
  bool                    disclose;
  uint64_t                client_id;
  faith_auth_id_t         owner;
  faith_auth_id_t         recipient;
  faith_device_id_t       device;
  faith_conversation_id_t conversation;
  faith_command_id_t      command_id;
  faith_command_type_t    type;
  uint8_t                 payload[FAITH_CONVERSATION_REQUEST_EVENT_MAX];
  char                    username[FAITH_USERNAME_MAX + 1];
  db_events_job_t        *events;
} conversation_job_t;

static bool _command(faith_db_t *db, const char *sql);
static faith_status_code_t _record_request(faith_db_t *db,
                                          conversation_job_t *job, bool *notify);
static faith_status_code_t _record_response(faith_db_t *db,
                                           conversation_job_t *job, bool *notify);
static faith_status_code_t _queue_event(faith_db_t *db, conversation_job_t *job,
                                        const faith_auth_id_t *target,
                                        faith_event_codec_type_t type,
                                        const uint8_t *payload, size_t size);
static faith_status_code_t _disclose(faith_db_t *db, conversation_job_t *job);
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
  _FH_CHECK_RETURN(conv_claim(db, job->conversation.bytes, &job->owner, &job->force));

  const char *values[] = {(const char *)job->conversation.bytes,
                          (const char *)job->recipient.bytes};
  const int lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE};
  const int formats[] = {1, 1};
  PGresult *res = PQexecParams(
      db->conn,
      "INSERT INTO conversation_requests (conversation_id, recipient_id) "
      "VALUES ($1, $2) ON CONFLICT DO NOTHING RETURNING "
      "(SELECT disclosed_username FROM conversations WHERE conversation_id = $1)",
      2, NULL, values, lengths, formats, 0);
  faith_status_code_t rc = FAITH_ERR_IO;

  if (res && PQresultStatus(res) == PGRES_TUPLES_OK) {
    *notify = PQntuples(res) == 1;
    rc = FAITH_OK;

    if (*notify && job->force) {
      const char *username = PQgetvalue(res, 0, 0);
      size_t size = PQgetlength(res, 0, 0);

      if (!size || !faith_username_valid(username, size))
        rc = FAITH_ERR_IO;
      else
        memcpy(job->username, username, size);
    }

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
  const char *force = job->force ? "true" : "false";
  const char *conversation = (const char *)job->conversation.bytes;
  const char *lock_values[] = {conversation, (const char *)job->owner.bytes, force};
  const int lock_lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE, 0};
  const int lock_formats[] = {1, 1, 0};
  PGresult *res = PQexecParams(
      db->conn,
      "SELECT 1 FROM conversations WHERE conversation_id = $1 AND owner_id = $2 "
      "AND force_username_disclosure = $3 FOR UPDATE",
      3, NULL, lock_values, lock_lengths, lock_formats, 0);
  faith_status_code_t rc = FAITH_ERR_IO;

  if (res && PQresultStatus(res) == PGRES_TUPLES_OK)
    rc = PQntuples(res) == 1 ? FAITH_OK : FAITH_ERR_UNAUTHORIZED;

  PQclear(res);

  if (rc != FAITH_OK)
    return rc;

  char limit[12];
  snprintf(limit, sizeof(limit), "%u", FAITH_GROUP_MAX_MEMBERS - 1);
  const char *values[] = {conversation, (const char *)job->recipient.bytes,
                          (const char *)job->device.bytes,
                          job->accepted ? "true" : "false", force,
                          job->disclose ? "true" : "false", limit};
  const int lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE,
                         FAITH_DEVICE_ID_SIZE, 0, 0, 0, 0};
  const int formats[] = {1, 1, 1, 0, 0, 0, 0};
  res = PQexecParams(
      db->conn,
      "UPDATE conversation_requests r SET accepted = $4, "
      "disclosed_username = CASE WHEN $4 AND ($5::boolean OR $6::boolean) "
      "THEN u.username END FROM users u "
      "WHERE r.conversation_id = $1 AND r.recipient_id = $2 AND u.auth_id = $2 "
      "AND r.accepted IS NULL "
      "AND (NOT ($4 AND ($5 OR $6)) OR u.username IS NOT NULL) "
      "AND EXISTS (SELECT 1 FROM devices "
      "WHERE auth_id = $2 AND device_id = $3 AND revoke_at IS NULL) "
      "AND (NOT $4 OR (SELECT count(*) FROM conversation_requests "
      "WHERE conversation_id = $1 AND accepted IS TRUE) < $7::integer) "
      "RETURNING r.disclosed_username",
      7, NULL, values, lengths, formats, 0);
  rc = FAITH_ERR_IO;

  if (res && PQresultStatus(res) == PGRES_TUPLES_OK) {
    *notify = PQntuples(res) == 1;
    rc = *notify ? FAITH_OK : FAITH_ERR_UNAUTHORIZED;

    if (*notify && !PQgetisnull(res, 0, 0)) {
      size_t size = PQgetlength(res, 0, 0);
      const char *username = PQgetvalue(res, 0, 0);

      if (!size || !faith_username_valid(username, size))
        rc = FAITH_ERR_IO;
      else
        memcpy(job->username, username, size);
    }
  }

  PQclear(res);

  return rc;
}

static faith_status_code_t _queue_event(faith_db_t *db, conversation_job_t *job,
                                        const faith_auth_id_t *target,
                                        faith_event_codec_type_t type,
                                        const uint8_t *payload, size_t size) {
  db_user_device_list_t devices = {0};
  faith_status_code_t rc = db_device_repo_get_device_list(db, target, &devices);

  for (size_t i = 0; i < devices.n_devices && rc == FAITH_OK; i++) {
    uint8_t *copy = malloc(size);

    if (!copy) {
      rc = FAITH_ERR_NOMEM;
      break;
    }

    memcpy(copy, payload, size);
    db_events_job_t event = {
        .operation = DB_EVENTS_INSERT,
        .in_transaction = true,
        .auth_id = *target,
        .device_id = devices.devices[i].device_id,
        .event = {.type = type, .data = copy, .data_size = size},
    };
    rc = db_events_repo_execute(db, &event);

    if (rc == FAITH_OK)
      arrput(job->events, event);
    else
      free(copy);
  }

  if (!devices.n_devices && type == FAITH_EVENT_CONVERSATION_REQUEST && rc == FAITH_OK)
    rc = FAITH_ERR_NOT_FOUND;

  db_device_repo_destroy_device_list(&devices);

  return rc;
}

static faith_status_code_t _disclose(faith_db_t *db, conversation_job_t *job) {
  const char *values[] = {(const char *)job->conversation.bytes,
                          (const char *)job->recipient.bytes};
  const int lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE};
  const int formats[] = {1, 1};
  PGresult *res = PQexecParams(
      db->conn,
      "SELECT owner_id, disclosed_username FROM conversations WHERE conversation_id = $1 "
      "UNION ALL SELECT recipient_id, disclosed_username FROM conversation_requests "
      "WHERE conversation_id = $1 AND accepted IS TRUE AND recipient_id <> $2",
      2, NULL, values, lengths, formats, 1);

  if (!res || PQresultStatus(res) != PGRES_TUPLES_OK ||
      PQntuples(res) >= (int)FAITH_GROUP_MAX_MEMBERS) {
    PQclear(res);

    return FAITH_ERR_IO;
  }

  /* the newcomer gets existing disclosures, and the other members get theirs */
  uint8_t disclosed[FAITH_CONVERSATION_ID_SIZE +
                    FAITH_GROUP_MAX_MEMBERS *
                        (FAITH_CONVERSATION_USERNAME_ENTRY_FIXED + FAITH_USERNAME_MAX)];
  uint8_t newcomer[FAITH_CONVERSATION_ID_SIZE +
                   FAITH_CONVERSATION_USERNAME_ENTRY_FIXED + FAITH_USERNAME_MAX];
  size_t used = FAITH_CONVERSATION_ID_SIZE;
  size_t username_size = strlen(job->username);
  memcpy(disclosed, job->conversation.bytes, FAITH_CONVERSATION_ID_SIZE);
  memcpy(newcomer, job->conversation.bytes, FAITH_CONVERSATION_ID_SIZE);
  memcpy(newcomer + FAITH_CONVERSATION_ID_SIZE, job->recipient.bytes,
         FAITH_AUTH_ID_SIZE);
  newcomer[FAITH_CONVERSATION_ID_SIZE + FAITH_AUTH_ID_SIZE] = username_size;
  memcpy(newcomer + FAITH_CONVERSATION_ID_SIZE + FAITH_CONVERSATION_USERNAME_ENTRY_FIXED,
         job->username, username_size);
  faith_status_code_t rc = FAITH_OK;

  for (int i = 0; i < PQntuples(res) && rc == FAITH_OK; i++) {
    faith_auth_id_t member;
    const char *name = PQgetvalue(res, i, 1);
    size_t size = PQgetlength(res, i, 1);

    if (PQgetlength(res, i, 0) != FAITH_AUTH_ID_SIZE ||
        (size && !faith_username_valid(name, size))) {
      rc = FAITH_ERR_IO;
      break;
    }

    memcpy(member.bytes, PQgetvalue(res, i, 0), FAITH_AUTH_ID_SIZE);

    if (size) {
      memcpy(disclosed + used, member.bytes, FAITH_AUTH_ID_SIZE);
      disclosed[used + FAITH_AUTH_ID_SIZE] = size;
      memcpy(disclosed + used + FAITH_CONVERSATION_USERNAME_ENTRY_FIXED, name, size);
      used += FAITH_CONVERSATION_USERNAME_ENTRY_FIXED + size;
    }

    if (username_size)
      rc = _queue_event(db, job, &member, FAITH_EVENT_CONVERSATION_USERNAMES,
                        newcomer, FAITH_CONVERSATION_ID_SIZE +
                                      FAITH_CONVERSATION_USERNAME_ENTRY_FIXED + username_size);
  }

  PQclear(res);

  if (rc == FAITH_OK && used > FAITH_CONVERSATION_ID_SIZE)
    rc = _queue_event(db, job, &job->recipient, FAITH_EVENT_CONVERSATION_USERNAMES,
                      disclosed, used);

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
    size_t size = request ? FAITH_CONVERSATION_REQUEST_SIZE
                          : FAITH_CONVERSATION_RESPONSE_SIZE;

    if (request && job->force) {
      size = FAITH_CONVERSATION_POLICY_REQUEST_SIZE + strlen(job->username);
      memcpy(job->payload + FAITH_CONVERSATION_POLICY_REQUEST_SIZE, job->username,
             size - FAITH_CONVERSATION_POLICY_REQUEST_SIZE);
    }

    rc = _queue_event(db, job, target,
                      request ? FAITH_EVENT_CONVERSATION_REQUEST
                              : FAITH_EVENT_CONVERSATION_RESPONSE,
                      job->payload, size);

    if (rc == FAITH_OK && !request && job->accepted)
      rc = _disclose(db, job);
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
  for (size_t i = 0; i < arrlenu(job->events); i++)
    free(job->events[i].event.data);

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

  if (request && cmd->payload_size == FAITH_CONVERSATION_POLICY_REQUEST_SIZE)
    job->force = cmd->payload[FAITH_CONVERSATION_REQUEST_SIZE];

  if (!request && cmd->payload_size == FAITH_CONVERSATION_SIGNED_POLICY_RESPONSE_SIZE) {
    job->force = cmd->payload[FAITH_CONVERSATION_RESPONSE_SIZE];
    job->disclose = cmd->payload[FAITH_CONVERSATION_RESPONSE_SIZE + 1];
  }

  memcpy(job->conversation.bytes, cmd->payload, FAITH_CONVERSATION_ID_SIZE);
  memcpy(job->payload, job->conversation.bytes, FAITH_CONVERSATION_ID_SIZE);
  memcpy(job->payload + FAITH_CONVERSATION_ID_SIZE, cl->ident.auth_id.bytes,
         FAITH_AUTH_ID_SIZE);
  job->payload[FAITH_CONVERSATION_REQUEST_SIZE] = request ? job->force : job->accepted;

  db_async_job_t task = {
      .run = _run, .complete = _complete, .destroy = _destroy, .data = job};
  faith_status_code_t rc = db_async_submit(&s->db_pool, &task);

  if (rc != FAITH_OK)
    _destroy(job);

  return rc;
}

faith_status_code_t
conv_claim(faith_db_t *db, const uint8_t id[FAITH_CONVERSATION_ID_SIZE],
           const faith_auth_id_t *owner, const bool *force) {
  const char *values[] = {(const char *)id, (const char *)owner->bytes,
                          force ? (*force ? "true" : "false") : NULL};
  const int lengths[] = {FAITH_CONVERSATION_ID_SIZE, FAITH_AUTH_ID_SIZE, 0};
  const int formats[] = {1, 1, 0};
  PGresult *res = PQexecParams(
      db->conn,
      "INSERT INTO conversations (conversation_id, owner_id, "
      "force_username_disclosure, disclosed_username) "
      "SELECT $1, auth_id, COALESCE($3::boolean, false), CASE WHEN $3 THEN username END "
      "FROM users WHERE auth_id = $2 AND ($3 IS NOT TRUE OR username IS NOT NULL) "
      "ON CONFLICT DO NOTHING",
      3, NULL, values, lengths, formats, 0);
  bool ok = res && PQresultStatus(res) == PGRES_COMMAND_OK;
  PQclear(res);

  if (!ok)
    return FAITH_ERR_IO;

  res = PQexecParams(db->conn,
                     "SELECT 1 FROM conversations "
                     "WHERE conversation_id = $1 AND owner_id = $2 "
                     "AND ($3::boolean IS NULL OR force_username_disclosure = $3) "
                     "FOR UPDATE",
                     3, NULL, values, lengths, formats, 0);
  faith_status_code_t rc = FAITH_ERR_IO;

  if (res && PQresultStatus(res) == PGRES_TUPLES_OK)
    rc = PQntuples(res) == 1 ? FAITH_OK : FAITH_ERR_UNAUTHORIZED;

  PQclear(res);

  return rc;
}

faith_status_code_t
conv_handle_conversation_request(server_state_t *s, client_conn_t *cl,
                                 const faith_envl_cts_command_t *cmd) {
  if (cmd->payload_size != FAITH_CONVERSATION_REQUEST_SIZE &&
      (cmd->payload_size != FAITH_CONVERSATION_POLICY_REQUEST_SIZE ||
       cmd->payload[FAITH_CONVERSATION_REQUEST_SIZE] > 1))
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
  bool policy = cmd->payload_size == FAITH_CONVERSATION_SIGNED_POLICY_RESPONSE_SIZE;

  if ((!policy && cmd->payload_size != FAITH_CONVERSATION_SIGNED_RESPONSE_SIZE) ||
      cmd->payload[FAITH_CONVERSATION_REQUEST_SIZE] > 1 ||
      (policy && (cmd->payload[FAITH_CONVERSATION_RESPONSE_SIZE] > 1 ||
                  cmd->payload[FAITH_CONVERSATION_RESPONSE_SIZE + 1] > 1 ||
                  (cmd->payload[FAITH_CONVERSATION_RESPONSE_SIZE + 1] &&
                   (!cmd->payload[FAITH_CONVERSATION_REQUEST_SIZE] ||
                     cmd->payload[FAITH_CONVERSATION_RESPONSE_SIZE])))))
    return FAITH_ERR_INVALID;

  client_device_session_data_t *sess = NULL;
  _FH_CHECK_RETURN(sess_registry_get_session(
      &s->rt, &cl->ident.auth_id, &cl->ident.device_id, &sess));

  if (!sess || sess->conn != cl)
    return FAITH_ERR_UNAUTHORIZED;

  uint8_t msg[FAITH_SIGNATURE_CONVERSATION_POLICY_RESPONSE_SIZE];
  size_t msg_size = FAITH_SIGNATURE_CONVERSATION_RESPONSE_SIZE;
  size_t signature_offset = FAITH_CONVERSATION_RESPONSE_SIZE;

  if (policy) {
    faith_gen_sign_buf_conversation_policy_response(
        msg, cmd->payload, &cl->ident.auth_id, &cl->ident.device_id);
    msg_size = FAITH_SIGNATURE_CONVERSATION_POLICY_RESPONSE_SIZE;
    signature_offset = FAITH_CONVERSATION_POLICY_RESPONSE_SIZE;
  } else {
    faith_gen_sign_buf_conversation_response(msg, cmd->payload,
                                             &cl->ident.auth_id, &cl->ident.device_id);
  }

  if (faith_verify_signature_raw_pubkey(
          sess->ident.public_key, msg, msg_size, cmd->payload + signature_offset,
          FAITH_ED25519_SIGNATURE_SIZE) != FAITH_OK)
    return FAITH_ERR_UNAUTHORIZED;

  faith_auth_id_t owner;
  memcpy(owner.bytes, cmd->payload + FAITH_CONVERSATION_ID_SIZE,
         FAITH_AUTH_ID_SIZE);

  return _submit(s, cl, cmd, &owner);
}
