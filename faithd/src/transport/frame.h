#pragma once

#include "../protocol.h"
#include "conn.h"

conn_read_result_t frame_try_full_read(transport_conn_t *conn,
                                       faith_frame_t    *frame,
                                       bool              verbose_logging);

faith_status_code_t frame_try_parse_from_buffer(const uint8_t *payload,
                                                size_t         payload_size,
                                                faith_frame_t *out,
                                                size_t        *consumed_out);
