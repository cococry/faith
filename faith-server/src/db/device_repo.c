#include "device_repo.h"
#include "faith-proto/core/core.h"
#include <libpq-fe.h>

#define _MODULE_NAME "db/device_repo"

faith_status_code_t db_device_repo_insert(faith_db_t             *db,
                                          const faith_auth_id_t  *auth_id,
                                          const db_user_device_t *device) {
  if (!db || !db->conn || !auth_id || !device)
    return FAITH_ERR_INVALID;

  const char *values[] = {
      (const char *)auth_id->bytes,
      (const char *)device->device_id.bytes,
      (const char *)device->public_key,
  };
  const int lengths[] = {FAITH_AUTH_ID_SIZE, FAITH_DEVICE_ID_SIZE,
                         FAITH_ED25519_PUBLIC_KEY_SIZE};
  const int formats[] = {1, 1, 1};

  PGresult *res =
      PQexecParams(db->conn,
                   "INSERT INTO devices (auth_id, device_id, public_key) "
                   "VALUES ($1, $2, $3) "
                   "ON CONFLICT (auth_id, device_id) "
                   "DO UPDATE SET "
                   "    public_key = EXCLUDED.public_key, "
                   "    revoked_at = NULL",
                   3, NULL, values, lengths, formats, 0);

  if (!res)
    return FAITH_ERR_NOMEM;

  faith_status_code_t _fh_result = FAITH_OK;

  if (PQresultStatus(res) != PGRES_COMMAND_OK) {
    char auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1] = {0};
    _FH_CHECK(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
    if (_fh_rc == FAITH_OK) {
      nob_log(ERROR, "[%s] Device registration failed (auth_id=%s): %s",
              _MODULE_NAME, auth_id_hex, PQresultErrorMessage(res));
    } else {
      nob_log(ERROR, "[%s] Device registration failed: %s", _MODULE_NAME,
              PQresultErrorMessage(res));
    }

    _FH_RETURN_DEFER(FAITH_ERR_IO);
  }

defer:
  PQclear(res);
  return _fh_result;
}

faith_status_code_t db_device_repo_delete(faith_db_t              *db,
                                          const faith_auth_id_t   *auth_id,
                                          const faith_device_id_t *device_id) {

  if (!db || !db->conn || !auth_id || !device_id)
    return FAITH_ERR_INVALID;

  const char *values[] = {
      (const char *)auth_id->bytes,
      (const char *)device_id->bytes,
  };
  const int lengths[] = {FAITH_AUTH_ID_SIZE, FAITH_DEVICE_ID_SIZE};
  const int formats[] = {1, 1};

  PGresult *res = PQexecParams(db->conn,
                               "DELETE FROM devices "
                               "WHERE auth_id = $1 "
                               "  AND device_id = $2 "
                               "RETURNING device_id",
                               2, NULL, values, lengths, formats, 0);

  if (!res)
    return FAITH_ERR_NOMEM;

  faith_status_code_t _fh_result = FAITH_OK;

  if (PQresultStatus(res) != PGRES_TUPLES_OK) {
    char auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1] = {0};
    char device_id_hex[FAITH_DEVICE_ID_SIZE * 2 + 1] = {0};

    faith_status_code_t rc = FAITH_OK;
    {
      _FH_CHECK(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
      if (_fh_rc != FAITH_OK)
        rc = _fh_rc;
    }
    {
      _FH_CHECK(faith_id128_to_hex(device_id->bytes, device_id_hex));
      if (_fh_rc != FAITH_OK)
        rc = _fh_rc;
    }

    if (rc == FAITH_OK) {
      nob_log(
          ERROR, "[%s] Device removal failed (auth_id=%s, device_id=%s): %s",
          _MODULE_NAME, auth_id_hex, device_id_hex, PQresultErrorMessage(res));
    } else {
      nob_log(ERROR, "[%s] Device removal failed: %s", _MODULE_NAME,
              PQresultErrorMessage(res));
    }

    _FH_RETURN_DEFER(FAITH_ERR_IO);
  }

  int n_tuples = PQntuples(res);

  switch (n_tuples) {
  case 0:
    _FH_RETURN_DEFER(FAITH_ERR_NOT_FOUND);

  case 1:
    _FH_RETURN_DEFER(FAITH_OK);

  default:
    nob_log(ERROR, "[%s] Device removal returned %d rows; expected at most 1",
            _MODULE_NAME, n_tuples);

    _FH_RETURN_DEFER(FAITH_ERR_INVALID);
  }

defer:
  PQclear(res);
  return _fh_result;
}

