#pragma once

#include <faith-proto/codec/events.h>
#include <faith-proto/codec/envelopes.h>
#include <faith-proto/core/core.h>
#include "../server/server.h"

faith_status_code_t delivery_queue_event(server_state_t          *s,
                                         const faith_auth_id_t   *auth_id,
                                         const faith_device_id_t *device_id,
                                         faith_event_codec_type_t type,
                                         uint8_t                 *data,
                                         faith_body_size_t        data_size);

faith_status_code_t delivery_queue_pending_events(server_state_t *s,
                                                  client_conn_t  *cl);

faith_status_code_t delivery_handle_event_acked(server_state_t   *s,
                                                client_conn_t    *cl,
                                                faith_envelope_t *envl);
