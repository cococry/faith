#pragma once

#include "conn.h"
#include <pthread.h>
#include <stdbool.h>

#define DB_ASYNC_CAPACITY 128

typedef struct {
  faith_status_code_t (*run)(faith_db_t *db, void *data);
  void (*complete)(void *owner, void *data, faith_status_code_t status);
  void (*destroy)(void *data);
  void               *data;
  faith_status_code_t status;
} db_async_job_t;

typedef struct {
  pthread_t       thread;
  pthread_mutex_t mutex;
  pthread_cond_t  ready;
  bool            stopping;
  bool            started;
  int             event_fd;
  char           *connection_str;
  db_async_job_t  jobs[DB_ASYNC_CAPACITY];
  size_t          submitted;
  size_t          processed;
  size_t          consumed;
} db_async_pool_t;

faith_status_code_t db_async_init(db_async_pool_t *pool,
                                  const char      *connection_str);
/* Ownership transfers only on success; completions run on the reactor thread.
 * One persistent connection preserves submission order across dependent jobs.
 */
faith_status_code_t db_async_submit(db_async_pool_t      *pool,
                                    const db_async_job_t *job);
void                db_async_dispatch(db_async_pool_t *pool, void *owner);
void                db_async_destroy(db_async_pool_t *pool);
