#include <stdio.h>

#define NOBDEF static inline
#define NOB_IMPLEMENTATION
#define NOB_EXPERIMENTAL_DELETE_OLD
#define NOB_WARN_DEPRECATED
#define NOB_STRIP_PREFIX
#include "../../third_party/third_party/nob.h"

#define command(arg, commands, name, signature, description)                   \
  command_loc(__FILE__, __LINE__, (arg), (commands), (name), (signature),      \
              (description))

#define BUILD_FOLDER "./build/"
#define OBJECT_FOLDER BUILD_FOLDER "obj/"

#define TEST_SOURCE "tui.c"
#define TEST_OBJECT OBJECT_FOLDER "tui.o"
#define TEST_EXECUTABLE BUILD_FOLDER "faith-tui"

#define FAITH_CLIENT_INCLUDE "../include"
#define FAITH_CLIENT_HEADER                                                   \
  FAITH_CLIENT_INCLUDE "/faith-client/client.h"
#define FAITH_CLIENT_LIBRARY "../build/libfaith_client.a"

#define FAITH_PROTO_INCLUDE "../../faith-proto/include"
#define FAITH_PROTO_HEADER                                                    \
  FAITH_PROTO_INCLUDE "/faith-proto/codec/codec.h"
#define FAITH_PROTO_LIBRARY "../../faith-proto/build/libfaith_proto.a"

typedef struct {
  const char *name;
  const char *signature;
  const char *description;
} Command;

typedef struct {
  Command *items;
  size_t   count;
  size_t   capacity;

  bool        picked;
  const char *picked_name;
  const char *picked_at_file;
  int         picked_at_line;
} Commands;

void commands_reset(Commands *commands) {
  commands->count = 0;
  commands->picked = false;
}

void print_available_commands(Commands commands) {
  size_t max_name_width = 0;
  size_t max_sign_width = 0;

  da_foreach(Command, command, &commands) {
    size_t name_width = strlen(command->name);
    size_t sign_width = strlen(command->signature);

    if (name_width > max_name_width)
      max_name_width = name_width;

    if (sign_width > max_sign_width)
      max_sign_width = sign_width;
  }

  nob_log(INFO, "Available commands:");

  da_foreach(Command, command, &commands) {
    nob_log(INFO, "    %-*s %-*s - %s", (int)max_name_width, command->name,
            (int)max_sign_width, command->signature, command->description);
  }
}

bool command_loc(const char *file, int line, const char *arg,
                 Commands *commands, const char *name, const char *signature,
                 const char *description) {
  if (commands->picked) {
    fprintf(
        stderr,
        "%s:%d: ASSERTION FAILED: the branch for command `%s` fell through.\n",
        commands->picked_at_file, commands->picked_at_line,
        commands->picked_name);

    fprintf(stderr,
            "%s:%d: NOTE: the execution proceeded to here, but the command was "
            "already picked.\n",
            file, line);

    abort();
  }

  Command command = {
      .name = name,
      .signature = signature,
      .description = description,
  };

  da_append(commands, command);

  commands->picked_name = name;
  commands->picked_at_line = line;
  commands->picked_at_file = file;
  commands->picked = strcmp(arg, name) == 0;

  return commands->picked;
}

static bool required_files_exist(void) {
  if (!nob_file_exists(TEST_SOURCE)) {
    nob_log(ERROR, "Test source is missing: %s", TEST_SOURCE);
    return false;
  }

  if (!nob_file_exists(FAITH_CLIENT_HEADER)) {
    nob_log(ERROR, "Client header is missing: %s", FAITH_CLIENT_HEADER);
    return false;
  }

  if (!nob_file_exists(FAITH_CLIENT_LIBRARY)) {
    nob_log(ERROR, "Client library is missing: %s", FAITH_CLIENT_LIBRARY);
    return false;
  }

  if (!nob_file_exists(FAITH_PROTO_HEADER)) {
    nob_log(ERROR, "Protocol header is missing: %s", FAITH_PROTO_HEADER);
    return false;
  }

  if (!nob_file_exists(FAITH_PROTO_LIBRARY)) {
    nob_log(ERROR, "Protocol library is missing: %s", FAITH_PROTO_LIBRARY);
    return false;
  }

  return true;
}

static bool compile_test(void) {
  Cmd cmd = {0};

  nob_cc(&cmd);
  nob_cc_flags(&cmd);

  nob_cmd_append(&cmd, "-I../../third_party/", "-I" FAITH_CLIENT_INCLUDE,
                 "-I" FAITH_PROTO_INCLUDE, "-c", TEST_SOURCE, "-o",
                 TEST_OBJECT);

  bool result = nob_cmd_run(&cmd);
  nob_cmd_free(cmd);

  return result;
}

static bool link_test(void) {
  Cmd cmd = {0};

  nob_cc(&cmd);
  nob_cc_output(&cmd, TEST_EXECUTABLE);

  nob_cmd_append(&cmd, TEST_OBJECT, FAITH_CLIENT_LIBRARY, FAITH_PROTO_LIBRARY,
                 "-lssl", "-lcrypto", "-lpthread");

  bool result = nob_cmd_run(&cmd);
  nob_cmd_free(cmd);

  return result;
}

static bool build(void) {
  if (!required_files_exist())
    return false;

  if (!nob_mkdir_if_not_exists(BUILD_FOLDER))
    return false;

  if (!nob_mkdir_if_not_exists(OBJECT_FOLDER))
    return false;

  if (!compile_test())
    return false;

  if (!link_test())
    return false;

  nob_log(INFO, "Built %s", TEST_EXECUTABLE);

  return true;
}

int main(int argc, char **argv) {
#ifdef _WIN32
  SetConsoleOutputCP(CP_UTF8);
#endif

  GO_REBUILD_URSELF_PLUS(argc, argv, "../../third_party/third_party/nob.h");

  const char *program_name = shift(argv, argc);
  const char *command_name = "build";

  (void)program_name;

  if (argc > 0)
    command_name = shift(argv, argc);

  Commands commands = {0};

  commands_reset(&commands);

  if (command(command_name, &commands, "build", "",
              "Build the native client test executable")) {
    bool result = build();
    nob_da_free(commands);
    return result ? 0 : 1;
  }

  if (command(command_name, &commands, "help", "", "Print this help message")) {
    print_available_commands(commands);
    nob_da_free(commands);
    return 0;
  }

  print_available_commands(commands);
  nob_log(ERROR, "Unknown command %s", command_name);

  nob_da_free(commands);

  return 1;
}
