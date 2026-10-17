//! Timers whose notice is a function run in a thread (SIGEV_THREAD).
//!
//! The clock service of stafeto delivers signals; a timer that asks for a
//! thread is kept by the library instead: a helper thread of the timer sleeps until the time of the timer (a futex wait with a deadline on
//! CLOCK_MONOTONIC), and at each expiry runs the function in a thread of its
//! own, detached, as the notification attributes say. No signal number is
//! used. The identifier of such a timer is the address of its object with
//! bit 62 set; the identifiers of the clock service are small numbers. A
//! child of `fork` does not have the timer (timers are not inherited).
//!
//! The timers on a CPU-time clock (of the process, of a thread) are kept
//! here as well, with any notice (a signal, a function, none): the service
//! cannot tell when a process has used some processor time. The helper
//! looks at the clock, and sleeps by the fourth of the time that is left
//! (at least a millisecond), since the time of a clock cannot pass faster
//! than that. `setitimer` on ITIMER_VIRTUAL and ITIMER_PROF is such a
//! timer on the clock of the process, which sends SIGVTALRM or SIGPROF.

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
        errno::{EINVAL, EPERM},
        pthread::{
            PTHREAD_EXPLICIT_SCHED, pthread_attr_destroy, pthread_attr_init,
            pthread_attr_setinheritsched, pthread_attr_setschedparam, pthread_attr_setschedpolicy,
            pthread_create, pthread_detach, pthread_getcpuclockid, pthread_self,
        },
        sched::{SCHED_FIFO, sched_param},
        signal::{SIG_BLOCK, SIGEV_SIGNAL, SIGEV_THREAD, pthread_sigmask, sigevent, sigfillset},
        time::{
            CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID,
            TIMER_ABSTIME, itimerspec, timespec,
        },
    },
    out::Out,
    platform::{
        Pal, Sys,
        types::{c_int, c_void, clockid_t, pthread_attr_t, pthread_t, timer_t},
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
    /// The clock is a CPU-time clock: `next` is in its nanoseconds.
    cpu: bool,
    event: sigevent,
    inner: Mutex<Inner>,
    /// Counts the changes the helper must look at.
    epoch: AtomicU32,
    gone: AtomicBool,
}

unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

/// Whether `clock` is a CPU-time clock: of the process (2), of the calling
/// thread (3), or of a thread (low bits 6).
pub fn is_cpu(clock: clockid_t) -> bool {
    clock == CLOCK_PROCESS_CPUTIME_ID
        || clock == CLOCK_THREAD_CPUTIME_ID
        || (clock > 0 && clock & 7 == 6)
}

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
                let at = now(if timer.cpu { timer.clock } else { CLOCK_MONOTONIC });
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
                if timer.cpu {
                    // Its time cannot pass faster than the time of the
                    // monotonic clock: look again after a part of the time
                    // that is left.
                    let left = inner.next - at;
                    (seen, Some(now(CLOCK_MONOTONIC) + (left / 4).max(1_000_000)))
                } else {
                    (seen, Some(inner.next))
                }
            }
        };
        let deadline = until.map(to_timespec);
        futex_wait(&timer.epoch, seen, deadline.as_ref());
    }
}

