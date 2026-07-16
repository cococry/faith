#include "client/client.h"
#include "shared.h"

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

  faith_client_id_t recipient_id = {0};
  faith_msg_request_t msg_request = {0};

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

  bool pending_device_link_request = false;
  bool pending_msg_request = true;

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
        printf("Quitting...\n");
        break;
      }

      if (strcmp(line, "/reconnect") == 0) {
        faith_status_code_t reconnect_rc = faith_client_reconnect(client);

        if (reconnect_rc != FAITH_OK) {
          fprintf(stderr, "Reconnect failed: %s\n",
                  faith_status_code_name(reconnect_rc));
        } else {
          printf("Reconnect requested.\n");
        }

        printf("> ");
        fflush(stdout);
        continue;
      }

      if (pending_device_link_request) {
        if (strcmp(line, "y") == 0 || strcmp(line, "Y") == 0 ||
            strcmp(line, "yes") == 0 || strcmp(line, "YES") == 0) {
          pending_device_link_request = false;

          faith_status_code_t approve_rc =
              faith_client_approve_pending_device_auth(client);

          if (approve_rc != FAITH_OK) {
            fprintf(stderr, "faith_client_approve_device_link() failed: %s\n",
                    faith_status_code_name(approve_rc));
          } else {
            printf("Device link approved.\n");
          }
        } else if (strcmp(line, "n") == 0 || strcmp(line, "N") == 0 ||
                   strcmp(line, "no") == 0 || strcmp(line, "NO") == 0) {
          pending_device_link_request = false;

          faith_status_code_t deny_rc =
              faith_client_deny_pending_device_auth(client);

          if (deny_rc != FAITH_OK) {
            fprintf(stderr, "faith_client_deny_device_link() failed: %s\n",
                    faith_status_code_name(deny_rc));
          } else {
            printf("Device link denied.\n");
          }
        } else {
          printf("Please answer y or n.\n");
          printf("(y)es/n(o) ? ");
          fflush(stdout);
          continue;
        }

        printf("> ");
        fflush(stdout);
        continue;
      }
      if (pending_msg_request) {
        if (strcmp(line, "y") == 0 || strcmp(line, "Y") == 0 ||
            strcmp(line, "yes") == 0 || strcmp(line, "YES") == 0) {
          pending_msg_request = false;

          faith_status_code_t approve_rc =
              faith_client_msg_request_accept(client, &msg_request);

          if (approve_rc != FAITH_OK) {
            fprintf(stderr, "faith_client_msg_request_accept() failed: %s\n",
                    faith_status_code_name(approve_rc));
          } else {
            printf("Message request accepted.\n");
          }
        } else if (strcmp(line, "n") == 0 || strcmp(line, "N") == 0 ||
                   strcmp(line, "no") == 0 || strcmp(line, "NO") == 0) {
          pending_msg_request = false;

          faith_status_code_t deny_rc =
              faith_client_msg_request_deny(client, &msg_request);

          if (deny_rc != FAITH_OK) {
            fprintf(stderr, "faith_client_msg_request_deny() failed: %s\n",
                    faith_status_code_name(deny_rc));
          } else {
            printf("Message request denied.\n");
          }
        } else {
          printf("Please answer y or n.\n");
          printf("(y)es/n(o) ? ");
          fflush(stdout);
          continue;
        }

        printf("> ");
        fflush(stdout);
        continue;
      }

      if (line[0] != '\0') {
        faith_status_code_t send_rc =
            faith_client_send_msg(client, recipient_id, line);

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
          printf("\rMista: %s\n", ev.chat_message);

          if (pending_device_link_request)
            printf("(y)es/n(o) ? ");
          else
            printf("> ");

          fflush(stdout);
          faith_client_free_event(&ev);
          continue;
        }

        if (ev.type == FAITH_EVENT_PONG) {
          faith_client_free_event(&ev);
          continue;
        }

        printf("\rGot event: %s", faith_event_name(ev.type));

        if (strlen(ev.message) != 0)
          printf(" message => %s\n", ev.message);
        else
          printf("\n");

        switch (ev.type) {
        case FAITH_EVENT_DEVICE_LINK_REQUEST:
          pending_device_link_request = true;
          printf("(y)es/n(o) ? ");
          break;

        case FAITH_EVENT_AUTHORIZED: 
          if (argc > 1) {
          faith_status_code_t rc =
              faith_client_send_msg_request(client, recipient_id);
          if (rc == FAITH_OK) {
            printf("Sent message request to %s\n", argv[1]);
          } else {

            printf("Failed to sent message request to %s (Error: %s)\n",
                   argv[1], faith_status_code_name(rc));
          }
        }
          break;
        case FAITH_EVENT_DEVICE_AUTH_RESPONSE_ACK:
        case FAITH_EVENT_DEVICE_LINK_CANCELLED:
          if (pending_device_link_request) {
            pending_device_link_request = false;

            if (ev.type == FAITH_EVENT_DEVICE_AUTH_RESPONSE_ACK) {
              printf("Device-link request was answered by another device.\n");
            } else {
              printf("Device-link request was cancelled because the requesting "
                     "device disconnected.\n");
            }
          }

          printf("> ");
          break;
        case FAITH_EVENT_MSG_REQUEST_RECEIVED: {

          pending_msg_request = true;
          memcpy(msg_request.srv_req_id.bytes, ev.value0_128,
                 FAITH_REQUEST_ID_SIZE);
          memcpy(msg_request.auth_id_req.bytes, ev.value1_128,
                 FAITH_REQUEST_ID_SIZE);

          printf("accept: (y)es/n(o) ? ");
          break;
        }

        default:
          if (pending_device_link_request)
            printf("(y)es/n(o) ? ");
          else
            printf("> ");
          break;
        }

        fflush(stdout);
        faith_client_free_event(&ev);
      }
    }
  }

  faith_client_stop(client);
  faith_client_destroy(client);

  printf("Client stopped.\n");

  return EXIT_SUCCESS;
}
