#include <ctype.h>
#include <errno.h>
#include <faith-client/client.h>
#include <faith-proto/codec/commands.h>
#include <faith-proto/codec/events.h>
#include <faith-proto/core/crypto.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <pthread.h>
#include <sqlite3.h>
#include <sys/eventfd.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <unistd.h>

typedef struct {
  sqlite3        *db;
  pthread_mutex_t lock;
} local_store_t;

static bool parse_id(const char *text, uint8_t id[16]) {
  if (!text || strlen(text) != 32)
    return false;

  for (size_t i = 0; i < 16; i++) {
    char  pair[3] = {text[i * 2], text[i * 2 + 1], 0};
    char *end;
    long  value = strtol(pair, &end, 16);

    if (*end || value < 0 || value > 255 || !isxdigit((unsigned char)pair[0]) ||
        !isxdigit((unsigned char)pair[1]))
      return false;

    id[i] = value;
  }

  return true;
}

static void print_id(const uint8_t id[16]) {
  char hex[33];
  faith_id128_to_hex(id, hex);
  printf("%s", hex);
}

static bool execute(sqlite3 *db, const char *sql) {
  return sqlite3_exec(db, sql, NULL, NULL, NULL) == SQLITE_OK;
}

static sqlite3_stmt *prepare(sqlite3 *db, const char *sql) {
  sqlite3_stmt *st = NULL;

  if (sqlite3_prepare_v2(db, sql, -1, &st, NULL) != SQLITE_OK)
    return NULL;

  return st;
}

static void blob(sqlite3_stmt *st, int n, const void *data, int size) {
  sqlite3_bind_blob(st, n, data, size, SQLITE_TRANSIENT);
}

static bool member(const faith_group_packet_t *p, const uint8_t id[16]) {
  for (size_t i = 0; i < p->n_members; i++)
    if (!memcmp(p->members[i].bytes, id, 16))
      return true;

  return false;
}

static bool load_group(sqlite3 *db, const uint8_t id[16],
                       faith_group_packet_t *p, bool *active) {
  sqlite3_stmt *st = prepare(
      db, "SELECT revision,members,active FROM conversations WHERE id=?");

  if (!st)
    return false;

  blob(st, 1, id, 16);
  bool found = sqlite3_step(st) == SQLITE_ROW;

  if (found) {
    memset(p, 0, sizeof(*p));
    p->format = FAITH_GROUP_PLAINTEXT_V1;
    p->revision = sqlite3_column_int64(st, 0);
    int size = sqlite3_column_bytes(st, 1);
    found = size >= 16 && size <= (int)(16 * FAITH_GROUP_MAX_MEMBERS) &&
            size % 16 == 0;

    if (found) {
      p->n_members = size / 16;
      memcpy(p->members, sqlite3_column_blob(st, 1), size);
      memcpy(p->conversation_id, id, 16);
      *active = sqlite3_column_int(st, 2);
    }
  }

  sqlite3_finalize(st);

  return found;
}

