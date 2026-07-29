
#include "server/server.h"

#include "logging/logging.h"
#include <stdlib.h>

enum { OPTIONS_NORMAL, OPTIONS_INVALID, JUST_PRINT_PRINT_HELP };

static int listen_port = 4433;

static void print_usage(const char *prog) {
  printf("Usage: %s [options]\n"
         "\n"
         "Options:\n"
         "  -v, --verbose       Enable verbose logging\n"
         "  -p, --port [port]   Specify the port to listen on\n"
         "  -h, --help          Show this help message\n",
         prog);
}

static int parse_args(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    const char *arg = argv[i];

    if (strcmp(arg, "-v") == 0 || strcmp(arg, "--verbose") == 0) {
      g_verbose_logging = 1;
      continue;
    }

    if (strcmp(arg, "-p") == 0 || strcmp(arg, "--pport") == 0) {
      if (i + 1 > argc - 1) {
        fprintf(stderr, "No port number specified for: %s\n", arg);
        fprintf(stderr, "Usage: -p, --port [port]\n");
        return OPTIONS_INVALID;
      }
      listen_port = atoi(argv[i + 1]);
      i++;
      continue;
    }

    if (strcmp(arg, "-h") == 0 || strcmp(arg, "--help") == 0) {
      print_usage(argv[0]);
      return JUST_PRINT_PRINT_HELP;
    }

    fprintf(stderr, "Unknown option: %s\n\n", arg);
    print_usage(argv[0]);

    return OPTIONS_INVALID;
  }

  return OPTIONS_NORMAL;
}

int main(int argc, char **argv) {

  int options = parse_args(argc, argv);

  if (options != OPTIONS_NORMAL) {
    return options == JUST_PRINT_PRINT_HELP ? EXIT_SUCCESS : EXIT_FAILURE;
  }

  server_state_t  s = {0};
  server_config_t cfg = {.listen_port = listen_port};

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
