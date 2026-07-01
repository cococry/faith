#include "client/client.h"
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>

int main(int argc, char **argv) {

  if (argc < 3) {
    fprintf(stderr, "usage: %s [client-id] [device-id]\n", argv[0]);
    return EXIT_FAILURE;
  }

  uint16_t client_id = (uint16_t)atoi(argv[1]);
  uint16_t device_id = (uint16_t)atoi(argv[2]);

  faith_client_init_global();

  faith_client_config_t cfg = {
      .insecure_skip_verify = 1,
      .ca_file = NULL,
      .server_name = NULL,
      .host = "127.0.0.1",
      .port = 4433,
      .client_id = client_id,
      .device_id = device_id,
  };

  faith_client_t *client = faith_client_create(&cfg);
  if (!client) {
    nob_log(ERROR, "faith_client_create() failed.\n");
    return 1;
  }

  faith_client_start(client);

  int event_fd = faith_client_event_fd(client);

  for (;;) {
    struct pollfd pfd = {
        .fd = event_fd,
        .events = POLLIN,
    };

    if (poll(&pfd, 1, -1) < 0) {
      perror("poll");
      break;
    }

    uint64_t count;
    read(event_fd, &count, sizeof(count));

    faith_event_t ev;
    while (faith_client_next_event(client, &ev) == FAITH_OK) {
      nob_log(INFO, "Got event type=%s value0=%llu value1=%llu message=%s\n",
              faith_event_name(ev.type), (unsigned long long)ev.value0,
              (unsigned long long)ev.value1, ev.message);
    }
  }

  faith_client_stop(client);
  faith_client_destroy(client);
  return EXIT_SUCCESS;
}