faith_status_code_t
db_device_repo_get_device_list(faith_db_t *db, const faith_auth_id_t *auth_id,
                               db_user_device_list_t *o_list) {
  if (!db || !db->conn || !auth_id || !o_list)
    return FAITH_ERR_INVALID;

  o_list->devices = NULL;
  o_list->n_devices = 0;

  const char *values[] = {(const char *)auth_id->bytes};
  const int   lengths[] = {FAITH_AUTH_ID_SIZE};
  const int   formats[] = {1};

  PGresult *res = PQexecParams(db->conn,
                               "SELECT device_id, public_key "
                               "FROM devices "
                               "WHERE auth_id = $1 "
                               "  AND revoked_at IS NULL "
                               "ORDER BY registered_at, device_id",
                               1, NULL, values, lengths, formats, 1);

  if (!res)
    return FAITH_ERR_NOMEM;

  db_user_device_t   *devices = NULL;
  faith_status_code_t _fh_result = FAITH_OK;

  if (PQresultStatus(res) != PGRES_TUPLES_OK) {
    char auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1] = {0};
    _FH_CHECK(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
    if (_fh_rc == FAITH_OK) {
      nob_log(ERROR, "[%s] Device query failed (auth_id=%s): %s", _MODULE_NAME,
              auth_id_hex, PQresultErrorMessage(res));
    } else {
      nob_log(ERROR, "[%s] Device query failed: %s", _MODULE_NAME,
              PQresultErrorMessage(res));
    }

    _FH_RETURN_DEFER(FAITH_ERR_IO);
  }

  int n_rows = PQntuples(res);

  /* no devices in account */
  if (n_rows == 0)
    _FH_RETURN_DEFER(FAITH_OK);

  int n_fields = PQnfields(res);
  if (n_fields != DB_DEVICE_REPO_USER_DEVICE_FIELDS) {
    nob_log(ERROR,
            "[%s] Device query returned an unexpected number of columns: %d",
            _MODULE_NAME, PQnfields(res));

    _FH_RETURN_DEFER(FAITH_ERR_IO);
  }

  devices = calloc((size_t)n_rows, sizeof(*devices));
  if (!devices)
    _FH_RETURN_DEFER(FAITH_ERR_NOMEM);

  for (int r = 0; r < n_rows; r++) {

    /* check for any unexpected NULL values */
    for (int f = 0; f < n_fields; f++) {
      if (!PQgetisnull(res, r, f))
        continue;
      nob_log(ERROR,
              "[%s] Device query returned an unexpected NULL field value "
              "(field %i)",
              _MODULE_NAME, f);

      _FH_RETURN_DEFER(FAITH_ERR_IO);
    }

    int device_id_size = PQgetlength(res, r, DB_DEVICE_FIELD_DEVICE_ID);
    int public_key_size = PQgetlength(res, r, DB_DEVICE_FIELD_PUBLIC_KEY);

    if (device_id_size != FAITH_DEVICE_ID_SIZE) {
      nob_log(ERROR, "[%s] Device query returned invalid device_id size: %d",
              _MODULE_NAME, device_id_size);

      _FH_RETURN_DEFER(FAITH_ERR_IO);
    }

    if (public_key_size != FAITH_ED25519_PUBLIC_KEY_SIZE) {
      nob_log(ERROR, "[%s] Device query returned invalid public_key size: %d",
              _MODULE_NAME, public_key_size);

      _FH_RETURN_DEFER(FAITH_ERR_IO);
    }

    /* copy data */
    memcpy(devices[r].device_id.bytes,
           PQgetvalue(res, r, DB_DEVICE_FIELD_DEVICE_ID), FAITH_DEVICE_ID_SIZE);

    memcpy(devices[r].public_key,
           PQgetvalue(res, r, DB_DEVICE_FIELD_PUBLIC_KEY),
           FAITH_ED25519_PUBLIC_KEY_SIZE);
  }
  o_list->devices = devices;
  o_list->n_devices = (size_t)n_rows;
  devices = NULL;

defer:
  if (devices != NULL) {
    free(devices);
  }
  PQclear(res);
  return _fh_result;
}

faith_status_code_t
db_device_repo_destroy_device_list(db_user_device_list_t *list) {
  if (!list)
    return FAITH_ERR_INVALID;

  free(list->devices);

  list->devices = NULL;
  list->n_devices = 0;

  return FAITH_OK;
}
