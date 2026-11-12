#include "conn.h"

#define _MODULE_NAME "db/conn"

faith_status_code_t db_connect(const char *connection_str, faith_db_t *o_db) {
  if (!o_db || !connection_str)
    return FAITH_ERR_INVALID;

  memset(o_db, 0, sizeof(*o_db));

  char        password[4096] = {0};
  const char *password_file = getenv("FAITH_DATABASE_PASSWORD_FILE");

  if (password_file) {
    FILE *file = fopen(password_file, "rb");

    if (!file)
      return FAITH_ERR_IO;
    size_t size = fread(password, 1, sizeof(password) - 1, file);
    bool   valid = !ferror(file) && feof(file) && size > 0;
    fclose(file);

    if (!valid)
      return FAITH_ERR_IO;
    password[strcspn(password, "\r\n")] = 0;
  }

  const char *keywords[] = {"dbname", "connect_timeout", "password", NULL};
  const char *values[] = {connection_str, "3", password_file ? password : NULL,
                          NULL};
  o_db->conn = PQconnectdbParams(keywords, values, 1);
  explicit_bzero(password, sizeof(password));

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
