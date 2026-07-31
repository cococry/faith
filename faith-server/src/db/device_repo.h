#pragma once

#include "conn.h"

#include <faith-proto/codec/auth.h>
#include <faith-proto/core/crypto.h>

typedef enum {
  DB_DEVICE_FIELD_DEVICE_ID,
  DB_DEVICE_FIELD_PUBLIC_KEY,
  DB_DEVICE_REPO_USER_DEVICE_FIELDS,
} db_user_device_fields_t;

typedef struct {
  faith_device_id_t device_id;
  uint8_t           public_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
} db_user_device_t;

typedef struct {
  db_user_device_t *devices;
  size_t            n_devices;
} db_user_device_list_t;

faith_status_code_t db_device_repo_insert(faith_db_t             *db,
                                          const faith_auth_id_t  *auth_id,
                                          const db_user_device_t *device);

faith_status_code_t db_device_repo_delete(faith_db_t              *db,
                                          const faith_auth_id_t   *auth_id,
                                          const faith_device_id_t *device_id);

faith_status_code_t
db_device_repo_get_device_list(faith_db_t *db, const faith_auth_id_t *auth_id,
                               db_user_device_list_t *o_list);

faith_status_code_t
db_device_repo_destroy_device_list(db_user_device_list_t *list);
