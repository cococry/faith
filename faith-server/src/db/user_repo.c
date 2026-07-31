#include "user_repo.h"
#include "faith-proto/codec/auth.h"
#include "faith-proto/core/core.h"
#include <libpq-fe.h>

#define _MODULE_NAME "db/user_repo"

faith_status_code_t db_user_repo_insert(faith_db_t            *db,
                                        const faith_auth_id_t *auth_id) {
  if (!db || !db->conn || !auth_id)
    return FAITH_ERR_INVALID;

  const char *values[] = {(const char *)auth_id->bytes};
  const int   lengths[] = {FAITH_AUTH_ID_SIZE};
  const int   formats[] = {1};

  PGresult *res = PQexecParams(db->conn,
                               "INSERT INTO users (auth_id) "
                               "VALUES ($1) "
                               "ON CONFLICT (auth_id) DO NOTHING",
                               1, NULL, values, lengths, formats, 0);

  if (!res)
    return FAITH_ERR_NOMEM;

  faith_status_code_t rc = FAITH_OK;

  if (PQresultStatus(res) != PGRES_COMMAND_OK) {
    char auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1];
    _FH_CHECK(faith_id128_to_hex(auth_id->bytes, auth_id_hex));

    if (_fh_rc == FAITH_OK) {
      nob_log(ERROR, "[%s] User insert failed (auth_id=%s): %s", _MODULE_NAME,
              auth_id_hex, PQresultErrorMessage(res));
    } else {
      nob_log(ERROR, "[%s] User insert failed: %s", _MODULE_NAME,
              PQresultErrorMessage(res));
    }

    rc = FAITH_ERR_IO;
  }

  PQclear(res);
  return rc;
}
faith_status_code_t db_user_repo_delete(faith_db_t            *db,
                                        const faith_auth_id_t *auth_id) {
  if (!db || !db->conn || !auth_id)
    return FAITH_ERR_INVALID;

  const char *values[] = {(const char *)auth_id->bytes};
  const int   lengths[] = {FAITH_AUTH_ID_SIZE};
  const int   formats[] = {1};

  PGresult *res = PQexecParams(db->conn,
                               "DELETE FROM users "
                               "WHERE auth_id is $1 "
                               "RETURNING auth_id ",
                               1, NULL, values, lengths, formats, 0);

  if (!res)
    return FAITH_ERR_NOMEM;

  faith_status_code_t _fh_result = FAITH_OK;

  if (PQresultStatus(res) != PGRES_TUPLES_OK) {
    char auth_id_hex[FAITH_AUTH_ID_SIZE * 2 + 1] = {0};

    _FH_CHECK(faith_id128_to_hex(auth_id->bytes, auth_id_hex));
    if (_fh_rc == FAITH_OK) {
      nob_log(ERROR, "[%s] User removal failed (auth_id=%s): %s", _MODULE_NAME,
              auth_id_hex, PQresultErrorMessage(res));
    } else {
      nob_log(ERROR, "[%s] User removal failed: %s", _MODULE_NAME,
              PQresultErrorMessage(res));
    }

    _FH_RETURN_DEFER(FAITH_ERR_IO);
  }

  int n_tuples = PQntuples(res);

  switch (n_tuples) {
  case 0:
    _FH_RETURN_DEFER(FAITH_ERR_NOT_FOUND);
    break;
  case 1:
    _FH_RETURN_DEFER(FAITH_OK);
    break;
  default:
    nob_log(ERROR, "[%s] User delete returned an unexpected number of rows: %d",
            _MODULE_NAME, n_tuples);

    _FH_RETURN_DEFER(FAITH_ERR_INVALID);
    break;
  }

defer:
  PQclear(res);
  return _fh_result;
}
