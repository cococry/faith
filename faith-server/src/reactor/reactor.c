#include "reactor.h"
#include <sys/epoll.h>

#define _MODULE_NAME "[reactor]: "

static reactor_events_t reactor_event_mask_from_epoll(uint32_t epoll_events) {
  reactor_events_t events = 0;

  if (epoll_events & EPOLLIN)
    events |= REACTOR_READABLE;

  if (epoll_events & EPOLLOUT)
    events |= REACTOR_WRITABLE;

  if (epoll_events & EPOLLERR)
    events |= REACTOR_ERROR;

  if (epoll_events & (EPOLLHUP | EPOLLRDHUP))
    events |= REACTOR_CLOSED;

  return events;
}

static uint32_t reactor_event_mask_to_epoll(reactor_events_t events) {
  uint32_t epoll_events = 0;

  if (events & REACTOR_READABLE)
    epoll_events |= EPOLLIN;

  if (events & REACTOR_WRITABLE)
    epoll_events |= EPOLLOUT;

  if (events & REACTOR_CLOSED)
    epoll_events |= EPOLLRDHUP;

  return epoll_events;
}

/*========================================== */
/* PUBLIC API - public api - public API */
/*==========================================*/

faith_status_code_t reactor_init(reactor_context_t *o_reactor_ctx) {
  if (!o_reactor_ctx)
    return FAITH_ERR_INVALID;

  o_reactor_ctx->fd = -1;

  int epoll_fd = epoll_create1(EPOLL_CLOEXEC);
  if (epoll_fd == -1) {
    nob_log(ERROR, _MODULE_NAME "epoll_create1(EPOLL_CLOEXEC) failed: %s",
            strerror(errno));
    return FAITH_ERR_EPOLL;
  }

  o_reactor_ctx->fd = epoll_fd;

  return FAITH_OK;
}

faith_status_code_t reactor_add(reactor_context_t *ctx, reactor_source_t *src) {
  if (!ctx || !src || ctx->fd < 0 || src->fd < 0)
    return FAITH_ERR_INVALID;

  struct epoll_event ev = {0};

  ev.events = reactor_event_mask_to_epoll(src->interests);
  ev.data.ptr = src;

  if (epoll_ctl(ctx->fd, EPOLL_CTL_ADD, src->fd, &ev) < 0) {
    nob_log(ERROR,
            _MODULE_NAME
            "reactor_add() failed with epoll fd=%i, source fd=%i: %s",
            ctx->fd, src->fd, strerror(errno));
    return FAITH_ERR_EPOLL;
  }

  return FAITH_OK;
}

faith_status_code_t reactor_modify_interests(reactor_context_t *ctx,
                                             reactor_source_t  *src,
                                             reactor_events_t   new_interests) {
  if (!src || !ctx || src->fd < 0 || ctx->fd < 0) {
    return FAITH_ERR_INVALID;
  }

  struct epoll_event ev;

  memset(&ev, 0, sizeof(ev));
  ev.events = reactor_event_mask_to_epoll(new_interests);
  ev.data.ptr = src;

  if (epoll_ctl(ctx->fd, EPOLL_CTL_MOD, src->fd, &ev) < 0) {
    nob_log(
        ERROR,
        _MODULE_NAME
        "reactor_modify_interests() failed with epoll fd=%i, source fd=%i: %s",
        ctx->fd, src->fd, strerror(errno));
    return FAITH_ERR_EPOLL;
  }

  src->interests = new_interests;

  return FAITH_OK;
}

faith_status_code_t reactor_remove(reactor_context_t *ctx,
                                   reactor_source_t  *src) {

  if (!ctx || !src || ctx->fd < 0 || src->fd < 0)
    return FAITH_ERR_INVALID;
  if (epoll_ctl(ctx->fd, EPOLL_CTL_DEL, src->fd, NULL) < 0 && errno != ENOENT) {
    nob_log(ERROR,
            _MODULE_NAME
            "reactor_remove() failed with epoll fd=%i, source fd=%i: %s",
            ctx->fd, src->fd, strerror(errno));
    return FAITH_ERR_EPOLL;
  }

  src->interests = 0;
  return FAITH_OK;
}

faith_status_code_t reactor_wait(reactor_context_t *ctx, int timeout_ms,
                                 reactor_event_data_t *o_events,
                                 size_t               *o_n_events) {
  if (!ctx || !o_events || !o_n_events || ctx->fd < 0)
    return FAITH_ERR_INVALID;

  *o_n_events = 0;

  struct epoll_event events[REACTOR_MAX_EVENTS];

  int n = epoll_wait(ctx->fd, events, REACTOR_MAX_EVENTS, timeout_ms);

  if (n < 0) {
    if (errno == EINTR)
      return FAITH_OK;

    nob_log(ERROR, _MODULE_NAME "epoll_wait() failed: %s", strerror(errno));

    return FAITH_ERR_EPOLL;
  }

  for (int i = 0; i < n; i++) {
    reactor_source_t *src = events[i].data.ptr;
    o_events[i].events = reactor_event_mask_from_epoll(events[i].events);
    o_events[i].src = src;
  }

  *o_n_events = (size_t)n;

  return FAITH_OK;
}

faith_status_code_t reactor_destroy(reactor_context_t *reactor) {
  if (!reactor)
    return FAITH_ERR_INVALID;

  if (reactor->fd >= 0) {
    if (close(reactor->fd) < 0) {
      int saved_errno = errno;
      reactor->fd = -1;

      nob_log(ERROR, _MODULE_NAME "close() failed: %s", strerror(saved_errno));
      return FAITH_ERR_IO;
    }
  }
  reactor->fd = -1;

  return FAITH_OK;
}
