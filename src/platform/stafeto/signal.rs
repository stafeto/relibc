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
    fn stafeto_sigaltstack(new: *const stack_t, old: *mut stack_t) -> c_int;
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
    fn stafeto_alarm(set: c_int, value_ns: u64, interval_ns: u64, out: *mut u64) -> c_int;
    fn stafeto_sigqueue(pid: pid_t, sig: c_int, value: u64) -> c_int;
    fn stafeto_killpg(pgrp: pid_t, sig: c_int) -> c_int;
    fn stafeto_thread_kill(id: c_int, sig: c_int) -> c_int;
}

/// The real-time timer of the process (the alarm), the one interval timer
/// there is: ITIMER_VIRTUAL and ITIMER_PROF count processor time, which the
/// kernel does not account (EINVAL).
#[expect(deprecated)]
fn itimer(
    which: c_int,
    set: bool,
    value_ns: u64,
    interval_ns: u64,
    old: Option<&mut itimerval>,
) -> Result<()> {
    if which != 0 {
        return Err(Errno(crate::header::errno::EINVAL));
    }
    let mut before = [0u64; 2];
    ret(unsafe { stafeto_alarm(c_int::from(set), value_ns, interval_ns, before.as_mut_ptr()) })?;
    if let Some(old) = old {
        // Microseconds rounded up: a timer with any time left shows some.
        let split = |ns: u64| {
            let micros = ns / 1000 + u64::from(ns % 1000 != 0);
            crate::header::sys_select::timeval {
                tv_sec: (micros / 1_000_000) as _,
                tv_usec: (micros % 1_000_000) as _,
            }
        };
        old.it_value = split(before[0]);
        old.it_interval = split(before[1]);
    }
    Ok(())
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
        itimer(which, false, 0, 0, Some(out))
    }

    fn kill(pid: pid_t, sig: c_int) -> Result<()> {
        ret(unsafe { stafeto_kill(pid, sig) }).map(|_| ())
    }
    fn sigqueue(pid: pid_t, sig: c_int, val: sigval) -> Result<()> {
        // The bytes of the union, as the sender stored them.
        let bytes = unsafe { val.sival_ptr } as usize as u64;
        ret(unsafe { stafeto_sigqueue(pid, sig, bytes) }).map(|_| ())
    }

    fn killpg(pgrp: pid_t, sig: c_int) -> Result<()> {
        ret(unsafe { stafeto_killpg(pgrp, sig) }).map(|_| ())
    }

    fn raise(sig: c_int) -> Result<()> {
        ret(unsafe { stafeto_raise(sig) }).map(|_| ())
    }

    #[expect(deprecated)]
    fn setitimer(which: c_int, new: &itimerval, old: Option<&mut itimerval>) -> Result<()> {
        let micros = |t: &crate::header::sys_select::timeval| {
            let (sec, usec) = (t.tv_sec, t.tv_usec as i64);
            if sec < 0 || !(0..1_000_000).contains(&usec) {
                return Err(Errno(crate::header::errno::EINVAL));
            }
            Ok((sec as u64).saturating_mul(1_000_000_000).saturating_add(usec as u64 * 1000))
        };
        let (value, interval) = (micros(&new.it_value)?, micros(&new.it_interval)?);
        match old {
            Some(old) => itimer(which, true, value, interval, Some(old)),
            None => itimer(which, true, value, interval, None),
        }
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
        let new = ss.map_or(ptr::null(), ptr::from_ref);
        let old = old_ss.map_or(ptr::null_mut(), ptr::from_mut);
        ret(unsafe { stafeto_sigaltstack(new, old) }).map(|_| ())
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
