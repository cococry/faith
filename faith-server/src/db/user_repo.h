#pragma once

#include "conn.h"
#include "faith-proto/codec/auth.h"

faith_status_code_t db_user_repo_insert(faith_db_t* db, const faith_auth_id_t* auth_id);

faith_status_code_t db_user_repo_delete(faith_db_t* db, const faith_auth_id_t* auth_id);