static faith_status_code_t store_event(void                    *context,
                                       const faith_auth_id_t   *auth,
                                       const faith_device_id_t *device,
                                       uint64_t seq, uint32_t type,
                                       const uint8_t *data, size_t size) {
  local_store_t *store = context;
  sqlite3       *db = store->db;
  pthread_mutex_lock(&store->lock);
  bool          ok = execute(db, "BEGIN IMMEDIATE");
  sqlite3_stmt *st = NULL;

  if (!ok)
    goto done;

  st = prepare(db, "SELECT seq FROM cursors WHERE device=?");

  if (!st) {
    ok = false;
    goto finish;
  }

  blob(st, 1, device->bytes, 16);
  bool duplicate = sqlite3_step(st) == SQLITE_ROW &&
                   seq <= (uint64_t)sqlite3_column_int64(st, 0);

  sqlite3_finalize(st);
  st = NULL;

  if (duplicate)
    goto finish;

  if (type == FAITH_EVENT_GROUP_PACKET && size >= 16) {
    faith_group_packet_t p, old;
    bool                 active = false;

    if (faith_group_decode(data + 16, size - 16, &p) != FAITH_OK) {
      ok = false;
      goto finish;
    }

    bool known = load_group(db, p.conversation_id, &old, &active);
    bool accepted = false;

    if (p.kind == FAITH_GROUP_MESSAGE) {
      accepted = known && active && p.revision == old.revision &&
                 p.n_members == old.n_members &&
                 !memcmp(p.members, old.members, p.n_members * 16) &&
                 member(&old, data) && member(&old, auth->bytes);

      if (accepted) {
        st = prepare(db,
                     "INSERT OR IGNORE INTO "
                     "messages(conversation,sender,id,body) VALUES(?,?,?,?)");

        if (!st) {
          ok = false;
          goto finish;
        }

        blob(st, 1, p.conversation_id, 16);
        blob(st, 2, data, 16);
        blob(st, 3, p.message_id, 16);
        sqlite3_bind_text(st, 4, (const char *)p.text, p.text_size,
                          SQLITE_TRANSIENT);
      }
    } else {
      accepted = !memcmp(data, p.members[0].bytes, 16) &&
                 ((!known && member(&p, auth->bytes)) ||
                  (known && !memcmp(old.members[0].bytes, data, 16) &&
                   p.revision > old.revision));

      if (accepted) {
        st =
            prepare(db, "INSERT INTO conversations(id,revision,members,active) "
                        "VALUES(?,?,?,?) "
                        "ON CONFLICT(id) DO UPDATE SET "
                        "revision=excluded.revision,members=excluded.members,"
                        "active=excluded.active");

        if (!st) {
          ok = false;
          goto finish;
        }

        blob(st, 1, p.conversation_id, 16);
        sqlite3_bind_int64(st, 2, p.revision);
        blob(st, 3, p.members, p.n_members * 16);
        sqlite3_bind_int(st, 4, member(&p, auth->bytes));
      }
    }

    if (st) {
      ok = sqlite3_step(st) == SQLITE_DONE;
      sqlite3_finalize(st);
      st = NULL;
    }

    if (!accepted)
      fprintf(
          stderr,
          "Ignored duplicate or unauthorized plaintext conversation packet.\n");
  } else if (type != FAITH_EVENT_CONVERSATION_CREATED) {
    ok = false;
    goto finish;
  }

  if (ok) {
    st = prepare(db, "INSERT INTO cursors(device,seq) VALUES(?,?) ON "
                     "CONFLICT(device) DO UPDATE SET seq=excluded.seq");

    if (!st) {
      ok = false;
      goto finish;
    }

    blob(st, 1, device->bytes, 16);
    sqlite3_bind_int64(st, 2, seq);
    ok = sqlite3_step(st) == SQLITE_DONE;
    sqlite3_finalize(st);
    st = NULL;
  }

finish:
  sqlite3_finalize(st);

  if (ok)
    ok = execute(db, "COMMIT");

  if (!ok)
    execute(db, "ROLLBACK");

done:
  pthread_mutex_unlock(&store->lock);

  return ok ? FAITH_OK : FAITH_ERR_IO;
}

static void show_store(local_store_t *store, const uint8_t *conversation) {
  pthread_mutex_lock(&store->lock);
  sqlite3_stmt *st = prepare(
      store->db,
      conversation
          ? "SELECT hex(sender),body FROM messages WHERE conversation=? ORDER "
            "BY rowid"
          : "SELECT hex(id),revision,active FROM conversations ORDER BY rowid");

  if (st) {
    if (conversation)
      blob(st, 1, conversation, 16);

    while (sqlite3_step(st) == SQLITE_ROW) {
      printf("%s | ", sqlite3_column_text(st, 0));

      if (conversation) {
        const unsigned char *text = sqlite3_column_text(st, 1);

        for (int i = 0; i < sqlite3_column_bytes(st, 1); i++) {
          unsigned char c = text[i];

          if (c >= 32 && c != 127)
            putchar(c);
        }

        putchar('\n');
      } else
        printf("revision=%lld %s\n", sqlite3_column_int64(st, 1),
               sqlite3_column_int(st, 2) ? "active" : "removed");
    }

    sqlite3_finalize(st);
  }

  pthread_mutex_unlock(&store->lock);
}

