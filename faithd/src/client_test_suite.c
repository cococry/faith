#include "client/client.h"

#include <errno.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static bool hex_char_to_nibble(char c, uint8_t *out) {
  if (!out)
    return false;

  if (c >= '0' && c <= '9') {
    *out = (uint8_t)(c - '0');
    return true;
  }

  if (c >= 'a' && c <= 'f') {
    *out = (uint8_t)(c - 'a' + 10);
    return true;
  }

  if (c >= 'A' && c <= 'F') {
    *out = (uint8_t)(c - 'A' + 10);
    return true;
  }

  return false;
}

static bool client_id_from_hex(const char *hex, faith_client_id_t *out) {
  if (!hex || !out)
    return false;

  if (strlen(hex) != FAITH_CLIENT_ID_SIZE * 2)
    return false;

  faith_client_id_t id = {0};

  for (size_t i = 0; i < FAITH_CLIENT_ID_SIZE; ++i) {
    uint8_t hi = 0;
    uint8_t lo = 0;

    if (!hex_char_to_nibble(hex[i * 2 + 0], &hi))
      return false;

    if (!hex_char_to_nibble(hex[i * 2 + 1], &lo))
      return false;

    id.bytes[i] = (uint8_t)((hi << 4) | lo);
  }

  *out = id;
  return true;
}

int main(int argc, char **argv) {
  faith_client_init_global(false);

  faith_client_config_t cfg = {
      .insecure_skip_verify = 1,
      .ca_file = NULL,
      .server_name = NULL,
      .host = "127.0.0.1",
      .port = 4433,
  };

  faith_client_t *client = faith_client_create(&cfg);
  if (!client) {
    fprintf(stderr, "faith_client_create() failed.\n");
    return EXIT_FAILURE;
  }

  faith_client_id_t recipient_id = FAITH_CLIENT_ID_NONE;

  if (argc >= 2) {
    if (!client_id_from_hex(argv[1], &recipient_id)) {
      nob_log(ERROR, "Invalid recipient client id: expected 32 hex characters");
      return EXIT_FAILURE;
    }
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
          fprintf(stderr, "faith_client_send_message() failed: %s\n",
                  faith_status_code_name(send_rc));
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
        } else {
          if(strlen(ev.message) != 0) {
            printf("GOT MESSAGE: %s\n", ev.message);
          }
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
