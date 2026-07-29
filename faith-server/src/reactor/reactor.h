#pragma once

#include <stdlib.h>

#include <faith-proto/core/core.h>

#define REACTOR_MAX_EVENTS 1024

typedef enum {
  REACTOR_NONE = 0,
  REACTOR_READABLE = 1u << 0,
  REACTOR_WRITABLE = 1u << 1,
  REACTOR_ERROR = 1u << 2,
  REACTOR_CLOSED = 1u << 3
} reactor_events_t;

typedef enum {
  REACTOR_SOURCE_CLIENT,
  REACTOR_SOURCE_LISTENER,
} reactor_source_type_t;

typedef struct {
  int fd;
} reactor_context_t;

typedef struct {
  int fd;

  reactor_events_t      interests;
  reactor_source_type_t type;

  void *user_data;
} reactor_source_t;

typedef struct {
  reactor_events_t  events;
  reactor_source_t *src;
} reactor_event_data_t;

faith_status_code_t reactor_init(reactor_context_t *o_reactor_ctx);

faith_status_code_t reactor_add(reactor_context_t *ctx, reactor_source_t *src);

faith_status_code_t reactor_modify_interests(reactor_context_t *ctx,
                                             reactor_source_t  *src,
                                             reactor_events_t   new_interests);

faith_status_code_t reactor_remove(reactor_context_t *ctx,
                                   reactor_source_t  *src);
faith_status_code_t reactor_wait(reactor_context_t *ctx, int timeout_ms,
                                 reactor_event_data_t *o_events,
                                 size_t               *o_n_events);

faith_status_code_t reactor_destroy(reactor_context_t *reactor);
