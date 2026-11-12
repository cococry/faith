#pragma once

#include "device_repo.h"

typedef struct {
  uint64_t            client_id;
  faith_auth_id_t     auth_id;
  db_user_device_t    device;
  bool                link;
  faith_device_id_t   approver_id;
  uint8_t             approver_key[FAITH_ED25519_PUBLIC_KEY_SIZE];
  faith_status_code_t status;
} db_identity_job_t;

faith_status_code_t db_identity_resolve(faith_db_t *db, db_identity_job_t *job);
