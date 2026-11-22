#[cfg(stafeto)]
use crate::platform::types::c_int;
use crate::platform::types::c_long;

/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man3/getopt.3.html>.
#[cfg(not(stafeto))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn syscall(sysno: c_long, mut args: ...) -> c_long {
    let a1 = unsafe { args.next_arg::<usize>() };
    let a2 = unsafe { args.next_arg::<usize>() };
    let a3 = unsafe { args.next_arg::<usize>() };
    let a4 = unsafe { args.next_arg::<usize>() };
    let a5 = unsafe { args.next_arg::<usize>() };
    let a6 = unsafe { args.next_arg::<usize>() };

    (unsafe { sc::syscall6(sysno as usize, a1, a2, a3, a4, a5, a6) }) as c_long
}

/// Linux AArch64 number of `futex`.
#[cfg(stafeto)]
const SYS_FUTEX: c_long = 98;
#[cfg(stafeto)]
const FUTEX_WAIT: c_int = 0;
#[cfg(stafeto)]
const FUTEX_WAKE: c_int = 1;
#[cfg(stafeto)]
const FUTEX_PRIVATE_FLAG: c_int = 128;

/// Non-POSIX. stafeto has no Linux system calls: every number fails with
/// ENOSYS, with one exception. `futex` (98) with `FUTEX_WAIT` or `FUTEX_WAKE`,
/// private or not, goes to the layer's waits by address: libc++'s
/// `std::atomic::wait`, `std::latch`, `std::barrier` and `std::counting_semaphore`
/// sleep on it. The timeout of a wait is relative, as on Linux; a wait that
/// the address's value refuses fails with EAGAIN, one that ends at its timeout
/// with ETIMEDOUT. A null address fails with EFAULT, a misaligned one and a
/// timeout with a negative field or a nanosecond count outside [0, 1e9) with
/// EINVAL. Other operations (requeue, bit sets, priority inheritance) and the
/// flag `FUTEX_CLOCK_REALTIME` (the timeout stays relative on the monotonic
/// clock) fail with ENOSYS.
#[cfg(stafeto)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn syscall(sysno: c_long, mut args: ...) -> c_long {
    use crate::{
        error::Errno,
        header::{
            errno::{EFAULT, EINVAL, ENOSYS},
            time::{CLOCK_MONOTONIC, timespec},
        },
        out::Out,
        platform::{ERRNO, Pal, Sys},
    };
    use core::mem::MaybeUninit;

    if sysno != SYS_FUTEX {
        ERRNO.set(ENOSYS);
        return -1;
    }
    // SAFETY: the caller passes the arguments of futex: an address, an
    // operation and a value. The timeout, the fourth, only a wait reads.
    let (addr, op, val) = unsafe {
        (
            args.next_arg::<*mut u32>(),
            args.next_arg::<c_int>(),
            args.next_arg::<u32>(),
        )
    };
    if addr.is_null() {
        ERRNO.set(EFAULT);
        return -1;
    }
    if (addr as usize) % 4 != 0 {
        ERRNO.set(EINVAL);
        return -1;
    }
    match op & !FUTEX_PRIVATE_FLAG {
        FUTEX_WAIT => {
            // SAFETY: a wait passes a fourth argument, the timeout.
            let timeout = unsafe { args.next_arg::<*const timespec>() };
            // The layer's deadline is absolute on CLOCK_MONOTONIC.
            let deadline = if timeout.is_null() {
                None
            } else {
                // SAFETY: a non-null timeout points to a timespec.
                let relative = unsafe { *timeout };
                if relative.tv_sec < 0 || relative.tv_nsec < 0 || relative.tv_nsec >= 1_000_000_000
                {
                    ERRNO.set(EINVAL);
                    return -1;
                }
                let mut now = MaybeUninit::<timespec>::uninit();
                if let Err(Errno(e)) =
                    Sys::clock_gettime(CLOCK_MONOTONIC, Out::from_uninit_mut(&mut now))
                {
                    ERRNO.set(e);
                    return -1;
                }
                // SAFETY: clock_gettime filled it.
                let now = unsafe { now.assume_init() };
                let mut sec = now.tv_sec.saturating_add(relative.tv_sec);
                let mut nsec = now.tv_nsec + relative.tv_nsec;
                if nsec >= 1_000_000_000 {
                    nsec -= 1_000_000_000;
                    sec = sec.saturating_add(1);
                }
                Some(timespec {
                    tv_sec: sec,
                    tv_nsec: nsec,
                })
            };
            match unsafe { Sys::futex_wait(addr, val, deadline.as_ref()) } {
                Ok(()) => 0,
                Err(Errno(e)) => {
                    ERRNO.set(e);
                    -1
                }
            }
        }
        FUTEX_WAKE => match unsafe { Sys::futex_wake(addr, val) } {
            Ok(woken) => woken as c_long,
            Err(Errno(e)) => {
                ERRNO.set(e);
                -1
            }
        },
        _ => {
            ERRNO.set(ENOSYS);
            -1
        }
    }
}
