// SPDX-License-Identifier: MIT
//! epoll over the layer: the three calls go to `stafeto_epoll_*`, which keep
//! the interest lists of the process and ask the network service for the
//! readiness of sockets.
use super::ret;
use crate::{
    error::Result,
    header::{bits_sigset_t::sigset_t, sys_epoll::epoll_event},
    platform::{PalEpoll, Sys, types::c_int},
};

unsafe extern "C" {
    fn stafeto_epoll_create1(flags: c_int) -> c_int;
    fn stafeto_epoll_ctl(epfd: c_int, op: c_int, fd: c_int, event: *mut epoll_event) -> c_int;
    fn stafeto_epoll_pwait(
        epfd: c_int,
        events: *mut epoll_event,
        maxevents: c_int,
        timeout: c_int,
        sigmask: *const sigset_t,
    ) -> c_int;
}

impl PalEpoll for Sys {
    fn epoll_create1(flags: c_int) -> Result<c_int> {
        // SAFETY: the call takes no pointers.
        ret(unsafe { stafeto_epoll_create1(flags) } as isize).map(|fd| fd as c_int)
    }

    unsafe fn epoll_ctl(epfd: c_int, op: c_int, fd: c_int, event: *mut epoll_event) -> Result<()> {
        // SAFETY: the caller's `event` is null (for a delete) or readable.
        ret(unsafe { stafeto_epoll_ctl(epfd, op, fd, event) } as isize).map(|_| ())
    }

    unsafe fn epoll_pwait(
        epfd: c_int,
        events: *mut epoll_event,
        maxevents: c_int,
        timeout: c_int,
        sigmask: *const sigset_t,
    ) -> Result<usize> {
        // SAFETY: the caller supplies `maxevents` writable events.
        ret(unsafe { stafeto_epoll_pwait(epfd, events, maxevents, timeout, sigmask) } as isize)
            .map(|count| count as usize)
    }
}
