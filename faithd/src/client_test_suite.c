#include "client/client.h"

#include <errno.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
  if (argc < 4) {
    fprintf(stderr, "usage: %s [auth-id] [device-id] [who-to-talk-to]\n",
            argv[0]);
    return EXIT_FAILURE;
  }

  uint16_t auth_id = (uint16_t)atoi(argv[1]);
  uint16_t device_id = (uint16_t)atoi(argv[2]);
  uint16_t recipient_id = (uint16_t)atoi(argv[3]);

  faith_client_init_global(false);

  faith_client_config_t cfg = {
      .insecure_skip_verify = 1,
      .ca_file = NULL,
      .server_name = NULL,
      .host = "127.0.0.1",
      .port = 4433,
      .auth_id = auth_id,
      .device_id = device_id,
  };

  faith_client_t *client = faith_client_create(&cfg);
  if (!client) {
    fprintf(stderr, "faith_client_create() failed.\n");
    return EXIT_FAILURE;
  }

  if (faith_client_start(client) != FAITH_OK) {
    fprintf(stderr, "faith_client_start() failed.\n");
    faith_client_destroy(client);
    return EXIT_FAILURE;
  }

  int event_fd = faith_client_event_fd(client);
  if (event_fd < 0) {
    fprintf(stderr, "faith_client_event_fd() failed.\n");
    faith_client_destroy(client);
    return EXIT_FAILURE;
  }

  printf("Type messages and press Enter. Type /quit to exit.\n");

  printf("> ");
  fflush(stdout);
  for (;;) {
    struct pollfd pfds[2] = {
        {
            .fd = STDIN_FILENO,
            .events = POLLIN,
        },
        {
            .fd = event_fd,
            .events = POLLIN,
        },
    };

    int rc = poll(pfds, 2, -1);
    if (rc < 0) {
      if (errno == EINTR)
        continue;

      perror("poll");
      break;
    }

    if (pfds[0].revents & POLLIN) {
      char line[1024];

      if (fgets(line, sizeof(line), stdin) == NULL) {
        printf("stdin closed\n");
        break;
      }

      line[strcspn(line, "\n")] = '\0';

      if (strcmp(line, "/quit") == 0) {
        break;
      }

      if (line[0] != '\0') {
        faith_status_code_t send_rc =
            faith_client_send_message(client, recipient_id, line);

        if (send_rc != FAITH_OK) {
          fprintf(stderr, "faith_client_send_message() failed: %d\n", send_rc);
        }
        printf("You: %s\n", line);
      }
      printf("> ");
      fflush(stdout);
    }

    if (pfds[1].revents & POLLIN) {
      uint64_t count;

      ssize_t n = read(event_fd, &count, sizeof(count));
      if (n < 0) {
        if (errno != EAGAIN && errno != EWOULDBLOCK) {
          perror("read event_fd");
          break;
        }
      }

      faith_event_t ev;
      while (faith_client_next_event(client, &ev) == FAITH_OK) {
        if (ev.type == FAITH_EVENT_MESSAGE_RECEIVED) {
          printf("Mista: %s\n", ev.chat_message);
          printf("> ");
          fflush(stdout);
        }
      }
      faith_client_free_event(&ev);
    }

    if (pfds[0].revents & (POLLERR | POLLHUP | POLLNVAL)) {
      fprintf(stderr, "stdin poll error/hangup\n");
      break;
    }

    if (pfds[1].revents & (POLLERR | POLLHUP | POLLNVAL)) {
      fprintf(stderr, "event_fd poll error/hangup\n");
      break;
    }
  }

  faith_client_stop(client);
  faith_client_destroy(client);

  return EXIT_SUCCESS;
}
