#pragma once

#include <faith-proto/codec/codec.h>
#include "conn.h"

transport_result_t frame_try_full_read(transport_conn_t *conn,
                                       faith_frame_t    *frame);

faith_status_code_t frame_try_parse_from_buffer(const uint8_t *payload,
                                                size_t         payload_size,
                                                faith_frame_t *out,
                                                size_t        *consumed_out);