static void show_message(local_store_t *store, const faith_event_t *event) {
  if (event->chat_message_size < 16)
    return;

  faith_group_packet_t packet;

  if (faith_group_decode((uint8_t *)event->chat_message + 16,
                         event->chat_message_size - 16, &packet) != FAITH_OK)
    return;

  pthread_mutex_lock(&store->lock);
  sqlite3_stmt *st = prepare(
      store->db,
      "SELECT body FROM messages WHERE conversation=? AND sender=? AND id=?");

  if (st) {
    blob(st, 1, packet.conversation_id, 16);
    blob(st, 2, event->chat_message, 16);
    blob(st, 3, packet.message_id, 16);

    if (sqlite3_step(st) == SQLITE_ROW) {
      print_id((uint8_t *)event->chat_message);
      printf(" | ");
      const unsigned char *text = sqlite3_column_text(st, 0);

      for (int i = 0; i < sqlite3_column_bytes(st, 0); i++)
        if (text[i] >= 32 && text[i] != 127)
          putchar(text[i]);

      putchar('\n');
    }

    sqlite3_finalize(st);
  }

  pthread_mutex_unlock(&store->lock);
}

int main(int argc, char **argv) {
  setvbuf(stdin, NULL, _IONBF, 0);

  if (argc < 2) {
    fprintf(stderr,
            "Usage: %s PROFILE_DIRECTORY [HOST PORT CA_FILE SERVER_NAME]\n",
            argv[0]);

    return 1;
  }

  umask(077);

  if (mkdir(argv[1], 0700) != 0 && errno != EEXIST) {
    perror("profile");

    return 1;
  }

  struct stat profile;

  if (lstat(argv[1], &profile) != 0 || !S_ISDIR(profile.st_mode) ||
      profile.st_uid != geteuid() || (profile.st_mode & 077)) {
    fprintf(stderr, "Profile directory must be owned by you with mode 0700.\n");

    return 1;
  }

  char lock_path[PATH_MAX];

  if (snprintf(lock_path, sizeof(lock_path), "%s/profile.lock", argv[1]) >=
      PATH_MAX)
    return 1;

  int profile_lock =
      open(lock_path, O_RDWR | O_CREAT | O_CLOEXEC | O_NOFOLLOW, 0600);

  if (profile_lock < 0 || flock(profile_lock, LOCK_EX | LOCK_NB) != 0) {
    fprintf(stderr, "Profile is already running or cannot be locked.\n");

    return 1;
  }

  char identity[PATH_MAX], database[PATH_MAX];

  if (snprintf(identity, sizeof(identity), "%s/identity.bin", argv[1]) >=
          PATH_MAX ||
      snprintf(database, sizeof(database), "%s/conversations.sqlite",
               argv[1]) >= PATH_MAX)
    return 1;

  local_store_t store = {0};
  pthread_mutex_init(&store.lock, NULL);

  if (sqlite3_open(database, &store.db) != SQLITE_OK ||
      !execute(store.db,
               "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA "
               "busy_timeout=3000;"
               "CREATE TABLE IF NOT EXISTS conversations(id BLOB PRIMARY "
               "KEY,revision INTEGER NOT NULL,members BLOB NOT NULL,active "
               "INTEGER NOT NULL);"
               "CREATE TABLE IF NOT EXISTS messages(conversation BLOB NOT "
               "NULL,sender BLOB NOT NULL,id BLOB NOT NULL,body TEXT NOT "
               "NULL,PRIMARY KEY(conversation,sender,id));"
               "CREATE TABLE IF NOT EXISTS cursors(device BLOB PRIMARY KEY,seq "
               "INTEGER NOT NULL);")) {
    fprintf(stderr, "Local database failed\n");

    return 1;
  }

  faith_client_init_global(0);
  faith_client_config_t config = {
      .host = argc > 2 ? argv[2] : "127.0.0.1",
      .port = argc > 3 ? atoi(argv[3]) : 4433,
      .ca_file = argc > 4 && strcmp(argv[4], "-") ? argv[4] : NULL,
      .server_name = argc > 5 ? argv[5] : (argc > 2 ? argv[2] : "localhost"),
      .identity_path = identity,
      .store_event = store_event,
      .store_event_context = &store};

  faith_client_t *client = faith_client_create(&config);

  if (!client) {
    fprintf(stderr, "Client initialization failed\n");

    return 1;
  }

  faith_auth_id_t   self;
  faith_device_id_t device;
  faith_client_identity(client, &self, &device);
  printf("Identity: ");
  print_id(self.bytes);
  printf("\nPLAINTEXT group chat over verified TLS (no MLS yet).\n");
  printf("/new AUTH_ID ... | /use CONVERSATION_ID | /add AUTH_ID | /remove "
         "AUTH_ID\n/list | /members | /history | /approve | /deny | /reconnect "
         "| /quit | otherwise send text "
         "to selected conversation\n");

  fflush(stdout);

  if (faith_client_start(client) != FAITH_OK)
    return 1;

  uint8_t selected[16] = {0};
  bool    have_selected = false, running = true, authorized = false;
  bool pending_operation = false, pending_echo = false, pending_result = false;
  uint8_t pending_id[16] = {0};

  while (running) {
    struct pollfd fds[2] = {
        {.fd = STDIN_FILENO, .events = POLLIN},
        {.fd = faith_client_event_fd(client), .events = POLLIN}};

    if (poll(fds, 2, -1) < 0) {
      if (errno == EINTR)
        continue;

      break;
    }

    if (fds[1].revents & POLLIN) {
      uint64_t counter;
      read(fds[1].fd, &counter, sizeof(counter));
      faith_event_t event;

      while (faith_client_next_event(client, &event) == FAITH_OK) {
        if (event.type == FAITH_EVENT_AUTHORIZED)
          authorized = true;

        if (event.type == FAITH_EVENT_DISCONNECTED ||
            event.type == FAITH_EVENT_SERVER_DISCONNECT)
          authorized = false;

        if (event.type == FAITH_EVENT_DISCONNECTED ||
            event.type == FAITH_EVENT_COMMAND_FAILED) {
          pending_operation = false;
        }

        if (event.type == FAITH_EVENT_COMMAND_RESULT && pending_operation) {
          if (event.value1 != FAITH_COMMAND_RESULT_ACCEPTED)
            pending_operation = false;
          else
            pending_result = true;
        }

        if (event.type == FAITH_EVENT_GROUP_RECEIVED) {
          faith_group_packet_t received;

          if (pending_operation && event.chat_message_size >= 16 &&
              faith_group_decode((uint8_t *)event.chat_message + 16,
                                 event.chat_message_size - 16,
                                 &received) == FAITH_OK &&
              !memcmp(received.message_id, pending_id, 16))
            pending_echo = true;

          printf("Conversation event: ");
          print_id(event.value0_128);
          printf(" (kind=%llu)\n", (unsigned long long)event.value1);

          if (!have_selected) {
            memcpy(selected, event.value0_128, 16);
            have_selected = true;
          }

          if (event.value1 == FAITH_GROUP_MESSAGE)
            show_message(&store, &event);
        } else if (event.type != FAITH_EVENT_PONG)
          printf("%s %s\n", faith_event_name(event.type), event.message);

        if (pending_echo && pending_result)
          pending_operation = false;

        faith_client_free_event(&event);
      }

      fflush(stdout);
    }

    if (fds[0].revents & (POLLIN | POLLHUP)) {
      char line[FAITH_GROUP_MAX_TEXT + 128];

      if (!fgets(line, sizeof(line), stdin))
        break;

      line[strcspn(line, "\n")] = 0;

      if (!strcmp(line, "/quit"))
        break;

      if (!strcmp(line, "/approve") || !strcmp(line, "/deny") ||
          !strcmp(line, "/reconnect")) {
        faith_status_code_t rc =
            !strcmp(line, "/approve")
                ? faith_client_approve_pending_device_auth(client)
            : !strcmp(line, "/deny")
                ? faith_client_deny_pending_device_auth(client)
                : faith_client_reconnect(client);

        printf("%s\n", faith_status_code_name(rc));
        continue;
      }

      if (!strcmp(line, "/list")) {
        show_store(&store, NULL);
        continue;
      }

      if (!strncmp(line, "/use ", 5)) {
        have_selected = parse_id(line + 5, selected);
        printf("Selected: %s\n", have_selected ? line + 5 : "invalid ID");
        continue;
      }

      if (!strcmp(line, "/history")) {
        if (have_selected)
          show_store(&store, selected);

        continue;
      }

      if (!authorized) {
        puts("Wait for authorization before sending.");
        continue;
      }
      if (pending_operation) {
        puts(
            "Wait for the pending operation to commit before sending another.");

        continue;
      }

      faith_group_packet_t packet = {.format = FAITH_GROUP_PLAINTEXT_V1};
      faith_auth_id_t      targets[FAITH_GROUP_MAX_TARGETS];
      size_t               n_targets = 0;

      if (!strncmp(line, "/new ", 5)) {
        packet.kind = FAITH_GROUP_CREATE;
        packet.members[0] = self;
        packet.n_members = 1;
        char *save;
        char *token = strtok_r(line + 5, " ,", &save);
        bool  valid = true;

        while (token) {
          faith_auth_id_t id;

          if (!parse_id(token, id.bytes) ||
              packet.n_members == FAITH_GROUP_MAX_MEMBERS ||
              member(&packet, id.bytes)) {
            valid = false;
            break;
          }

          packet.members[packet.n_members++] = id;
          token = strtok_r(NULL, " ,", &save);
        }

        if (!valid) {
          puts("Invalid/duplicate participant or group is full.");
          continue;
        }

        faith_random_bytes(packet.conversation_id, 16);
        memcpy(selected, packet.conversation_id, 16);
        have_selected = true;
        memcpy(targets, packet.members, packet.n_members * 16);
        n_targets = packet.n_members;
      } else {
        bool active = false;
        pthread_mutex_lock(&store.lock);
        bool found =
            have_selected && load_group(store.db, selected, &packet, &active);

        pthread_mutex_unlock(&store.lock);

        if (!found || !active) {
          puts("Select an active conversation first.");
          continue;
        }

        if (!strcmp(line, "/members")) {
          for (size_t i = 0; i < packet.n_members; i++) {
            print_id(packet.members[i].bytes);
            putchar('\n');
          }

          continue;
        }

        memcpy(targets, packet.members, packet.n_members * 16);
        n_targets = packet.n_members;

        if (!strncmp(line, "/add ", 5) || !strncmp(line, "/remove ", 8)) {
          bool            add = !strncmp(line, "/add ", 5);
          faith_auth_id_t id;

          if (memcmp(self.bytes, packet.members[0].bytes, 16) ||
              !parse_id(line + (add ? 5 : 8), id.bytes) ||
              !memcmp(id.bytes, self.bytes, 16)) {
            puts("Only the creator can edit membership; creator cannot be "
                 "removed.");

            continue;
          }

          if (add) {
            if (member(&packet, id.bytes) ||
                packet.n_members == FAITH_GROUP_MAX_MEMBERS) {
              puts("Already a member or full.");
              continue;
            }

            targets[n_targets++] = id;
            packet.members[packet.n_members++] = id;
          } else {
            size_t i = 1;

            while (i < packet.n_members &&
                   memcmp(packet.members[i].bytes, id.bytes, 16))
              i++;

            if (i == packet.n_members) {
              puts("Not a member.");
              continue;
            }

            memmove(&packet.members[i], &packet.members[i + 1],
                    (packet.n_members - i - 1) * 16);

            packet.n_members--;
          }

          packet.kind = FAITH_GROUP_UPDATE;
          packet.revision++;
        } else {
          if (line[0] == '/' || !line[0]) {
            puts("Unknown command.");
            continue;
          }

          packet.kind = FAITH_GROUP_MESSAGE;
          packet.text = (uint8_t *)line;
          packet.text_size = strlen(line);
        }
      }

      faith_random_bytes(packet.message_id, 16);
      faith_status_code_t rc =
          faith_client_deliver_group(client, &packet, targets, n_targets);

      if (rc == FAITH_OK) {
        pending_operation = true;
        pending_echo = false;
        pending_result = false;
        memcpy(pending_id, packet.message_id, 16);
      }

      printf("Queued conversation ");
      print_id(packet.conversation_id);
      printf(": %s\n", faith_status_code_name(rc));
      fflush(stdout);
    }
  }

  faith_client_stop(client);
  faith_client_destroy(client);
  sqlite3_close(store.db);
  pthread_mutex_destroy(&store.lock);

  return 0;
}
