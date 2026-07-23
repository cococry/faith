#include "server/server.h"

#include "logging/logging.h"
#include <stdlib.h>

enum { OPTIONS_VALID, UNKOWN_OPTION, JUST_PRINT_PRINT_HELP };

static void print_usage(const char *prog) {
  printf("Usage: %s [options]\n"
         "\n"
         "Options:\n"
         "  -v, --verbose    Enable verbose logging\n"
         "  -h, --help       Show this help message\n",
         prog);
}

static int parse_args(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    const char *arg = argv[i];

    if (strcmp(arg, "-v") == 0 || strcmp(arg, "--verbose") == 0) {
      g_verbose_logging = 1;
      continue;
    }

    if (strcmp(arg, "-h") == 0 || strcmp(arg, "--help") == 0) {
      print_usage(argv[0]);
      return JUST_PRINT_PRINT_HELP;
    }

    fprintf(stderr, "Unknown option: %s\n\n", arg);
    print_usage(argv[0]);

    return UNKOWN_OPTION;
  }

  return OPTIONS_VALID;
}

int main(int argc, char **argv) {

  int rc = parse_args(argc, argv);
  if (rc != OPTIONS_VALID) {
    return rc == JUST_PRINT_PRINT_HELP ? EXIT_SUCCESS : EXIT_FAILURE;
  }

  server_state_t  s = {0};
  server_config_t cfg = {.listen_port = 4433};

  if (server_init(&s, &cfg) != FAITH_OK) {
    nob_log(INFO, "Server initialization failed.");
    return EXIT_FAILURE;
  }

  if (server_loop(&s) != FAITH_OK) {
    nob_log(INFO, "Server loop failed.");
    return EXIT_FAILURE;
  }

  if (server_destroy(&s) != FAITH_OK) {
    nob_log(INFO, "Server de-initialization failed.");
    return EXIT_FAILURE;
  }

  return EXIT_SUCCESS;
}
