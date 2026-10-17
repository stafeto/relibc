//! Timers whose notice is a function run in a thread (SIGEV_THREAD).
//!
//! The clock service of stafeto delivers signals; a timer that asks for a
//! thread is kept by the library instead: a helper thread of the timer sleeps until the time of the timer (a futex wait with a deadline on
//! CLOCK_MONOTONIC), and at each expiry runs the function in a thread of its
//! own, detached, as the notification attributes say. No signal number is
//! used. The identifier of such a timer is the address of its object with
//! bit 62 set; the identifiers of the clock service are small numbers. A
//! child of `fork` does not have the timer (timers are not inherited).

use alloc::boxed::Box;
use core::{
    mem::MaybeUninit,
    ptr,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use crate::{
    error::Errno,
    header::{
        aio,
        bits_sigset_t::sigset_t,
        errno::EINVAL,
        pthread::{pthread_create, pthread_detach},
        signal::{SIG_BLOCK, pthread_sigmask, sigevent, sigfillset},
        time::{CLOCK_MONOTONIC, CLOCK_REALTIME, TIMER_ABSTIME, itimerspec, timespec},
    },
    out::Out,
    platform::{
        Pal, Sys,
        types::{c_int, c_void, clockid_t, pthread_t, timer_t},
    },
    sync::{Mutex, futex_wait, futex_wake},
};

/// Set in the identifier of a timer of this module.
const TAG: usize = 1 << 62;
const NANOS: u64 = 1_000_000_000;

struct Inner {
    armed: bool,
    /// The next expiry, on the monotonic clock (ns).
    next: u64,
    /// The period (ns), 0 for a timer that fires once.
    interval: u64,
    /// The expiries that came while the previous notice was not yet given.
    overrun: c_int,
}

struct Timer {
    clock: clockid_t,
    event: sigevent,
    inner: Mutex<Inner>,
    /// Counts the changes the helper must look at.
    epoch: AtomicU32,
    gone: AtomicBool,
}

unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

fn now(clock: clockid_t) -> u64 {
    let mut at = MaybeUninit::<timespec>::uninit();
    let _ = Sys::clock_gettime(clock, Out::from_uninit_mut(&mut at));
    let at = unsafe { at.assume_init() };
    (at.tv_sec.max(0) as u64)
        .saturating_mul(NANOS)
        .saturating_add(at.tv_nsec.max(0) as u64)
}

fn to_ns(at: &timespec) -> u64 {
    (at.tv_sec.max(0) as u64)
        .saturating_mul(NANOS)
        .saturating_add(at.tv_nsec.max(0) as u64)
}

fn to_timespec(ns: u64) -> timespec {
    timespec {
        tv_sec: (ns / NANOS) as _,
        tv_nsec: (ns % NANOS) as _,
    }
}

fn valid(at: &timespec) -> bool {
    at.tv_sec >= 0 && (0..NANOS as i64).contains(&(at.tv_nsec as i64))
}

/// The timer of an identifier of this module, if it is one.
fn timer_of(id: timer_t) -> Option<&'static Timer> {
    let id = id as usize;
    if id & TAG == 0 {
        return None;
    }
    Some(unsafe { &*((id & !TAG) as *const Timer) })
}

pub fn is_thread_timer(id: timer_t) -> bool {
    id as usize & TAG != 0
}

fn changed(timer: &Timer) {
    timer.epoch.fetch_add(1, Ordering::SeqCst);
    futex_wake(&timer.epoch, 1);
}

