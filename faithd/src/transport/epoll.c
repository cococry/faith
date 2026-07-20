#include "epoll.h"
#include <errno.h>
#include <string.h>
#include <sys/epoll.h>


int epoll_modify_ev_mask(int epoll_fd, int fd, uint32_t mask, uint32_t *o_mask,
                         void *ptr) {
  if (!o_mask) {
    errno = EINVAL;
    return -1;
  }

  struct epoll_event ev;

  memset(&ev, 0, sizeof(ev));
  ev.events = mask;
  ev.data.ptr = ptr;

  *o_mask = mask;

  return epoll_ctl(epoll_fd, EPOLL_CTL_MOD, fd, &ev);
}

int epoll_del_fd(int epoll_fd, int fd) {
  return epoll_ctl(epoll_fd, EPOLL_CTL_DEL, fd, NULL);
}
