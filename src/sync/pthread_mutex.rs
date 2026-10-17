use core::{
    cell::Cell,
    ptr,
    sync::atomic::{AtomicU8, AtomicU32 as AtomicUint, Ordering},
};

use crate::{
    error::Errno,
    header::{errno::*, pthread::*, time::timespec},
};

use crate::platform::{Pal, Sys, types::c_int};

pub struct RlctMutex {
    // Actual locking word.
    inner: AtomicUint,
    recursive_count: AtomicUint,

    ty: Ty,
    /// PTHREAD_PRIO_NONE, PTHREAD_PRIO_INHERIT or PTHREAD_PRIO_PROTECT in
    /// the low bits; `ROBUST` for a robust mutex, and for one, `OWNER_DEAD`
    /// from the end of its owner until pthread_mutex_consistent, and
    /// `NOT_RECOVERABLE` once an owner unlocked it without that call.
    protocol: AtomicU8,
    /// The priority ceiling of PTHREAD_PRIO_PROTECT, 1 to 32. The ceiling
    /// is read when a thread starts to lock and written by
    /// pthread_mutex_setprioceiling while it holds the mutex.
    prioceiling: AtomicU8,
    /// The ceiling at which the owner entered the protocol: the owner
    /// leaves at it even when the ceiling was replaced while it waited.
    entered: AtomicU8,
}

const ROBUST: u8 = 0x80;
const NOT_RECOVERABLE: u8 = 0x40;
const OWNER_DEAD: u8 = 0x20;
const PROTOCOL_MASK: u8 = 0x0f;

/// The robust mutexes this thread holds, which its end marks as having an
/// owner that died (stafeto: threads end inside the process, so a process
/// that ends with robust mutexes held leaves them to the process that shares
/// them, if any, locked).
const HELD_MAX: usize = 64;
#[thread_local]
static HELD: [Cell<*const RlctMutex>; HELD_MAX] = [const { Cell::new(ptr::null()) }; HELD_MAX];
#[thread_local]
static HELD_COUNT: Cell<usize> = Cell::new(0);

/// The end of the calling thread: each robust mutex it holds is unlocked
/// and says EOWNERDEAD to the next thread that locks it.
pub fn release_held_robust() {
    for index in 0..HELD_COUNT.get() {
        let mutex = HELD[index].replace(ptr::null());
        if !mutex.is_null() {
            let mutex = unsafe { &*mutex };
            mutex.protocol.fetch_or(OWNER_DEAD, Ordering::Relaxed);
            mutex.recursive_count.store(0, Ordering::Relaxed);
            mutex.release_word();
        }
    }
    HELD_COUNT.set(0);
}

const STATE_UNLOCKED: u32 = 0;
const WAITING_BIT: u32 = 1 << 31;
const INDEX_MASK: u32 = !WAITING_BIT;

// TODO: Lower limit is probably better.
const RECURSIVE_COUNT_MAX_INCLUSIVE: u32 = u32::MAX;
// TODO: How many spins should we do before it becomes more time-economical to enter kernel mode
// via futexes?
const SPIN_COUNT: usize = 0;

