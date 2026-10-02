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

// The layer reads and writes these with the Linux AArch64 layouts and
// numbers relibc uses: `struct sigaction`, a 64-bit sigset_t, siginfo_t.
unsafe extern "C" {
    fn stafeto_sigaction(sig: c_int, act: *const sigaction, old: *mut sigaction) -> c_int;
    fn stafeto_sigprocmask(how: c_int, set: *const sigset_t, old: *mut sigset_t) -> c_int;
    fn stafeto_sigpending(set: *mut sigset_t) -> c_int;
    fn stafeto_sigsuspend(mask: *const sigset_t) -> c_int;
    fn stafeto_sigtimedwait(
        set: *const sigset_t,
        info: *mut siginfo_t,
        timeout: *const timespec,
    ) -> c_int;
    fn stafeto_raise(sig: c_int) -> c_int;
    fn stafeto_kill(pid: pid_t, sig: c_int) -> c_int;
    fn stafeto_killpg(pgrp: pid_t, sig: c_int) -> c_int;
    fn stafeto_thread_kill(id: c_int, sig: c_int) -> c_int;
}

fn ret(value: c_int) -> Result<c_int> {
    if value < 0 {
        Err(Errno(-value))
    } else {
        Ok(value)
    }
}

/// pthread_kill: the platform's thread number is the OsTid.
pub(crate) fn thread_kill(os_tid: crate::pthread::OsTid, signal: usize) -> Result<()> {
    ret(unsafe { stafeto_thread_kill(os_tid.thread_id as c_int, signal as c_int) }).map(|_| ())
}

impl PalSignal for Sys {
    #[expect(deprecated)]
    fn getitimer(which: c_int, out: &mut itimerval) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn kill(pid: pid_t, sig: c_int) -> Result<()> {
        ret(unsafe { stafeto_kill(pid, sig) }).map(|_| ())
    }
    fn sigqueue(pid: pid_t, sig: c_int, val: sigval) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn killpg(pgrp: pid_t, sig: c_int) -> Result<()> {
        ret(unsafe { stafeto_killpg(pgrp, sig) }).map(|_| ())
    }

    fn raise(sig: c_int) -> Result<()> {
        ret(unsafe { stafeto_raise(sig) }).map(|_| ())
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
        let act = act.map_or(ptr::null(), ptr::from_ref);
        let old = oact.map_or(ptr::null_mut(), ptr::from_mut);
        ret(unsafe { stafeto_sigaction(sig, act, old) }).map(|_| ())
    }

    unsafe fn sigaltstack(ss: Option<&stack_t>, old_ss: Option<&mut stack_t>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sigpending(set: &mut sigset_t) -> Result<()> {
        ret(unsafe { stafeto_sigpending(set) }).map(|_| ())
    }

    fn sigprocmask(how: c_int, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) -> Result<()> {
        let set = set.map_or(ptr::null(), ptr::from_ref);
        let old = oset.map_or(ptr::null_mut(), ptr::from_mut);
        ret(unsafe { stafeto_sigprocmask(how, set, old) }).map(|_| ())
    }

    fn sigsuspend(mask: &sigset_t) -> Errno {
        match ret(unsafe { stafeto_sigsuspend(mask) }) {
            Err(errno) => errno,
            Ok(_) => Errno(crate::header::errno::EINTR),
        }
    }

    fn sigtimedwait(
        set: &sigset_t,
        sig: Option<&mut siginfo_t>,
        tp: Option<&timespec>,
    ) -> Result<c_int> {
        let info = sig.map_or(ptr::null_mut(), ptr::from_mut);
        let timeout = tp.map_or(ptr::null(), ptr::from_ref);
        ret(unsafe { stafeto_sigtimedwait(set, info, timeout) })
    }
}
