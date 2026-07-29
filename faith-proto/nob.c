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

#define BUILD_FOLDER  "./build/"
#define OBJECT_FOLDER BUILD_FOLDER "obj/"
#define OUTPUT_LIBRARY BUILD_FOLDER "libfaith_proto.a"

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

static const char *faith_proto_sources[] = {
    "src/codec/auth.c",
    "src/codec/codec.c",
    "src/codec/commands.c",
    "src/codec/envelopes.c",
    "src/codec/events.c",
    "src/codec/helpers.c",
    "src/codec/msg.c",
    "src/codec/signatures.c",

    "src/core/core.c",
    "src/core/crypto.c",
};

static const char *faith_proto_objects[] = {
    OBJECT_FOLDER "codec_auth.o",
    OBJECT_FOLDER "codec_codec.o",
    OBJECT_FOLDER "codec_commands.o",
    OBJECT_FOLDER "codec_envelopes.o",
    OBJECT_FOLDER "codec_events.o",
    OBJECT_FOLDER "codec_helpers.o",
    OBJECT_FOLDER "codec_msg.o",
    OBJECT_FOLDER "codec_signatures.o",

    OBJECT_FOLDER "core_core.o",
    OBJECT_FOLDER "core_crypto.o",
};

static void commands_reset(Commands *commands) {
  commands->count = 0;
  commands->picked = false;
}

static void print_available_commands(Commands commands) {
  size_t max_name_width = 0;
  size_t max_sign_width = 0;

  da_foreach(Command, command, &commands) {
    const size_t name_width = strlen(command->name);
    const size_t sign_width = strlen(command->signature);

    if (name_width > max_name_width)
      max_name_width = name_width;

    if (sign_width > max_sign_width)
      max_sign_width = sign_width;
  }

  nob_log(INFO, "Available commands:");

  da_foreach(Command, command, &commands) {
    nob_log(INFO,
            "    %-*s %-*s - %s",
            (int)max_name_width,
            command->name,
            (int)max_sign_width,
            command->signature,
            command->description);
  }
}

static bool command_loc(const char *file,
                        int line,
                        const char *arg,
                        Commands *commands,
                        const char *name,
                        const char *signature,
                        const char *description) {
  if (commands->picked) {
    fprintf(stderr,
            "%s:%d: ASSERTION FAILED: the branch for command `%s` fell "
            "through.\n",
            commands->picked_at_file,
            commands->picked_at_line,
            commands->picked_name);

    fprintf(stderr,
            "%s:%d: NOTE: execution proceeded here after a command had "
            "already been picked.\n",
            file,
            line);

    abort();
  }

  const Command cmd = {
      .name = name,
      .signature = signature,
      .description = description,
  };

  da_append(commands, cmd);

  commands->picked_name = name;
  commands->picked_at_line = line;
  commands->picked_at_file = file;
  commands->picked = strcmp(arg, name) == 0;

  return commands->picked;
}

static bool compile_source_async(Procs *procs,
                                 const char *source,
                                 const char *object) {
  Cmd cmd = {0};

  nob_cc(&cmd);
  nob_cc_flags(&cmd);

#ifndef _WIN32
  nob_cmd_append(&cmd, "-fPIC");
#endif

  nob_cmd_append(&cmd,
                 "-I.",
                 "-Iinclude/faith-proto/",
                 "-I../third_party/",
                 "-c",
                 source,
                 "-o",
                 object);

  if (!nob_cmd_run(&cmd, .async = procs, .max_procs = 0)) {
    nob_cmd_free(cmd);
    return false;
  }

  nob_cmd_free(cmd);
  return true;
}

static bool archive_static_library(void) {
  Cmd cmd = {0};
  bool result = false;

  if (nob_file_exists(OUTPUT_LIBRARY)) {
    if (!nob_delete_file(OUTPUT_LIBRARY)) {
      nob_log(ERROR, "Could not remove old library %s", OUTPUT_LIBRARY);
      goto cleanup;
    }
  }

  nob_cmd_append(&cmd, "ar", "rcs", OUTPUT_LIBRARY);

  for (size_t i = 0; i < NOB_ARRAY_LEN(faith_proto_objects); ++i)
    nob_cmd_append(&cmd, faith_proto_objects[i]);

  if (!nob_cmd_run(&cmd))
    goto cleanup;

  nob_log(INFO, "Built %s", OUTPUT_LIBRARY);
  result = true;

cleanup:
  nob_cmd_free(cmd);
  return result;
}

static bool build(void) {
  Procs procs = {0};
  bool result = false;

  _Static_assert(
      NOB_ARRAY_LEN(faith_proto_sources) ==
          NOB_ARRAY_LEN(faith_proto_objects),
      "faith-proto source/object count mismatch");

  if (!nob_mkdir_if_not_exists(BUILD_FOLDER))
    goto cleanup;

  if (!nob_mkdir_if_not_exists(OBJECT_FOLDER))
    goto cleanup;

  for (size_t i = 0; i < NOB_ARRAY_LEN(faith_proto_sources); ++i) {
    if (!compile_source_async(
            &procs,
            faith_proto_sources[i],
            faith_proto_objects[i])) {
      goto cleanup;
    }
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

  const char *program_name = shift(argv, argc);
  (void)program_name;

  const char *command_name = "build";

  if (argc > 0)
    command_name = shift(argv, argc);

  Commands commands = {0};
  commands_reset(&commands);

  if (command(command_name,
              &commands,
              "build",
              "",
              "Build the static faith protocol library")) {
    const bool ok = build();
    nob_da_free(commands);
    return ok ? 0 : 1;
  }

  if (command(command_name,
              &commands,
              "help",
              "",
              "Print this help message")) {
    print_available_commands(commands);
    nob_da_free(commands);
    return 0;
  }

  print_available_commands(commands);
  nob_log(ERROR, "Unknown command %s", command_name);

  nob_da_free(commands);
  return 1;
}
