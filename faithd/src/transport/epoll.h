#pragma once


#include <stdint.h>

int epoll_modify_ev_mask(int epoll_fd, int fd, uint32_t mask, uint32_t *o_mask,
                         void *ptr);

int epoll_del_fd(int epoll_fd, int fd);
