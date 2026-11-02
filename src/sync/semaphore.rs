// From https://www.remlab.net/op/futex-misc.shtml
//TODO: improve implementation

use crate::{
    error::{Errno, Result},
    header::{
        errno,
        time::{CLOCK_MONOTONIC, CLOCK_REALTIME, timespec, timespec_realtime_to_monotonic},
    },
    platform::{
        Pal, Sys,
        types::{c_uint, clockid_t},
    },
    sync::FutexAtomicTy,
};

use core::sync::atomic::{AtomicU32, Ordering};

// The semaphore lives in a `sem_t`.
const _: () = assert!(
    core::mem::size_of::<Semaphore>() <= core::mem::size_of::<crate::header::semaphore::sem_t>()
);

pub struct Semaphore {
    count: AtomicU32,
    /// The threads in a wait by address on `count` (stafeto: a post wakes one
    /// and only when there is one).
    waiters: AtomicU32,
}

impl Semaphore {
    pub const fn new(value: c_uint) -> Self {
        Self {
            count: AtomicU32::new(value),
            waiters: AtomicU32::new(0),
        }
    }

    // TODO: Acquire-Release ordering?

    pub fn post(&self, count: c_uint) {
        self.count.fetch_add(count, Ordering::SeqCst);
        // A unit wakes one waiter, and none when nobody waits; a waiter that
        // leaves without the unit (a deadline) passes the wake on.
        if self.waiters.load(Ordering::SeqCst) != 0 {
            crate::sync::futex_wake(&self.count, count.min(i32::MAX as c_uint) as i32);
        }
    }

    /// A woken waiter that leaves without taking its unit leaves it to the
    /// next one.
    fn pass_on(&self) {
        if self.count.load(Ordering::SeqCst) != 0 && self.waiters.load(Ordering::SeqCst) != 0 {
            crate::sync::futex_wake(&self.count, 1);
        }
    }

    pub fn try_wait(&self) -> bool {
        loop {
            let value = self.count.load(Ordering::SeqCst);

            if value == 0 {
                return false;
            }

            if self
                .count
                .compare_exchange_weak(value, value - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // Acquired
                return true;
            }
            // Try again (as long as value > 0)
        }
    }

    pub fn wait(&self, timeout_opt: Option<&timespec>, clock_id: clockid_t) -> Result<()> {
        loop {
            // A cancellation point (sem_wait, sem_timedwait): on stafeto a
            // request wakes the wait by address, which comes back here. A
            // wake that is meant for another waiter goes on before this
            // thread ends.
            #[cfg(stafeto)]
            unsafe {
                crate::pthread::testcancel_before(|| self.pass_on())
            };
            if self.try_wait() {
                return Ok(());
            }
            // value must be zero
            if let Some(timeout) = timeout_opt {
                // The wait would block: a time out of range is the error.
                if timeout.tv_nsec < 0 || timeout.tv_nsec >= 1_000_000_000 {
                    return Err(Errno(errno::EINVAL));
                }
                let relative = match clock_id {
                    // FUTEX expect monotonic clock
                    CLOCK_MONOTONIC => timeout.clone(),
                    CLOCK_REALTIME => timespec_realtime_to_monotonic(timeout)?,
                    _ => return Err(Errno(errno::EINVAL)),
                };
                self.waiters.fetch_add(1, Ordering::SeqCst);
                let result = unsafe { Sys::futex_wait(self.count.ptr(), 0, Some(&relative)) };
                self.waiters.fetch_sub(1, Ordering::SeqCst);
                if let Err(error) = Self::waited(result) {
                    self.pass_on();
                    return Err(error);
                }
            } else {
                // Use futex to wait for the next change, without a timeout
                self.waiters.fetch_add(1, Ordering::SeqCst);
                let result = unsafe { Sys::futex_wait(self.count.ptr(), 0, None) };
                self.waiters.fetch_sub(1, Ordering::SeqCst);
                if let Err(error) = Self::waited(result) {
                    self.pass_on();
                    return Err(error);
                }
            }
        }
    }
    /// What a wait by address says for the semaphore: a changed count
    /// (EAGAIN) means look again; an interruption by a handler (EINTR), the
    /// deadline (ETIMEDOUT) and real errors end the wait.
    fn waited(result: Result<()>) -> Result<()> {
        match result {
            Err(Errno(errno::EAGAIN)) => Ok(()),
            other => other,
        }
    }
    pub fn value(&self) -> c_uint {
        self.count.load(Ordering::SeqCst)
    }
}
