#include "helpers.h"

#include <sys/eventfd.h>
#include <unistd.h>

static void *_run(void *arg);

static void *_run(void *arg) {
  db_async_pool_t *worker = arg;
  faith_db_t       db = {0};

  for (;;) {
    pthread_mutex_lock(&worker->mutex);

    while (!worker->stopping && worker->processed == worker->submitted)
      pthread_cond_wait(&worker->ready, &worker->mutex);

    if (worker->stopping && worker->processed == worker->submitted) {
      pthread_mutex_unlock(&worker->mutex);
      break;
    }

    size_t         index = worker->processed % DB_ASYNC_CAPACITY;
    db_async_job_t job = worker->jobs[index];
    pthread_mutex_unlock(&worker->mutex);

    if (!db.conn) {
      job.status = db_connect(worker->connection_str, &db);

      if (job.status == FAITH_OK) {
        PGresult *settings = PQexec(
            db.conn,
            "SET statement_timeout = '3000ms'; SET lock_timeout = '2000ms'");
        bool configured =
            settings && PQresultStatus(settings) == PGRES_COMMAND_OK;
        PQclear(settings);

        if (!configured) {
          job.status = FAITH_ERR_IO;
          db_disconnect(&db);
        }
      }

    } else {
      job.status = FAITH_OK;
    }

    if (job.status == FAITH_OK)
      job.status = job.run(&db, job.data);

    if (db.conn && PQstatus(db.conn) != CONNECTION_OK)
      db_disconnect(&db);

    pthread_mutex_lock(&worker->mutex);
    worker->jobs[index] = job;
    worker->processed++;
    pthread_mutex_unlock(&worker->mutex);

    uint64_t one = 1;
    while (write(worker->event_fd, &one, sizeof(one)) < 0 && errno == EINTR) {
    }
  }

  db_disconnect(&db);
  return NULL;
}

faith_status_code_t db_async_init(db_async_pool_t *worker,
                                  const char      *connection_str) {
  if (!worker || !connection_str)
    return FAITH_ERR_INVALID;

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

faith_status_code_t db_async_submit(db_async_pool_t      *worker,
                                    const db_async_job_t *job) {
  if (!worker || !worker->started || !job || !job->run || !job->complete ||
      !job->destroy)
    return FAITH_ERR_INVALID;

  pthread_mutex_lock(&worker->mutex);

  if (worker->submitted - worker->consumed == DB_ASYNC_CAPACITY ||
      worker->stopping) {
    pthread_mutex_unlock(&worker->mutex);
    return FAITH_ERR_OVERFLOW;
  }

  worker->jobs[worker->submitted % DB_ASYNC_CAPACITY] = *job;
  worker->submitted++;
  pthread_cond_signal(&worker->ready);
  pthread_mutex_unlock(&worker->mutex);
  return FAITH_OK;
}

void db_async_dispatch(db_async_pool_t *worker, void *owner) {
  uint64_t count;
  while (read(worker->event_fd, &count, sizeof(count)) < 0 && errno == EINTR) {
  }

  for (;;) {
    pthread_mutex_lock(&worker->mutex);

    if (worker->consumed == worker->processed) {
      pthread_mutex_unlock(&worker->mutex);
      break;
    }

    db_async_job_t job = worker->jobs[worker->consumed % DB_ASYNC_CAPACITY];
    worker->consumed++;
    pthread_mutex_unlock(&worker->mutex);
    job.complete(owner, job.data, job.status);
    job.destroy(job.data);
  }
}

void db_async_destroy(db_async_pool_t *worker) {
  if (!worker->started)
    return;

  pthread_mutex_lock(&worker->mutex);
  worker->stopping = true;
  pthread_cond_signal(&worker->ready);
  pthread_mutex_unlock(&worker->mutex);
  pthread_join(worker->thread, NULL);

  while (worker->consumed != worker->submitted) {
    db_async_job_t *job = &worker->jobs[worker->consumed % DB_ASYNC_CAPACITY];
    job->destroy(job->data);
    worker->consumed++;
  }

  pthread_cond_destroy(&worker->ready);
  pthread_mutex_destroy(&worker->mutex);
  close(worker->event_fd);
  free(worker->connection_str);
  worker->started = false;
}
