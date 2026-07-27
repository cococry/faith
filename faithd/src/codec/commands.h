#pragma once

#include <stdint.h>

#include "../auth/structs.h"
#include "core.h"

#define FAITH_COMMAND_ID_SIZE  16
#define FAITH_COMMAND_TYPES(X) X(FAITH_COMMAND_CREATE_CONVERSATION, 0)

typedef enum {
#define X(name, value) name = value,
  FAITH_COMMAND_TYPES(X)
#undef X
} faith_command_type_t;

#define FAITH_COMMAND_RESULTS(X)                                               \
  X(FAITH_COMMAND_RESULT_NONE, 0)                                              \
  X(FAITH_COMMAND_RESULT_REJECTED, 1)                                          \
  X(FAITH_COMMAND_RESULT_ACCEPTED, 2)                                          \
  X(FAITH_COMMAND_RESULT_NOT_HANDLED, 3)

typedef enum {
#define X(name, value) name = value,
  FAITH_COMMAND_RESULTS(X)
#undef X
} faith_command_result_t;

#define FAITH_COMMAND_RESULT_ERRS(X)                                           \
  X(FAITH_COMMAND_ERR_NONE, 0)                                                 \
  X(FAITH_COMMAND_ERR_UNAUTHORIZED, 1)                                         \
  X(FAITH_COMMAND_ERR_BAD_COMMAND, 2)                                          \
  X(FAITH_COMMAND_ERR_TIMED_OUT, 3)

typedef enum {
#define X(name, value) name = value,
  FAITH_COMMAND_RESULT_ERRS(X)
#undef X
} faith_command_result_err_t;

typedef struct {
  uint8_t bytes[FAITH_COMMAND_ID_SIZE];
} faith_command_id_t;

#define FAITH_ENVL_CTS_COMMAND_BODY_SIZE_FIXED                                 \
  _FAITH_BODY_SIZE(FAITH_COMMAND_ID_SIZE /* command ID */ +                    \
                   sizeof(uint32_t) /* type */ +                               \
                   sizeof(faith_body_size_t) /* payload size */                \
  )

#define FAITH_ENVL_STC_COMMAND_RESULT_BODY_SIZE                                \
  _FAITH_BODY_SIZE(FAITH_COMMAND_ID_SIZE /* command ID */ +                    \
                   sizeof(uint32_t) /* type */ + sizeof(uint32_t) /* error*/ + \
                   sizeof(uint32_t) /* result */                               \
  )

#define FAITH_COMMAND_PAYLOAD_SIZE_MAX 512

#define FAITH_ENVL_CTS_COMMAND_BODY_SIZE_MAX                                   \
  _FAITH_BODY_SIZE(FAITH_ENVL_CTS_COMMAND_BODY_SIZE_FIXED +                    \
                   FAITH_COMMAND_PAYLOAD_SIZE_MAX)

#define FAITH_CMD_CREATE_CONVERSATION_BODY_SIZE                                \
  _FAITH_BODY_SIZE(FAITH_AUTH_ID_SIZE)

typedef struct {
  /* The client-generated ID of the command*/
  faith_command_id_t cmd_id;

  faith_command_type_t type;

  faith_body_size_t payload_size;
  uint8_t          *payload;
} faith_envl_cts_command_t;

typedef struct {
  /* The ID of the command that is refered to by this result */
  faith_command_id_t cmd_id;

  /* The kind of the command */
  faith_command_type_t type;
  /* The error reason (err = FAITH_COMMAND_ERR_NONE when <result> is
   * FAITH_COMMAND_RESULT_ACCEPTED)*/
  faith_command_result_err_t err;

  /* The result of the command (accepted or rejected)*/
  faith_command_result_t result;
} faith_envl_stc_command_result_t;

typedef struct {
  faith_auth_id_t conversant_id;
} faith_cmd_create_converstation_t;

faith_status_code_t faith_encode_cmd_create_conversation(
    uint8_t *out_buf, faith_body_size_t *out_size, size_t buf_cap_in_bytes,
    const faith_cmd_create_converstation_t *in);

faith_status_code_t
faith_decode_cmd_create_conversation(const uint8_t    *payload,
                                     faith_body_size_t payload_size,
                                     faith_cmd_create_converstation_t *out);

const char *faith_command_type_name(faith_command_type_t type);
const char *faith_command_result_name(faith_command_result_t res);
const char *faith_command_result_err_name(faith_command_result_err_t err);
