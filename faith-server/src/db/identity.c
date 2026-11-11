#include "identity.h"

static bool _command(faith_db_t *db, const char *sql);

static bool _command(faith_db_t *db, const char *sql) {
  PGresult *res = PQexec(db->conn, sql);
  bool      ok = res && PQresultStatus(res) == PGRES_COMMAND_OK;
  PQclear(res);
  return ok;
}

faith_status_code_t db_identity_resolve(faith_db_t        *db,
                                        db_identity_job_t *job) {
  if (!db || !db->conn || !job)
    return FAITH_ERR_INVALID;

  if (!_command(db, "BEGIN") ||
      !_command(db, "SET LOCAL statement_timeout = '3000ms'") ||
      !_command(db, "SET LOCAL lock_timeout = '2000ms'")) {
    _command(db, "ROLLBACK");
    return FAITH_ERR_IO;
  }

  const char         *values[] = {(const char *)job->auth_id.bytes};
  const int           lengths[] = {FAITH_AUTH_ID_SIZE};
  const int           formats[] = {1};
  PGresult           *res = NULL;
  bool                created = false;
  faith_status_code_t rc = FAITH_ERR_IO;

  /* only the transaction that claims the id gets to choose its first key */
  if (!job->link) {
    res = PQexecParams(db->conn,
                       "INSERT INTO users (auth_id) VALUES ($1) "
                       "ON CONFLICT DO NOTHING RETURNING auth_id",
                       1, NULL, values, lengths, formats, 1);

    if (!res || PQresultStatus(res) != PGRES_TUPLES_OK)
      goto done;

    created = PQntuples(res) == 1;
    PQclear(res);
    res = NULL;
  }

  res = PQexecParams(db->conn,
                     "SELECT auth_id FROM users WHERE auth_id = $1 FOR UPDATE",
                     1, NULL, values, lengths, formats, 1);

  if (!res || PQresultStatus(res) != PGRES_TUPLES_OK || PQntuples(res) != 1)
    goto done;

  PQclear(res);
  res = NULL;

  db_user_device_list_t list = {0};
  rc = db_device_repo_get_device_list(db, &job->auth_id, &list);

  if (rc != FAITH_OK)
    goto done;

  bool approved = false;
  bool found = false;
  bool matches = false;

  for (size_t i = 0; i < list.n_devices; i++) {
    db_user_device_t *device = &list.devices[i];

    if (faith_device_id_equal(device->device_id, job->device.device_id)) {
      found = true;
      matches = memcmp(device->public_key, job->device.public_key,
                       FAITH_ED25519_PUBLIC_KEY_SIZE) == 0;
    }

    if (faith_device_id_equal(device->device_id, job->approver_id) &&
        memcmp(device->public_key, job->approver_key,
               FAITH_ED25519_PUBLIC_KEY_SIZE) == 0)
      approved = true;
  }

  db_device_repo_destroy_device_list(&list);

  if (found)
    rc = matches ? FAITH_OK : FAITH_ERR_UNAUTHORIZED;
  else if (created || (job->link && approved))
    rc = db_device_repo_insert(db, &job->auth_id, &job->device);
  else
    rc = job->link ? FAITH_ERR_UNAUTHORIZED : FAITH_ERR_NOT_FOUND;

done:
  PQclear(res);

  if (rc == FAITH_OK) {
    if (!_command(db, "COMMIT"))
      rc = FAITH_ERR_IO;
  } else {
    _command(db, "ROLLBACK");
  }

  return rc;
}
