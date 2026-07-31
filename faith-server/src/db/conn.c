#include "conn.h"

#define _MODULE_NAME "db/conn"

faith_status_code_t db_connect(const char *connection_str, faith_db_t *o_db) {
  if (!o_db || !connection_str)
    return FAITH_ERR_INVALID;

  memset(o_db, 0, sizeof(*o_db));

  o_db->conn = PQconnectdb(connection_str);
  if (!o_db->conn) {
    return FAITH_ERR_NOMEM;
  }
  if (PQstatus(o_db->conn) != CONNECTION_OK) {
    nob_log(ERROR, "[%s] Database connection failed: %s", _MODULE_NAME,
            PQerrorMessage(o_db->conn));

    PQfinish(o_db->conn);
    o_db->conn = NULL;
    return FAITH_ERR_IO;
  }

  return FAITH_OK;
}

faith_status_code_t db_disconnect(faith_db_t *db) {
  if (!db)
    return FAITH_ERR_INVALID;
  if (db->conn != NULL) {
    PQfinish(db->conn);
    db->conn = NULL;
  }
  return FAITH_OK;
}