extern "C" fn helper(arg: *mut c_void) -> *mut c_void {
    let mut set = MaybeUninit::<sigset_t>::uninit();
    unsafe {
        sigfillset(set.as_mut_ptr());
        pthread_sigmask(SIG_BLOCK, set.as_ptr(), ptr::null_mut());
    }
    let timer = unsafe { &*(arg as *const Timer) };
    loop {
        let (seen, until) = {
            let mut inner = timer.inner.lock();
            if timer.gone.load(Ordering::Acquire) {
                drop(inner);
                drop(unsafe { Box::from_raw(arg.cast::<Timer>()) });
                return ptr::null_mut();
            }
            let seen = timer.epoch.load(Ordering::SeqCst);
            if !inner.armed {
                (seen, None)
            } else {
                let at = now(CLOCK_MONOTONIC);
                if at >= inner.next {
                    if inner.interval > 0 {
                        let missed = (at - inner.next) / inner.interval;
                        inner.next += (missed + 1) * inner.interval;
                        inner.overrun = missed.min(i32::MAX as u64) as c_int;
                    } else {
                        inner.armed = false;
                        inner.overrun = 0;
                    }
                    drop(inner);
                    unsafe { aio::notify_event(&timer.event) };
                    continue;
                }
                (seen, Some(inner.next))
            }
        };
        let deadline = until.map(to_timespec);
        futex_wait(&timer.epoch, seen, deadline.as_ref());
    }
}

/// A timer on `clock` that gives its notice as `event` (SIGEV_THREAD) asks.
pub fn create(clock: clockid_t, event: &sigevent) -> Result<timer_t, Errno> {
    if (clock != CLOCK_REALTIME && clock != CLOCK_MONOTONIC)
        || event.sigev_notify_function.is_none()
    {
        return Err(Errno(EINVAL));
    }
    let timer = Box::into_raw(Box::new(Timer {
        clock,
        event: event.clone(),
        inner: Mutex::new(Inner {
            armed: false,
            next: 0,
            interval: 0,
            overrun: 0,
        }),
        epoch: AtomicU32::new(0),
        gone: AtomicBool::new(false),
    }));
    let mut thread = MaybeUninit::<pthread_t>::uninit();
    let made = unsafe { pthread_create(thread.as_mut_ptr(), ptr::null(), helper, timer.cast()) };
    if made != 0 {
        drop(unsafe { Box::from_raw(timer) });
        return Err(Errno(crate::header::errno::EAGAIN));
    }
    unsafe { pthread_detach(thread.assume_init()) };
    Ok((timer as usize | TAG) as timer_t)
}

fn remaining(timer: &Timer, inner: &Inner) -> itimerspec {
    let left = if inner.armed {
        inner.next.saturating_sub(now(CLOCK_MONOTONIC))
    } else {
        0
    };
    let _ = timer;
    itimerspec {
        it_interval: to_timespec(inner.interval),
        it_value: to_timespec(left),
    }
}

pub fn gettime(id: timer_t) -> Option<itimerspec> {
    let timer = timer_of(id)?;
    let inner = timer.inner.lock();
    Some(remaining(timer, &inner))
}

pub fn getoverrun(id: timer_t) -> Option<c_int> {
    let timer = timer_of(id)?;
    let overrun = timer.inner.lock().overrun;
    Some(overrun)
}

pub fn delete(id: timer_t) -> Option<()> {
    let timer = timer_of(id)?;
    timer.gone.store(true, Ordering::Release);
    changed(timer);
    Some(())
}

/// The old setting, or the error number for a value that is no good.
pub fn settime(id: timer_t, flags: c_int, value: &itimerspec) -> Option<Result<itimerspec, Errno>> {
    let timer = timer_of(id)?;
    if !valid(&value.it_value) || !valid(&value.it_interval) {
        return Some(Err(Errno(EINVAL)));
    }
    let mut inner = timer.inner.lock();
    let old = remaining(timer, &inner);
    let mono = now(CLOCK_MONOTONIC);
    if value.it_value.tv_sec == 0 && value.it_value.tv_nsec == 0 {
        inner.armed = false;
    } else {
        let given = to_ns(&value.it_value);
        inner.next = if flags & TIMER_ABSTIME == 0 {
            mono.saturating_add(given)
        } else if timer.clock == CLOCK_MONOTONIC {
            given
        } else {
            // The time of the realtime clock is turned into the monotonic
            // scale now, as the clock service does for its timers.
            mono.saturating_add(given.saturating_sub(now(CLOCK_REALTIME)))
        };
        inner.interval = to_ns(&value.it_interval);
        inner.overrun = 0;
        inner.armed = true;
    }
    drop(inner);
    changed(timer);
    Some(Ok(old))
}
