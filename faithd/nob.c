#include <stdio.h>
#define NOBDEF static inline
#define NOB_IMPLEMENTATION
#define NOB_EXPERIMENTAL_DELETE_OLD
#define NOB_WARN_DEPRECATED
#define NOB_STRIP_PREFIX
#include "third_party/nob.h"

#define command(arg, commands, name, signature, description)                   \
  command_loc(__FILE__, __LINE__, (arg), (commands), (name), (signature),      \
              (description))

#define BUILD_FOLDER "./build/"

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
  commands->picked = (strcmp(arg, name) == 0);
  return commands->picked;
}

bool build() {
  Cmd cmd = {0};

  nob_cc(&cmd);
  nob_cc_flags(&cmd);
  nob_cc_output(&cmd, BUILD_FOLDER "faithd");
  nob_cmd_append(&cmd, "-lssl", "-lcrypto");
  nob_cc_inputs(&cmd, "src/faithd.c", "src/protocol.c", "src/transport/tls.c",
                "src/transport/conn.c", "src/reactor/reactor.c",
                "src/transport/frame.c", "src/logging/logging.c");

  if (!cmd_run(&cmd))
    return false;

  nob_cc(&cmd);
  nob_cc_flags(&cmd);
  nob_cc_output(&cmd, BUILD_FOLDER "test_suite");
  nob_cmd_append(&cmd, "-lssl", "-lcrypto");
  nob_cc_inputs(&cmd, "src/client_test_suite.c", "src/protocol.c",
                "src/client/client.c");

  if (!cmd_run(&cmd))
    return false;

  return true;
}

int main(int argc, char **argv) {
#ifdef _WIN32
  SetConsoleOutputCP(CP_UTF8);
#endif // _WIN32

  GO_REBUILD_URSELF_PLUS(argc, argv, "third_party/nob.h");

  set_log_handler(cancer_log_handler);

  const char *program_name = shift(argv, argc);
  const char *command_name = "build";
  if (argc > 0)
    command_name = shift(argv, argc);

  Commands commands = {0};

  commands_reset(&commands);

  if (command(command_name, &commands, "build", "[test_names...]",
              "Run the tests checking their expected output")) {
    if (!mkdir_if_not_exists(BUILD_FOLDER))
      return 1;

    if (!build())
      return 1;

    return 0;
  }
  if (command(command_name, &commands, "chain", "",
              "Generate certificate files \"chain.pem\" and \"pkey.pem\"")) {
    // generate pkey.pem
    {
      Nob_Cmd cmd = {0};
      cmd_append(&cmd, "openssl", "genpkey", "-algorithm", "rsa", "-out",
                 "pkey.pem", "-pkeyopt", "rsa_keygen_bits:2048");
      cmd_run(&cmd);
    }
    // generate chain.pem
    {
      Nob_Cmd cmd = {0};
      cmd_append(&cmd, "openssl", "req", "-x509", "-new", "-key", "pkey.pem",
                 "-days", "36500", "-subj", "/CN=localhost", "-out",
                 "chain.pem");
      cmd_run(&cmd);
    }

    return 0;
  }
  if (command(command_name, &commands, "help", "", "Print this help message")) {
    print_available_commands(commands);
    return 0;
  }

  print_available_commands(commands);
  nob_log(ERROR, "Unknown command %s", command_name);
  return 1;
}
