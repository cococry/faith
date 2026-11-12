#pragma once

#include "../delivery/event_inbox.h"
#include "conn.h"
#include "faith-proto/codec/auth.h"

typedef enum {
  DB_EVENTS_INSERT,
  DB_EVENTS_LOAD,
  DB_EVENTS_ACK,
  DB_EVENTS_EXPIRE,
  DB_EVENTS_SYNC,
} db_events_operation_t;

typedef struct {
  faith_auth_id_t   auth_id;
  faith_device_id_t device_id;
  uint64_t          until;
} db_expired_inbox_t;

typedef struct {
  db_events_operation_t  operation;
  bool                   in_transaction;
  uint64_t               client_id;
  faith_auth_id_t        auth_id;
  faith_device_id_t      device_id;
  faith_envl_stc_event_t event;
  device_event_inbox_t   inbox;
  db_expired_inbox_t    *expired;
} db_events_job_t;

faith_status_code_t db_events_repo_execute(faith_db_t *db, void *data);
void                db_events_repo_destroy(void *data);
