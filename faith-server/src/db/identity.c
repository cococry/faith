#include "identity.h"

#include <sys/eventfd.h>
#include <unistd.h>

static bool  _command(faith_db_t *db, const char *sql);
static void *_run(void *arg);

static bool _command(faith_db_t *db, const char *sql) {
  PGresult *res = PQexec(db->conn, sql);
  bool      ok = res && PQresultStatus(res) == PGRES_COMMAND_OK;
  PQclear(res);
  return ok;
}

static void *_run(void *arg) {
  db_identity_worker_t *worker = arg;
  faith_db_t            db = {0};

  for (;;) {
    pthread_mutex_lock(&worker->mutex);

    while (!worker->stopping && worker->processed == worker->submitted)
      pthread_cond_wait(&worker->ready, &worker->mutex);

    if (worker->stopping) {
      pthread_mutex_unlock(&worker->mutex);
      break;
    }

    size_t            index = worker->processed % DB_IDENTITY_CAPACITY;
    db_identity_job_t job = worker->jobs[index];
    pthread_mutex_unlock(&worker->mutex);

    if (!db.conn)
      job.status = db_connect(worker->connection_str, &db);
    else
      job.status = FAITH_OK;

    if (job.status == FAITH_OK)
      job.status = db_identity_resolve(&db, &job);

    if (db.conn && PQstatus(db.conn) != CONNECTION_OK)
      db_disconnect(&db);

    pthread_mutex_lock(&worker->mutex);
    worker->jobs[index] = job;
    worker->processed++;
    pthread_mutex_unlock(&worker->mutex);

    uint64_t one = 1;
    while (write(worker->event_fd, &one, sizeof(one)) < 0 && errno == EINTR);
  }

  db_disconnect(&db);
  return NULL;
}

faith_status_code_t db_identity_resolve(faith_db_t        *db,
                                        db_identity_job_t *job) {
  if (!db || !db->conn || !job)
    return FAITH_ERR_INVALID;

  if (!_command(db, "BEGIN") ||
      !_command(db, "SET LOCAL statement_timeout = '3000ms'") ||
      !_command(db, "SET LOCAL lock_timeout = '2000ms'")) {
    _command(db, "ROLLBACK");
    return FAITH_ERR_IO;
  }

  const char         *values[] = {(const char *)job->auth_id.bytes};
  const int           lengths[] = {FAITH_AUTH_ID_SIZE};
  const int           formats[] = {1};
  PGresult           *res = NULL;
  bool                created = false;
  faith_status_code_t rc = FAITH_ERR_IO;

  /* only the transaction that claims the id gets to choose its first key */
  if (!job->link) {
    res = PQexecParams(db->conn,
                       "INSERT INTO users (auth_id) VALUES ($1) "
                       "ON CONFLICT DO NOTHING RETURNING auth_id",
                       1, NULL, values, lengths, formats, 1);

    if (!res || PQresultStatus(res) != PGRES_TUPLES_OK)
      goto done;

    created = PQntuples(res) == 1;
    PQclear(res);
    res = NULL;
  }

  res = PQexecParams(db->conn,
                     "SELECT auth_id FROM users WHERE auth_id = $1 FOR UPDATE",
                     1, NULL, values, lengths, formats, 1);

  if (!res || PQresultStatus(res) != PGRES_TUPLES_OK || PQntuples(res) != 1)
    goto done;

  PQclear(res);
  res = NULL;

  db_user_device_list_t list = {0};
  rc = db_device_repo_get_device_list(db, &job->auth_id, &list);

  if (rc != FAITH_OK)
    goto done;

  bool approved = false;
  bool found = false;
  bool matches = false;

  for (size_t i = 0; i < list.n_devices; i++) {
    db_user_device_t *device = &list.devices[i];

    if (faith_device_id_equal(device->device_id, job->device.device_id)) {
      found = true;
      matches = memcmp(device->public_key, job->device.public_key,
                       FAITH_ED25519_PUBLIC_KEY_SIZE) == 0;
    }

    if (faith_device_id_equal(device->device_id, job->approver_id) &&
        memcmp(device->public_key, job->approver_key,
               FAITH_ED25519_PUBLIC_KEY_SIZE) == 0)
      approved = true;
  }

  db_device_repo_destroy_device_list(&list);

  if (found)
    rc = matches ? FAITH_OK : FAITH_ERR_UNAUTHORIZED;
  else if (created || (job->link && approved))
    rc = db_device_repo_insert(db, &job->auth_id, &job->device);
  else
    rc = job->link ? FAITH_ERR_UNAUTHORIZED : FAITH_ERR_NOT_FOUND;

done:
  PQclear(res);

  if (rc == FAITH_OK) {
    if (!_command(db, "COMMIT"))
      rc = FAITH_ERR_IO;
  } else {
    _command(db, "ROLLBACK");
  }

  return rc;
}

faith_status_code_t db_identity_worker_init(db_identity_worker_t *worker,
                                            const char *connection_str) {
  memset(worker, 0, sizeof(*worker));
  worker->event_fd = eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC);
  worker->connection_str = strdup(connection_str);

  if (worker->event_fd < 0 || !worker->connection_str)
    goto fail;

  if (pthread_mutex_init(&worker->mutex, NULL) != 0)
    goto fail;

  if (pthread_cond_init(&worker->ready, NULL) != 0) {
    pthread_mutex_destroy(&worker->mutex);
    goto fail;
  }

  if (pthread_create(&worker->thread, NULL, _run, worker) != 0) {
    pthread_cond_destroy(&worker->ready);
    pthread_mutex_destroy(&worker->mutex);
    goto fail;
  }

  worker->started = true;
  return FAITH_OK;

fail:
  if (worker->event_fd >= 0)
    close(worker->event_fd);

  free(worker->connection_str);
  worker->event_fd = -1;
  return FAITH_ERR_IO;
}

faith_status_code_t db_identity_submit(db_identity_worker_t    *worker,
                                       const db_identity_job_t *job) {
  pthread_mutex_lock(&worker->mutex);

  if (worker->submitted - worker->consumed == DB_IDENTITY_CAPACITY ||
      worker->stopping) {
    pthread_mutex_unlock(&worker->mutex);
    return FAITH_ERR_OVERFLOW;
  }

  worker->jobs[worker->submitted % DB_IDENTITY_CAPACITY] = *job;
  worker->submitted++;
  pthread_cond_signal(&worker->ready);
  pthread_mutex_unlock(&worker->mutex);
  return FAITH_OK;
}

bool db_identity_take(db_identity_worker_t *worker, db_identity_job_t *job) {
  pthread_mutex_lock(&worker->mutex);
  bool available = worker->consumed != worker->processed;

  if (available) {
    *job = worker->jobs[worker->consumed % DB_IDENTITY_CAPACITY];
    worker->consumed++;
  }

  pthread_mutex_unlock(&worker->mutex);
  return available;
}

void db_identity_worker_destroy(db_identity_worker_t *worker) {
  if (!worker->started)
    return;

  pthread_mutex_lock(&worker->mutex);
  worker->stopping = true;
  pthread_cond_signal(&worker->ready);
  pthread_mutex_unlock(&worker->mutex);
  pthread_join(worker->thread, NULL);
  pthread_cond_destroy(&worker->ready);
  pthread_mutex_destroy(&worker->mutex);
  close(worker->event_fd);
  free(worker->connection_str);
  worker->started = false;
}
