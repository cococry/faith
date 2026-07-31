#pragma once

#include "conn.h"
#include "faith-proto/codec/auth.h"

#include <faith-proto/core/core.h>
#include <faith-proto/codec/events.h>

faith_status_code_t db_events_repo_insert(faith_db_t              *db,
                                          const faith_auth_id_t   *auth_id,
                                          const faith_device_id_t *device_id,
                                          const faith_envl_stc_event_t *event);