impl RlctMutex {
    pub(crate) const fn new(attr: &RlctMutexAttr) -> Result<Self, Errno> {
        let RlctMutexAttr {
            prioceiling,
            protocol,
            pshared,
            robust,
            ty,
        } = *attr;

        // Limit of this implementation: a PTHREAD_PRIO_INHERIT mutex shared
        // between processes would lend a level to a thread of another
        // process, which the layer does not do, so pthread_mutex_init says
        // ENOTSUP. PTHREAD_PRIO_PROTECT needs no knowledge of the owner and
        // works between processes.
        if protocol == PTHREAD_PRIO_INHERIT && pshared == PTHREAD_PROCESS_SHARED {
            return Err(Errno(ENOTSUP));
        }
        match protocol {
            PTHREAD_PRIO_NONE | PTHREAD_PRIO_INHERIT => (),
            PTHREAD_PRIO_PROTECT if prioceiling >= 1 && prioceiling <= 32 => (),
            _ => return Err(Errno(EINVAL)),
        }

        Ok(Self {
            protocol: AtomicU8::new(
                protocol as u8
                    | match robust {
                        PTHREAD_MUTEX_STALLED => 0,
                        PTHREAD_MUTEX_ROBUST => ROBUST,

                        _ => return Err(Errno(EINVAL)),
                    },
            ),
            prioceiling: AtomicU8::new(prioceiling as u8),
            entered: AtomicU8::new(0),
            inner: AtomicUint::new(STATE_UNLOCKED),
            recursive_count: AtomicUint::new(0),
            ty: match ty {
                PTHREAD_MUTEX_DEFAULT => Ty::Def,
                PTHREAD_MUTEX_ERRORCHECK => Ty::Errck,
                PTHREAD_MUTEX_RECURSIVE => Ty::Recursive,
                PTHREAD_MUTEX_NORMAL => Ty::Normal,

                _ => return Err(Errno(EINVAL)),
            },
        })
    }
    fn protocol(&self) -> i32 {
        i32::from(self.protocol.load(Ordering::Relaxed) & PROTOCOL_MASK)
    }
    fn robust(&self) -> bool {
        self.protocol.load(Ordering::Relaxed) & ROBUST != 0
    }
    /// A robust mutex just locked: it joins the mutexes the thread holds.
    /// EOWNERDEAD tells the new owner that the previous one ended holding
    /// it, ENOTRECOVERABLE (the mutex unlocked again) that nobody repaired it.
    fn acquired(&self) -> Result<(), Errno> {
        if !self.robust() {
            return Ok(());
        }
        let state = self.protocol.load(Ordering::Relaxed);
        if state & NOT_RECOVERABLE != 0 {
            self.recursive_count.store(0, Ordering::Relaxed);
            self.release_word();
            return Err(Errno(ENOTRECOVERABLE));
        }
        let count = HELD_COUNT.get();
        if count == HELD_MAX {
            self.recursive_count.store(0, Ordering::Relaxed);
            self.release_word();
            return Err(Errno(EAGAIN));
        }
        HELD[count].set(self);
        HELD_COUNT.set(count + 1);
        if state & OWNER_DEAD != 0 {
            return Err(Errno(EOWNERDEAD));
        }
        Ok(())
    }
    /// The mutex leaves the mutexes the thread holds.
    fn forget(&self) {
        let count = HELD_COUNT.get();
        for index in (0..count).rev() {
            if ptr::eq(HELD[index].get(), self) {
                let last = HELD[count - 1].replace(ptr::null());
                if index != count - 1 {
                    HELD[index].set(last);
                }
                HELD_COUNT.set(count - 1);
                return;
            }
        }
    }
    /// The locking word becomes free and one waiter wakes.
    fn release_word(&self) {
        // One waiter at a time: it takes the lock with WAITING_BIT, so the
        // next unlock wakes the next one. With PTHREAD_PRIO_INHERIT all
        // wake: those that lose the race wait for the new owner and lend it
        // their level, and this thread's level, raised for them, is worked
        // out again.
        if self.inner.swap(STATE_UNLOCKED, Ordering::Release) & WAITING_BIT != 0 {
            if self.protocol() == PTHREAD_PRIO_INHERIT {
                unsafe { crate::platform::sys::futex_wake_pi(self.inner.as_ptr(), u32::MAX) };
            } else {
                crate::sync::futex_wake(&self.inner, 1);
            }
        }
    }
    /// pthread_mutex_getprioceiling: the ceiling of the mutex.
    pub fn prioceiling(&self) -> Result<c_int, Errno> {
        if self.protocol() != PTHREAD_PRIO_PROTECT {
            return Err(Errno(EINVAL));
        }
        Ok(c_int::from(self.prioceiling.load(Ordering::Relaxed)))
    }
    /// pthread_mutex_setprioceiling: locks the mutex by the rule of the
    /// protocol at the old ceiling, sets the new ceiling and unlocks it at
    /// the same old ceiling; the new ceiling applies from the next lock. The
    /// old ceiling comes back.
    pub fn replace_prioceiling(&self, ceiling: c_int) -> Result<c_int, Errno> {
        if self.protocol() != PTHREAD_PRIO_PROTECT || !(1..=32).contains(&ceiling) {
            return Err(Errno(EINVAL));
        }
        self.lock()?;
        let old = self.prioceiling.swap(ceiling as u8, Ordering::Relaxed);
        self.unlock()?;
        Ok(c_int::from(old))
    }
    /// pthread_mutex_consistent: the state the mutex protects is repaired.
    pub fn make_consistent(&self) -> Result<(), Errno> {
        if !self.robust() || self.protocol.load(Ordering::Relaxed) & OWNER_DEAD == 0 {
            return Err(Errno(EINVAL));
        }
        self.protocol.fetch_and(!OWNER_DEAD, Ordering::Relaxed);
        Ok(())
    }
    /// Locks, waiting until `deadline` on CLOCK_REALTIME if one is given.
    /// The deadline is looked at only when the lock is taken: a mutex free
    /// now is locked whatever the deadline (POSIX).
    fn lock_inner(&self, deadline: Option<&timespec>) -> Result<(), Errno> {
        let this_thread = os_tid_invalid_after_fork();

        // A thread that waited takes the lock with WAITING_BIT set: other
        // threads may still wait, and `unlock` wakes one of them only when
        // the bit is set. So each unlock wakes one waiter and none is lost.
        let mut waited = 0;

        loop {
            let result = self.inner.compare_exchange_weak(
                STATE_UNLOCKED,
                this_thread | waited,
                Ordering::Acquire,
                Ordering::Relaxed,
            );

            match result {
                // CAS succeeded
                Ok(_) => {
                    if self.ty == Ty::Recursive {
                        self.increment_recursive_count()?;
                    }
                    return self.acquired();
                }
                // CAS failed, but the mutex was recursive and we already own the lock.
                Err(thread) if thread & INDEX_MASK == this_thread && self.ty == Ty::Recursive => {
                    self.increment_recursive_count()?;
                    return Ok(());
                }
                // CAS failed, but the mutex was error-checking and we already own the lock.
                Err(thread) if thread & INDEX_MASK == this_thread && self.ty == Ty::Errck => {
                    return Err(Errno(EDEADLK));
                }
                // CAS spuriously failed, simply retry the CAS. TODO: Use core::hint::spin_loop()?
                Err(thread) if thread & INDEX_MASK == 0 => {
                    continue;
                }
                // Some other thread owns the lock and nobody marked waiting
                // yet: mark it, then wait.
                Err(thread) if thread & WAITING_BIT == 0 => {
                    let _ = self.inner.compare_exchange_weak(
                        thread,
                        thread | WAITING_BIT,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                    continue;
                }
                // CAS failed because some other thread owned the lock. We must now wait.
                Err(thread) => {
                    /*if spins_left > 0 {
                        // TODO: Faster to spin trying to load the flag, compared to CAS?
                        spins_left -= 1;
                        core::hint::spin_loop();
                        continue;
                    }

                    spins_left = SPIN_COUNT;

                    let inner = self.inner.fetch_or(WAITING_BIT, Ordering::Relaxed);

                    if inner == STATE_UNLOCKED {
                        continue;
                    }*/

                    // If the mutex is not robust, simply futex_wait until unblocked.
                    //crate::sync::futex_wait(&self.inner, inner | WAITING_BIT, None);
                    let monotonic = match deadline {
                        Some(deadline) if !(0..1_000_000_000).contains(&deadline.tv_nsec) => {
                            return Err(Errno(EINVAL));
                        }
                        Some(deadline) => Some(
                            crate::header::time::timespec_realtime_to_monotonic(deadline)?,
                        ),
                        None => None,
                    };
                    // A wait that timed out goes round again: the deadline
                    // is on the calendar, which may have stepped back while
                    // the thread waited; the conversion above says
                    // ETIMEDOUT once the calendar passed it (POSIX: the new
                    // value of CLOCK_REALTIME applies to absolute waits).
                    if self.protocol() == PTHREAD_PRIO_INHERIT && thread & INDEX_MASK != this_thread {
                        // The level of this thread goes to the owner, and
                        // along the chain of what the owner waits for. A
                        // cycle of owners is a deadlock.
                        match unsafe {
                            crate::platform::sys::futex_wait_pi(
                                self.inner.as_ptr(),
                                thread,
                                monotonic.as_ref(),
                                thread & INDEX_MASK,
                            )
                        } {
                            Err(Errno(EDEADLK)) => return Err(Errno(EDEADLK)),
                            Err(Errno(ETIMEDOUT)) if monotonic.is_some() => (),
                            _ => (),
                        }
                    } else {
                        let _ = crate::sync::futex_wait(&self.inner, thread, monotonic.as_ref());
                    }
                    waited = WAITING_BIT;
                }
            }
        }
    }
    /// PTHREAD_PRIO_PROTECT: the thread runs at least at the ceiling from
    /// before it locks until it unlocks (EINVAL for a thread above it).
    /// Returns the ceiling it entered at.
    fn protect_enter(&self) -> Result<u8, Errno> {
        if self.protocol() == PTHREAD_PRIO_PROTECT {
            let ceiling = self.prioceiling.load(Ordering::Relaxed);
            crate::platform::sys::protect_enter(c_int::from(ceiling))?;
            return Ok(ceiling);
        }
        Ok(0)
    }
    fn protect_leave(&self, entered: u8) {
        if entered != 0 {
            crate::platform::sys::protect_leave(c_int::from(entered));
        }
    }
    /// The owner takes the mutex: it leaves the protocol at the ceiling it
    /// entered at, which the mutex keeps while it is held.
    fn locked(&self, entered: u8) {
        if entered != 0 {
            self.entered.store(entered, Ordering::Relaxed);
        }
    }
    pub fn lock(&self) -> Result<(), Errno> {
        let entered = self.protect_enter()?;
        let locked = self.lock_inner(None);
        match locked {
            Ok(()) | Err(Errno(EOWNERDEAD)) => self.locked(entered),
            Err(_) => self.protect_leave(entered),
        }
        locked
    }
    /// `lock` until `deadline`, an absolute time on CLOCK_REALTIME.
    pub fn lock_with_timeout(&self, deadline: &timespec) -> Result<(), Errno> {
        let entered = self.protect_enter()?;
        let locked = self.lock_inner(Some(deadline));
        match locked {
            Ok(()) | Err(Errno(EOWNERDEAD)) => self.locked(entered),
            Err(_) => self.protect_leave(entered),
        }
        locked
    }
    fn increment_recursive_count(&self) -> Result<(), Errno> {
        // We don't have to worry about asynchronous signals here, since pthread_mutex_trylock
        // is not async-signal-safe.
        //
        // TODO: Maybe just use Cell? Send/Sync doesn't matter much anyway, and will be
        // protected by the lock itself anyway.

        let prev_recursive_count = self.recursive_count.load(Ordering::Relaxed);

        if prev_recursive_count == RECURSIVE_COUNT_MAX_INCLUSIVE {
            return Err(Errno(EAGAIN));
        }

        self.recursive_count
            .store(prev_recursive_count + 1, Ordering::Relaxed);

        Ok(())
    }
    pub fn try_lock(&self) -> Result<(), Errno> {
        let entered = self.protect_enter()?;
        let locked = self.try_lock_inner();
        match locked {
            Ok(()) | Err(Errno(EOWNERDEAD)) => self.locked(entered),
            Err(_) => self.protect_leave(entered),
        }
        locked
    }
    fn try_lock_inner(&self) -> Result<(), Errno> {
        let this_thread = os_tid_invalid_after_fork();

        // TODO: If recursive, omitting CAS may be faster if it is already owned by this thread.
        let result = self.inner.compare_exchange(
            STATE_UNLOCKED,
            this_thread,
            Ordering::Acquire,
            Ordering::Relaxed,
        );

        if self.ty == Ty::Recursive {
            match result {
                Err(index) if index & INDEX_MASK != this_thread => return Err(Errno(EBUSY)),
                _ => (),
            }

            self.increment_recursive_count()?;

            return if result.is_ok() { self.acquired() } else { Ok(()) };
        }

        match result {
            Ok(_) => self.acquired(),
            Err(_) => Err(Errno(EBUSY)),
        }
    }
    // Safe because we are not protecting any data.
    pub fn unlock(&self) -> Result<(), Errno> {
        if self.robust() || matches!(self.ty, Ty::Recursive | Ty::Errck) {
            if self.inner.load(Ordering::Relaxed) & INDEX_MASK != os_tid_invalid_after_fork() {
                return Err(Errno(EPERM));
            }

            // TODO: Is this fence correct?
            core::sync::atomic::fence(Ordering::Acquire);
        }

        if self.ty == Ty::Recursive {
            let next = self.recursive_count.load(Ordering::Relaxed) - 1;
            self.recursive_count.store(next, Ordering::Relaxed);

            if next > 0 {
                // Each lock of a recursive mutex entered once.
                self.protect_leave(self.entered.load(Ordering::Relaxed));
                return Ok(());
            }
        }
        let entered = if self.protocol() == PTHREAD_PRIO_PROTECT {
            self.entered.swap(0, Ordering::Relaxed)
        } else {
            0
        };

        if self.robust() {
            self.forget();
            if self.protocol.load(Ordering::Relaxed) & OWNER_DEAD != 0 {
                // Unlocked before pthread_mutex_consistent: nobody can use it.
                self.protocol.fetch_or(NOT_RECOVERABLE, Ordering::Relaxed);
            }
        }
        self.release_word();
        self.protect_leave(entered);

        Ok(())
    }
}

#[repr(u8)]
#[derive(PartialEq)]
enum Ty {
    // The only difference between PTHREAD_MUTEX_NORMAL and PTHREAD_MUTEX_DEFAULT appears to be
    // that "normal" mutexes deadlock if locked multiple times on the same thread, whereas
    // "default" mutexes are UB in that case. So we can treat them as being the same type.
    Normal,
    Def,

    Errck,
    Recursive,
}

// Children after fork can only call async-signal-safe functions until they exec.
#[thread_local]
static CACHED_OS_TID_INVALID_AFTER_FORK: Cell<u32> = Cell::new(0);

// Assumes TIDs are unique between processes, which I only know is true for Redox.
fn os_tid_invalid_after_fork() -> u32 {
    // TODO: Coordinate better if using shared == PTHREAD_PROCESS_SHARED, with up to 2^32 separate
    // threads within possibly distinct processes, using the mutex. OS thread IDs on Redox are
    // pointer-sized, but relibc and POSIX uses int everywhere.

    let value = CACHED_OS_TID_INVALID_AFTER_FORK.get();

    if value == 0 {
        let tid = Sys::gettid();

        assert_ne!(tid, -1, "failed to obtain current thread ID");

        CACHED_OS_TID_INVALID_AFTER_FORK.set(tid.cast_unsigned());

        tid.cast_unsigned()
    } else {
        value
    }
}