/// A timer on `clock` that gives its notice as `event` (SIGEV_THREAD) asks.
pub fn create(clock: clockid_t, event: &sigevent) -> Result<timer_t, Errno> {
    let cpu = is_cpu(clock);
    if !cpu
        && ((clock != CLOCK_REALTIME && clock != CLOCK_MONOTONIC)
            || event.sigev_notify_function.is_none())
    {
        return Err(Errno(EINVAL));
    }
    if cpu && event.sigev_notify == SIGEV_THREAD && event.sigev_notify_function.is_none() {
        return Err(Errno(EINVAL));
    }
    // The clock of "the calling thread" is the one of the thread that
    // makes the timer, not of the helper that reads it.
    let clock = if clock == CLOCK_THREAD_CPUTIME_ID {
        let mut own: clockid_t = 0;
        if unsafe { pthread_getcpuclockid(pthread_self(), &mut own) } != 0 {
            return Err(Errno(EINVAL));
        }
        own
    } else {
        clock
    };
    let timer = Box::into_raw(Box::new(Timer {
        clock,
        cpu,
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
    let id = (timer as usize | TAG) as timer_t;
    // A signal with the number of the timer as its value (the default
    // notice) is a plain signal here.
    if unsafe { (*timer).event.sigev_notify } & 0xFF == SIGEV_SIGNAL {
        unsafe {
            if (*timer).event.sigev_notify & 0x100 != 0 {
                (*timer).event.sigev_value.sival_ptr = id.cast();
            }
            (*timer).event.sigev_notify = SIGEV_SIGNAL;
        }
    }
    let mut thread = MaybeUninit::<pthread_t>::uninit();
    // A timer on processor time must fire while threads of a real-time policy
    // keep the processor, so its helper runs at the highest priority when the
    // caller may give it that (the superuser), else as the caller does.
    let mut made = EPERM;
    if cpu {
        let mut attr = MaybeUninit::<pthread_attr_t>::uninit();
        unsafe {
            if pthread_attr_init(attr.as_mut_ptr()) == 0 {
                let param = sched_param { sched_priority: 32 };
                if pthread_attr_setinheritsched(attr.as_mut_ptr(), PTHREAD_EXPLICIT_SCHED) == 0
                    && pthread_attr_setschedpolicy(attr.as_mut_ptr(), SCHED_FIFO) == 0
                    && pthread_attr_setschedparam(attr.as_mut_ptr(), &param) == 0
                {
                    made = pthread_create(thread.as_mut_ptr(), attr.as_ptr(), helper, timer.cast());
                }
                pthread_attr_destroy(attr.as_mut_ptr());
            }
        }
    }
    if made != 0 {
        made = unsafe { pthread_create(thread.as_mut_ptr(), ptr::null(), helper, timer.cast()) };
    }
    if made != 0 {
        drop(unsafe { Box::from_raw(timer) });
        return Err(Errno(crate::header::errno::EAGAIN));
    }
    unsafe { pthread_detach(thread.assume_init()) };
    Ok(id)
}

fn remaining(timer: &Timer, inner: &Inner) -> itimerspec {
    let left = if inner.armed {
        inner
            .next
            .saturating_sub(now(if timer.cpu { timer.clock } else { CLOCK_MONOTONIC }))
    } else {
        0
    };
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
    // The helper looks at `gone` under this lock and frees the timer after
    // it, so the flag and the wake-up are made while the lock is held; the
    // timer is not touched after the lock is let go.
    let inner = timer.inner.lock();
    timer.gone.store(true, Ordering::Release);
    changed(timer);
    drop(inner);
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
    let mono = now(if timer.cpu { timer.clock } else { CLOCK_MONOTONIC });
    if value.it_value.tv_sec == 0 && value.it_value.tv_nsec == 0 {
        inner.armed = false;
    } else {
        let given = to_ns(&value.it_value);
        inner.next = if flags & TIMER_ABSTIME == 0 {
            mono.saturating_add(given)
        } else if timer.clock == CLOCK_MONOTONIC || timer.cpu {
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

/// The interval timers on processor time, ITIMER_VIRTUAL (1) and ITIMER_PROF
/// (2): one timer of this module for each, of the process that made it.
static ITIMERS: Mutex<[(c_int, usize); 2]> = Mutex::new([(0, 0); 2]);

/// Sets (`set`) or reads ITIMER_VIRTUAL or ITIMER_PROF (`which` 1 or 2): the
/// setting before, the time left and the interval in nanoseconds. The
/// timer sends SIGVTALRM or SIGPROF to the process when `value_ns` of the
/// time its threads ran have passed, and then every `interval_ns`.
pub fn interval_timer(which: c_int, set: bool, value_ns: u64, interval_ns: u64) -> Result<[u64; 2], Errno> {
    use crate::header::unistd::getpid;
    let index = which as usize - 1;
    let me = getpid();
    let mut table = ITIMERS.lock();
    let (owner, address) = table[index];
    // A child of fork has none of the helper threads.
    let existing = (owner == me && address != 0).then_some(address);
    let timer = match existing {
        Some(address) => address as *const Timer,
        None if !set => return Ok([0, 0]),
        None if value_ns == 0 => return Ok([0, 0]),
        None => {
            let mut event = unsafe { core::mem::zeroed::<sigevent>() };
            event.sigev_notify = SIGEV_SIGNAL;
            event.sigev_signo = if which == 1 { 26 } else { 27 };
            let id = create(CLOCK_PROCESS_CPUTIME_ID, &event)? as usize;
            table[index] = (me, id & !TAG);
            (id & !TAG) as *const Timer
        }
    };
    drop(table);
    let timer = unsafe { &*timer };
    let before = {
        let inner = timer.inner.lock();
        remaining(timer, &inner)
    };
    if set {
        let value = itimerspec {
            it_interval: to_timespec(interval_ns),
            it_value: to_timespec(value_ns),
        };
        let id = (timer as *const Timer as usize | TAG) as timer_t;
        settime(id, 0, &value);
    }
    Ok([to_ns(&before.it_value), to_ns(&before.it_interval)])
}
