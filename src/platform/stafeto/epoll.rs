// SPDX-License-Identifier: MIT
#![allow(unused_variables, unused_imports, unused_mut)]
use crate::{error::Errno, header::errno::ENOSYS};
use core::mem;

use super::{Sys, stafeto_exit};
use crate::{
    error::Result,
    header::{bits_sigset_t::sigset_t, sys_epoll::epoll_event},
    platform::{
        PalEpoll,
        types::{c_int, size_t},
    },
};

impl PalEpoll for Sys {
    fn epoll_create1(flags: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    unsafe fn epoll_ctl(epfd: c_int, op: c_int, fd: c_int, event: *mut epoll_event) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn epoll_pwait(
        epfd: c_int,
        events: *mut epoll_event,
        maxevents: c_int,
        timeout: c_int,
        sigmask: *const sigset_t,
    ) -> Result<usize> {
        Err(Errno(ENOSYS))
    }
}
