#include "events_repo.h"

#include <inttypes.h>
#include <limits.h>
#include <third_party/stb_ds.h>

static bool      _command(faith_db_t *db, const char *sql);
static PGresult *_query(faith_db_t *db, const char *sql, int count,
                        const char **values, const int *lengths,
                        const int *formats);

static bool _command(faith_db_t *db, const char *sql) {
  PGresult *res = PQexec(db->conn, sql);
  bool      ok = res && PQresultStatus(res) == PGRES_COMMAND_OK;
  PQclear(res);
  return ok;
}

static PGresult *_query(faith_db_t *db, const char *sql, int count,
                        const char **values, const int *lengths,
                        const int *formats) {
  PGresult *res =
      PQexecParams(db->conn, sql, count, NULL, values, lengths, formats, 0);

  if (!res || (PQresultStatus(res) != PGRES_COMMAND_OK &&
               PQresultStatus(res) != PGRES_TUPLES_OK)) {
    PQclear(res);
    return NULL;
  }

  return res;
}

faith_status_code_t db_events_repo_execute(faith_db_t *db, void *data) {
  db_events_job_t *job = data;

  if (!db || !db->conn || !job || job->event.data_size > INT_MAX)
    return FAITH_ERR_INVALID;

  if (!job->in_transaction && !_command(db, "BEGIN"))
    return FAITH_ERR_IO;

  faith_status_code_t rc = FAITH_ERR_IO;
  char                sequence[32];
  char                type[16];
  snprintf(sequence, sizeof(sequence), "%" PRIu64, job->event.seq_num);
  snprintf(type, sizeof(type), "%u", (unsigned)job->event.type);
  const char *values[] = {(const char *)job->auth_id.bytes,
                          (const char *)job->device_id.bytes, sequence, type,
                          job->event.data ? (const char *)job->event.data : ""};
  int         lengths[] = {16, 16, 0, 0, (int)job->event.data_size};
  int         formats[] = {1, 1, 0, 0, 1};
  PGresult   *res = _query(
      db,
      "INSERT INTO device_event_inboxes (auth_id, device_id) VALUES ($1, $2) "
        "ON CONFLICT DO NOTHING",
      2, values, lengths, formats);

  if (!res)
    goto done;

  PQclear(res);
  res = _query(db,
               "SELECT next_seq, last_acked_seq FROM device_event_inboxes "
               "WHERE auth_id = $1 AND device_id = $2 FOR UPDATE",
               2, values, lengths, formats);

  if (!res || PQntuples(res) != 1)
    goto done;

  uint64_t next_seq = strtoull(PQgetvalue(res, 0, 0), NULL, 10);
  uint64_t acked = PQgetisnull(res, 0, 1)
                       ? UINT64_MAX
                       : strtoull(PQgetvalue(res, 0, 1), NULL, 10);
  PQclear(res);
  res = NULL;

  if (job->operation == DB_EVENTS_INSERT) {
    /* postgres counters are signed, so leave room for the next value */
    if (next_seq >= INT64_MAX) {
      rc = FAITH_ERR_OVERFLOW;
      goto done;
    }

    job->event.seq_num = next_seq;
    snprintf(sequence, sizeof(sequence), "%" PRIu64, next_seq);
    res = _query(db,
                 "INSERT INTO device_events "
                 "(auth_id, device_id, seq_num, event_type, event_data) "
                 "VALUES ($1, $2, $3::bigint, $4::integer, $5)",
                 5, values, lengths, formats);

    if (!res)
      goto done;

    PQclear(res);
    res = _query(db,
                 "UPDATE device_event_inboxes SET next_seq = next_seq + 1 "
                 "WHERE auth_id = $1 AND device_id = $2",
                 2, values, lengths, formats);
  } else if (job->operation == DB_EVENTS_ACK) {
    if (job->event.seq_num >= next_seq) {
      rc = FAITH_ERR_BAD_ENVELOPE;
      goto done;
    }

    res = _query(
        db,
        "DELETE FROM device_events WHERE auth_id = $1 AND device_id = $2 "
        "AND seq_num <= $3::bigint",
        3, values, lengths, formats);

    if (!res)
      goto done;

    PQclear(res);
    res = _query(db,
                 "UPDATE device_event_inboxes SET last_acked_seq = "
                 "GREATEST(last_acked_seq, $3::bigint) "
                 "WHERE auth_id = $1 AND device_id = $2",
                 3, values, lengths, formats);
  } else if (job->operation == DB_EVENTS_LOAD) {
    device_event_inbox_init(&job->inbox);
    job->inbox.next_seq = next_seq;
    job->inbox.last_sent_seq = next_seq ? next_seq - 1 : UINT64_MAX;
    job->inbox.last_acked_seq = acked;
    res = _query(db,
                 "SELECT seq_num, event_type, event_data FROM device_events "
                 "WHERE auth_id = $1 AND device_id = $2 ORDER BY seq_num",
                 2, values, lengths, formats);

    if (!res)
      goto done;

    for (int i = 0; i < PQntuples(res); i++) {
      faith_envl_stc_event_t event = {0};
      event.seq_num = strtoull(PQgetvalue(res, i, 0), NULL, 10);
      event.type = strtoul(PQgetvalue(res, i, 1), NULL, 10);
      size_t         size = 0;
      unsigned char *bytes =
          PQunescapeBytea((unsigned char *)PQgetvalue(res, i, 2), &size);

      if (!bytes || size > UINT32_MAX) {
        PQfreemem(bytes);
        rc = FAITH_ERR_NOMEM;
        goto done;
      }

      event.data_size = size;
      event.data = malloc(size ? size : 1);

      if (!event.data) {
        PQfreemem(bytes);
        rc = FAITH_ERR_NOMEM;
        goto done;
      }

      memcpy(event.data, bytes, size);
      PQfreemem(bytes);
      arrput(job->inbox.events, event);
    }
  } else {
    rc = FAITH_ERR_INVALID;
    goto done;
  }

  if (res)
    rc = FAITH_OK;

done:
  PQclear(res);

  if (job->in_transaction)
    return rc;

  if (rc == FAITH_OK) {
    if (!_command(db, "COMMIT"))
      rc = FAITH_ERR_IO;
  } else {
    _command(db, "ROLLBACK");
  }

  return rc;
}

void db_events_repo_destroy(void *data) {
  db_events_job_t *job = data;
  free(job->event.data);

  for (size_t i = 0; i < arrlenu(job->inbox.events); i++)
    free(job->inbox.events[i].data);

  arrfree(job->inbox.events);
  free(job);
}
