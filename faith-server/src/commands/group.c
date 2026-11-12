#include "conversation.h"
#include <faith-proto/codec/group.h>

#include "../db/device_repo.h"
#include "../delivery/events.h"
#include "dispatch.h"

#include <third_party/stb_ds.h>

typedef struct {
  bool               uncertain;
  uint64_t           client_id;
  faith_command_id_t command_id;
  faith_auth_id_t    targets[FAITH_GROUP_MAX_TARGETS];
  uint16_t           n_targets;
  uint8_t           *payload;
  size_t             payload_size;
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
  conversation_job_t *job = data;

  if (!_command(db, "BEGIN"))
    return FAITH_ERR_IO;

  faith_status_code_t rc = FAITH_OK;

  for (size_t target = 0; target < job->n_targets && rc == FAITH_OK; target++) {
    db_user_device_list_t devices = {0};
    rc = db_device_repo_get_device_list(db, &job->targets[target], &devices);

    if (rc == FAITH_OK && !devices.n_devices)
      rc = FAITH_ERR_NOT_FOUND;

    for (size_t i = 0; i < devices.n_devices && rc == FAITH_OK; i++) {
      /* TOOD: remove this arbitrary limit*/
      if (arrlenu(job->events) >= 128) {
        rc = FAITH_ERR_OVERFLOW;
        break;
      }

      db_events_job_t event = {.operation = DB_EVENTS_INSERT,
                               .in_transaction = true,
                               .auth_id = job->targets[target],
                               .device_id = devices.devices[i].device_id,
                               .event = {.type = FAITH_EVENT_GROUP_PACKET,
                                         .data = job->payload,
                                         .data_size = job->payload_size}};

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
  } else if (!_command(db, "ROLLBACK"))
    job->uncertain = true;

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

  command_queue_result(s, cl, &job->command_id, FAITH_COMMAND_GROUP_DELIVER,
                       status == FAITH_OK ? FAITH_COMMAND_ERR_NONE
                       : status == FAITH_ERR_NOT_FOUND
                           ? FAITH_COMMAND_ERR_USER_NOT_FOUND
                           : FAITH_COMMAND_ERR_INTERNAL_ERROR,
                       status == FAITH_OK ? FAITH_COMMAND_RESULT_ACCEPTED
                                          : FAITH_COMMAND_RESULT_REJECTED);
}

static void _destroy(void *data) {
  conversation_job_t *job = data;
  arrfree(job->events);
  free(job->payload);
  free(job);
}

faith_status_code_t
conv_handle_group_delivery(server_state_t *s, client_conn_t *cl,
                           const faith_envl_cts_command_t *cmd) {
  if (cmd->payload_size < 2)
    return FAITH_ERR_INVALID;

  uint16_t count = ((uint16_t)cmd->payload[0] << 8) | cmd->payload[1];
  size_t   offset = 2 + 16u * count;

  if (!count || count > FAITH_GROUP_MAX_TARGETS || offset > cmd->payload_size)
    return FAITH_ERR_INVALID;

  faith_group_packet_t packet;

  _FH_CHECK_RETURN(faith_group_decode(cmd->payload + offset,
                                      cmd->payload_size - offset, &packet));

  if (packet.kind != FAITH_GROUP_UPDATE && count != packet.n_members)
    return FAITH_ERR_INVALID;

  bool member = false, target = false;

  for (size_t i = 0; i < packet.n_members; i++)
    if (faith_client_id_equal(packet.members[i], cl->ident.auth_id))
      member = true;

  /*a non-member trying to do a group operation or a non-creator trying
   * to do something else then sending a message must be rejected. */
  if (!member || (packet.kind != FAITH_GROUP_MESSAGE &&
                  !faith_client_id_equal(packet.members[0], cl->ident.auth_id)))
    return FAITH_ERR_UNAUTHORIZED;

  conversation_job_t *job = calloc(1, sizeof(*job));

  if (!job)
    return FAITH_ERR_NOMEM;

  job->n_targets = count;
  /* the outer payload holds the group targets, after the uint16_t count */
  memcpy(job->targets, cmd->payload + 2, 16u * count);

  for (size_t i = 0; i < count; i++) {
    if (faith_client_id_equal(job->targets[i], cl->ident.auth_id))
      target = true;

    for (size_t j = 0; j < i; j++)
      /* there must be no duplicate targets */
      if (faith_client_id_equal(job->targets[i], job->targets[j])) {
        free(job);

        return FAITH_ERR_INVALID;
      }
  }

  for (size_t i = 0; i < packet.n_members; i++) {
    bool found = false;

    for (size_t j = 0; j < count; j++)
      /* each member of the group must be a target. this disallows someone to
       * send an operation to only a subset of the group. */
      if (faith_client_id_equal(packet.members[i], job->targets[j]))
        found = true;

    if (!found) {
      free(job);

      return FAITH_ERR_INVALID;
    }
  }

  /* we must be a target (senders are also expected to be targets) */
  if (!target) {
    free(job);

    return FAITH_ERR_INVALID;
  }

  job->client_id = cl->conn.id;
  job->command_id = cmd->cmd_id;
  job->payload_size = 16 + cmd->payload_size - offset;
  job->payload = malloc(job->payload_size);

  if (!job->payload) {
    free(job);

    return FAITH_ERR_NOMEM;
  }

  memcpy(job->payload, cl->ident.auth_id.bytes, 16);
  memcpy(job->payload + 16, cmd->payload + offset, cmd->payload_size - offset);
  db_async_job_t task = {
      .run = _create, .complete = _complete, .destroy = _destroy, .data = job};

  faith_status_code_t rc = db_async_submit(&s->db_pool, &task);

  if (rc != FAITH_OK)
    _destroy(job);

  return rc;
}
