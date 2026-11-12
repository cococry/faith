#include "conversation.h"

#include "../db/device_repo.h"
#include "../delivery/events.h"
#include "dispatch.h"

#include <third_party/stb_ds.h>

typedef struct {
  bool               uncertain;
  uint64_t           client_id;
  faith_auth_id_t    sender;
  faith_device_id_t  device;
  faith_auth_id_t    recipient;
  faith_command_id_t command_id;
  uint8_t            payload[FAITH_EVENT_CONVERSATION_CREATED_DATA_SIZE];
  db_events_job_t   *events;
} conversation_job_t;

static bool                _command(faith_db_t *db, const char *sql);
static faith_status_code_t _create(faith_db_t *db, void *data);
static void _complete(void *owner, void *data, faith_status_code_t status);
static void _destroy(void *data);

static bool _command(faith_db_t *db, const char *sql) {
  PGresult *res = PQexec(db->conn, sql);
  bool      ok = res && PQresultStatus(res) == PGRES_COMMAND_OK;
  PQclear(res);

  return ok;
}

static faith_status_code_t _create(faith_db_t *db, void *data) {
  conversation_job_t   *job = data;
  db_user_device_list_t devices = {0};
  faith_status_code_t   rc =
      db_device_repo_get_device_list(db, &job->recipient, &devices);

  if (rc != FAITH_OK)
    return rc;

  if (!devices.n_devices) {
    db_device_repo_destroy_device_list(&devices);

    return FAITH_ERR_NOT_FOUND;
  }

  if (!_command(db, "BEGIN")) {
    db_device_repo_destroy_device_list(&devices);

    return FAITH_ERR_IO;
  }

  /* commit all recipient inboxes together before reporting success */
  for (size_t i = 0; i <= devices.n_devices; i++) {
    db_events_job_t event = {
        .operation = DB_EVENTS_INSERT,
        .in_transaction = true,
        .auth_id = i ? job->recipient : job->sender,
        .device_id = i ? devices.devices[i - 1].device_id : job->device,
        .event = {.type = FAITH_EVENT_CONVERSATION_CREATED,
                  .data = job->payload,
                  .data_size = sizeof(job->payload)},
    };
    rc = db_events_repo_execute(db, &event);

    if (rc != FAITH_OK)
      break;

    arrput(job->events, event);
  }

  db_device_repo_destroy_device_list(&devices);

  if (rc == FAITH_OK) {
    if (!_command(db, "COMMIT")) {
      job->uncertain = true;
      rc = FAITH_ERR_IO;
    }

  } else {
    if (!_command(db, "ROLLBACK"))
      job->uncertain = true;
  }

  return rc;
}

static void _complete(void *owner, void *data, faith_status_code_t status) {
  server_state_t     *s = owner;
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
      s, cl, &job->command_id, FAITH_COMMAND_CREATE_CONVERSATION,
      status == FAITH_OK              ? FAITH_COMMAND_ERR_NONE
      : status == FAITH_ERR_NOT_FOUND ? FAITH_COMMAND_ERR_USER_NOT_FOUND
                                      : FAITH_COMMAND_ERR_INTERNAL_ERROR,
      status == FAITH_OK ? FAITH_COMMAND_RESULT_ACCEPTED
                         : FAITH_COMMAND_RESULT_REJECTED);
}

static void _destroy(void *data) {
  conversation_job_t *job = data;
  arrfree(job->events);
  free(job);
}

faith_status_code_t
conv_handle_create_conversation(server_state_t *s, client_conn_t *cl,
                                const faith_envl_cts_command_t *cmd) {
  faith_cmd_create_converstation_t request = {0};
  _FH_CHECK_RETURN(faith_decode_cmd_create_conversation(
      cmd->payload, cmd->payload_size, &request));

  conversation_job_t *job = calloc(1, sizeof(*job));

  if (!job)
    return FAITH_ERR_NOMEM;

  job->client_id = cl->conn.id;
  job->sender = cl->ident.auth_id;
  job->device = cl->ident.device_id;
  job->recipient = request.conversant_id;
  job->command_id = cmd->cmd_id;
  faith_status_code_t rc =
      faith_random_bytes(job->payload, sizeof(job->payload));

  if (rc == FAITH_OK) {
    db_async_job_t task = {.run = _create,
                           .complete = _complete,
                           .destroy = _destroy,
                           .data = job};

    rc = db_async_submit(&s->db_pool, &task);
  }

  if (rc != FAITH_OK)
    _destroy(job);

  return rc;
}
