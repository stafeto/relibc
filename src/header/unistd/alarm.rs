use crate::{
    header::bits_timespec::timespec,
    platform::types::c_uint,
};
#[cfg(not(stafeto))]
use crate::{
    header::{
        signal::{SIGALRM, SIGEV_SIGNAL, sigevent},
        time::itimerspec,
    },
    platform::{
        Pal, Sys,
        types::{c_int, timer_t},
    },
    sync::Mutex,
};

/// Wrapper for timer_t that implements Send (the timer_t pointer is a process-
/// wide mmap'd allocation that outlives any single thread).
#[cfg(not(stafeto))]
struct AlarmTimer(timer_t);
// SAFETY: The timer_t pointer refers to an mmap'd RlctTimer that is
// only accessed under the ALARM_TIMER mutex lock.
#[cfg(not(stafeto))]
unsafe impl Send for AlarmTimer {}

/// Process-global singleton timer used by alarm(). Protected by a mutex to
/// ensure only one alarm is active at a time (POSIX requirement).
#[cfg(not(stafeto))]
static ALARM_TIMER: Mutex<Option<AlarmTimer>> = Mutex::new(None);

/// Internal helper that arms/disarms the process-global alarm timer.
/// Accepts a full timespec so sub-second timers (ualarm) can reuse this later.
/// Returns the number of seconds remaining on the previous alarm (rounded up),
/// or 0 if there was no previous alarm.
///
/// TODO: This implementation does not survive `exec()`. POSIX requires that a
/// pending alarm be preserved across exec (the timer continues counting down
/// in the new process image as i understand).
#[cfg(not(stafeto))]
pub fn alarm_timespec(duration: timespec) -> c_uint {
    let mut guard = ALARM_TIMER.lock();

    // Determine remaining time on any existing alarm
    let remaining = if let Some(ref alarm) = *guard {
        match Sys::timer_gettime(alarm.0) {
            Ok(cur) => {
                let secs = cur.it_value.tv_sec as c_uint;
                if cur.it_value.tv_nsec > 0 {
                    secs + 1 // POSIX: round up
                } else {
                    secs
                }
            }
            Err(_) => 0,
        }
    } else {
        0
    };

    let disarm = duration.tv_sec == 0 && duration.tv_nsec == 0;

    if disarm {
        // alarm(0): cancel any pending alarm
        if let Some(ref alarm) = *guard {
            let zero = itimerspec::default();
            let _ = Sys::timer_settime(alarm.0, 0, &zero, None);
        }
        return remaining;
    }

    // Lazily create the singleton timer if it doesn't exist yet
    let timer_id = if let Some(timer) = &*guard {
        timer.0
    } else {
        let mut evp = unsafe { core::mem::zeroed::<sigevent>() };
        evp.sigev_notify = SIGEV_SIGNAL;
        evp.sigev_signo = c_int::try_from(SIGALRM).expect("constant within c_int::MAX");
        let Ok(timer_id) = Sys::timer_create(crate::header::time::CLOCK_REALTIME, &evp) else {
            return remaining;
        };

        *guard = Some(AlarmTimer(timer_id));
        timer_id
    };
    drop(guard);

    // Arm the timer as a one-shot (no interval)
    let spec = itimerspec {
        it_value: duration,
        it_interval: timespec::default(),
    };
    let _ = Sys::timer_settime(timer_id, 0, &spec, None);

    remaining
}

/// stafeto: the alarm lives in the process service, which keeps a pending
/// alarm over `exec` as POSIX asks; it is the real-time interval timer of
/// `setitimer`. Arms it `duration` from now (zero cancels it) and returns
/// the seconds left of the alarm before, rounded up.
#[cfg(stafeto)]
#[expect(deprecated)]
pub fn alarm_timespec(duration: timespec) -> c_uint {
    use crate::{
        header::{sys_select::timeval, sys_time::itimerval},
        platform::{PalSignal, Sys},
    };
    let new = itimerval {
        it_interval: timeval::default(),
        it_value: timeval {
            tv_sec: duration.tv_sec,
            tv_usec: (duration.tv_nsec / 1000) as _,
        },
    };
    let mut old = itimerval::default();
    if Sys::setitimer(0, &new, Some(&mut old)).is_err() {
        return 0;
    }
    let secs = old.it_value.tv_sec as c_uint;
    if old.it_value.tv_usec > 0 { secs + 1 } else { secs }
}
