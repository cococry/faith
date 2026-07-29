#include <stdio.h>

#define NOBDEF static inline
#define NOB_IMPLEMENTATION
#define NOB_EXPERIMENTAL_DELETE_OLD
#define NOB_WARN_DEPRECATED
#define NOB_STRIP_PREFIX
#include "../third_party/third_party/nob.h"

#define command(arg, commands, name, signature, description)                   \
  command_loc(__FILE__, __LINE__, (arg), (commands), (name), (signature),      \
              (description))

#define BUILD_FOLDER "./build/"
#define OBJECT_FOLDER BUILD_FOLDER "obj/"
#define OUTPUT_LIBRARY BUILD_FOLDER "libfaith_client.a"

#define FAITH_PROTO_ROOT "../faith-proto/"
#define FAITH_PROTO_INCLUDE FAITH_PROTO_ROOT "include"
#define FAITH_PROTO_LIBRARY FAITH_PROTO_ROOT "build/libfaith_proto.a"
#define FAITH_PROTO_HEADER FAITH_PROTO_INCLUDE "/faith-proto/codec/codec.h"

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

static const char *faith_client_sources[] = {
    "src/client.c",
};

static const char *faith_client_objects[] = {
    OBJECT_FOLDER "client.o",
};

static bool compile_source_async(Procs *procs, const char *source,
                                 const char *object) {
  Cmd cmd = {0};

  nob_cc(&cmd);
  nob_cc_flags(&cmd);

#ifndef _WIN32
  nob_cmd_append(&cmd, "-fPIC");
#endif

  nob_cmd_append(&cmd, "-Iinclude", "-I../third_party/", "-I"FAITH_PROTO_INCLUDE, "-c", source,
                 "-o", object);

  if (!nob_cmd_run(&cmd, .async = procs, .max_procs = 0)) {
    nob_cmd_free(cmd);
    return false;
  }

  nob_cmd_free(cmd);
  return true;
}

static bool archive_static_library(void) {
  Cmd  cmd = {0};
  bool result = false;

  if (nob_file_exists(OUTPUT_LIBRARY)) {
    if (!nob_delete_file(OUTPUT_LIBRARY)) {
      nob_log(ERROR, "Could not remove old library: %s", OUTPUT_LIBRARY);
      goto cleanup;
    }
  }

  nob_cmd_append(&cmd, "ar", "rcs", OUTPUT_LIBRARY);

  for (size_t i = 0; i < NOB_ARRAY_LEN(faith_client_objects); ++i)
    nob_cmd_append(&cmd, faith_client_objects[i]);

  if (!nob_cmd_run(&cmd))
    goto cleanup;

  nob_log(INFO, "Built %s", OUTPUT_LIBRARY);
  result = true;

cleanup:
  nob_cmd_free(cmd);
  return result;
}

bool build(void) {
  Procs procs = {0};
  bool  result = false;

  _Static_assert(
      NOB_ARRAY_LEN(faith_client_sources) ==
          NOB_ARRAY_LEN(faith_client_objects),
      "faith-client source/object count mismatch");

  if (!nob_file_exists(FAITH_PROTO_LIBRARY)) {
    nob_log(ERROR, "Required library is missing: %s", FAITH_PROTO_LIBRARY);
    goto cleanup;
  }

  if (!nob_file_exists(FAITH_PROTO_HEADER)) {
    nob_log(ERROR, "Required protocol header is missing: %s",
            FAITH_PROTO_HEADER);
    goto cleanup;
  }

  if (!nob_file_exists("include/faith-client/client.h")) {
    nob_log(ERROR, "Required client header is missing: %s",
            "include/faith-client/client.h");
    goto cleanup;
  }

  if (!nob_mkdir_if_not_exists(BUILD_FOLDER))
    goto cleanup;

  if (!nob_mkdir_if_not_exists(OBJECT_FOLDER))
    goto cleanup;

  for (size_t i = 0; i < NOB_ARRAY_LEN(faith_client_sources); ++i) {
    if (!compile_source_async(&procs, faith_client_sources[i],
                              faith_client_objects[i]))
      goto cleanup;
  }

  if (!nob_procs_flush(&procs))
    goto cleanup;

  if (!archive_static_library())
    goto cleanup;

  result = true;

cleanup:
  if (procs.count > 0 && !nob_procs_flush(&procs))
    result = false;

  nob_da_free(procs);

  return result;
}


int main(int argc, char **argv) {
#ifdef _WIN32
  SetConsoleOutputCP(CP_UTF8);
#endif

  GO_REBUILD_URSELF_PLUS(argc, argv, "../third_party/third_party/nob.h");

  set_log_handler(cancer_log_handler);

  const char *program_name = shift(argv, argc);
  const char *command_name = "build";

  (void)program_name;

  if (argc > 0)
    command_name = shift(argv, argc);

  Commands commands = {0};

  commands_reset(&commands);

  if (command(command_name, &commands, "build", "",
              "Build the native Faith client library")) {
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
