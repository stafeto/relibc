// SPDX-License-Identifier: MIT
#![allow(unused_variables, unused_imports, unused_mut)]
use crate::header::errno::ENOSYS;
use core::{
    mem,
    ptr::{self, addr_of},
};

use super::{
    super::{
        PalSignal,
        types::{c_int, pid_t},
    },
    Sys, stafeto_exit,
};
#[expect(deprecated)]
use crate::header::sys_time::itimerval;
use crate::{
    error::{Errno, Result},
    header::{
        bits_sigset_t::sigset_t,
        bits_timespec::timespec,
        signal::{
            SA_RESTORER, SI_QUEUE, sigaction, siginfo_common, siginfo_sifields, siginfo_t, sigval,
            stack_t,
        },
    },
};

impl PalSignal for Sys {
    #[expect(deprecated)]
    fn getitimer(which: c_int, out: &mut itimerval) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn kill(pid: pid_t, sig: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }
    fn sigqueue(pid: pid_t, sig: c_int, val: sigval) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn killpg(pgrp: pid_t, sig: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn raise(sig: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    #[expect(deprecated)]
    fn setitimer(which: c_int, new: &itimerval, old: Option<&mut itimerval>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sigaction(
        sig: c_int,
        act: Option<&sigaction>,
        oact: Option<&mut sigaction>,
    ) -> Result<(), Errno> {
        Err(Errno(ENOSYS))
    }

    unsafe fn sigaltstack(ss: Option<&stack_t>, old_ss: Option<&mut stack_t>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sigpending(set: &mut sigset_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sigprocmask(how: c_int, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sigsuspend(mask: &sigset_t) -> Errno {
        Errno(ENOSYS)
    }

    fn sigtimedwait(
        set: &sigset_t,
        sig: Option<&mut siginfo_t>,
        tp: Option<&timespec>,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }
}
