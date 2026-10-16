use core::sync::atomic::{AtomicI32 as AtomicInt, Ordering};

use crate::{
    header::errno::EBUSY,
    platform::types::{c_int, pthread_spinlock_t},
};

/// The spin lock is in an unlocked state.
const UNLOCKED: c_int = 0;
/// The spin lock is in a locked state.
const LOCKED: c_int = 1;
/// The spin lock is locked and a thread waits for it by address.
const WAITED: c_int = 2;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_spin_destroy.html>.
///
/// Destroys the spin lock referenced by `lock` and releases any resources used
/// by the lock.
///
/// Upon success, returns `0`. Upon failure, an error number is returned.
///
/// # Implementation
/// Cannot fail on the Rust side so no error number is ever returned.
///
/// # Safety
/// It is undefined behaviour for any of the following:
/// - Subsequent use of `lock` after calling this function unless it is
///   reinitialized with `pthread_spin_init()`.
/// - This function is called when a thread holds the lock.
/// - `lock` is uninitialized when this function is called.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_spin_destroy(lock: *mut pthread_spinlock_t) -> c_int {
    let _spinlock = unsafe { &mut *lock.cast::<RlctSpinlock>() };

    // No-op
    0
}
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_spin_init.html>.
///
/// Allocates any resources required to use the spin lock referenced to by
/// `lock` and initializes the lock to an unlocked state.
///
/// Upon success, returns `0`. Upon failure, an error number is returned.
///
/// # Implementation
/// Cannot fail on the Rust side so no error number is ever returned.
///
/// # Safety
/// It is undefined behaviour for any of the following:
/// - `lock` has already been initialized.
/// - `lock` has been used before being initialized by this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_spin_init(
    lock: *mut pthread_spinlock_t,
    _pshared: c_int,
) -> c_int {
    // TODO: pshared doesn't matter in most situations, as memory is just memory, but this may be
    // different on some architectures...

    unsafe {
        lock.cast::<RlctSpinlock>().write(RlctSpinlock {
            inner: AtomicInt::new(UNLOCKED),
        })
    };

    0
}
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_spin_lock.html>.
///
/// Locks the spin lock referenced by `lock`.
///
/// Upon success, returns `0`. Upon failure, an error number is returned.
///
/// # Safety
/// It is undefined behaviour for any of the following:
/// - `lock` is uninitialized.
/// - The calling thread holds `lock` at the time the call is made.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_spin_lock(lock: *mut pthread_spinlock_t) -> c_int {
    let spinlock = unsafe { &*lock.cast::<RlctSpinlock>() };

    // stafeto runs on one processor: the holder cannot make progress while
    // this thread spins, and under SCHED_FIFO a holder of a lower priority
    // never would. A few turns for a holder that is about to let go, then
    // the thread waits by address and the unlock wakes it.
    let mut turns = 0u32;
    loop {
        match spinlock.inner.compare_exchange_weak(
            UNLOCKED,
            LOCKED,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(_) if turns < 4 => {
                turns += 1;
                core::hint::spin_loop();
            }
            Err(_) => {
                // Waiting: state 2. The thread that finds the lock free
                // takes it in that state, so that its unlock wakes the next.
                if spinlock.inner.swap(WAITED, Ordering::Acquire) == UNLOCKED {
                    break;
                }
                crate::sync::futex_wait(&spinlock.inner, WAITED, None);
            }
        }
    }

    0
}
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_spin_trylock.html>.
///
/// Locks the spin lock referenced by `lock` if it is not held by any thread.
///
/// Upon success, returns `0`. Upon failure, an error number is returned.
///
/// # Safety
/// It is undefined behaviour if `lock` is uninitialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_spin_trylock(lock: *mut pthread_spinlock_t) -> c_int {
    let spinlock = unsafe { &*lock.cast::<RlctSpinlock>() };

    match spinlock
        .inner
        .compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
    {
        Ok(_) => (),
        Err(_) => return EBUSY,
    }

    0
}
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_spin_unlock.html>.
///
/// Releases the spin lock referenced by `lock` which was locked via the
/// `pthread_spin_lock()` or `pthread_spin_trylock()` functions.
///
/// Upon success, returns `0`. Upon failure, an error number is returned.
///
/// # Implementation
/// Cannot fail on the Rust side so no error number is ever returned.
///
/// # Safety
/// It is undefined behaviour for any of the following:
/// - `lock` is uninitialized.
/// - `lock` is not held by the calling thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_spin_unlock(lock: *mut pthread_spinlock_t) -> c_int {
    let spinlock = unsafe { &*lock.cast::<RlctSpinlock>() };

    if spinlock.inner.swap(UNLOCKED, Ordering::Release) == WAITED {
        crate::sync::futex_wake(&spinlock.inner, 1);
    }

    0
}

pub(crate) struct RlctSpinlock {
    pub inner: AtomicInt,
}
