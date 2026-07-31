#pragma once

#include <libpq-fe.h>

#include <faith-proto/core/core.h>

typedef struct {
  PGconn *conn;
} faith_db_t;

faith_status_code_t db_connect(const char *connection_str, faith_db_t *o_db);

faith_status_code_t db_disconnect(faith_db_t *db);
