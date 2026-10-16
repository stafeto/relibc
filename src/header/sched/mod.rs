//! `sched.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
//!
//! stafeto: the scheduling policies are SCHED_OTHER, SCHED_FIFO and SCHED_RR
//! of the process service's layer; the priorities of FIFO and RR run from 1
//! to 32, those of SCHED_OTHER are 0. SCHED_SPORADIC is not claimed.

use crate::{
    error::{Errno, ResultExt},
    header::{errno, time::timespec},
    platform::{
        Pal, Sys,
        types::{c_int, c_ulong, pid_t, size_t},
    },
};

// TODO: There are extensions, but adding more member is breaking ABI for pthread_attr_t
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
///
/// Scheduling parameters required for each supported scheduling policy.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct sched_param {
    /// Process or thread execution scheduling priority.
    pub sched_priority: c_int,
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
///
/// Another scheduling policy (the values are those of Linux).
pub const SCHED_OTHER: c_int = 0;
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
///
/// First in first out (FIFO) scheduling policy.
pub const SCHED_FIFO: c_int = 1;
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
///
/// Round robin scheduling policy.
pub const SCHED_RR: c_int = 2;

/// Sets errno and gives -1.
fn fail(error: c_int) -> c_int {
    crate::platform::ERRNO.set(error);
    -1
}

/// The bits of a CPU set (glibc's `CPU_SETSIZE`).
pub const CPU_SETSIZE: c_int = 1024;

/// A set of CPUs (glibc's layout: 1024 bits). stafeto runs on one processor:
/// the set of any thread is {0}.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct cpu_set_t {
    pub __bits: [c_ulong; 16],
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_get_priority_max.html>.
#[unsafe(no_mangle)]
pub extern "C" fn sched_get_priority_max(policy: c_int) -> c_int {
    Sys::sched_priority_range(policy, true).or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_get_priority_max.html>.
#[unsafe(no_mangle)]
pub extern "C" fn sched_get_priority_min(policy: c_int) -> c_int {
    Sys::sched_priority_range(policy, false).or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_getparam.html>.
///
/// # Safety
/// `param` points to a `sched_param`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_getparam(pid: pid_t, param: *mut sched_param) -> c_int {
    if param.is_null() {
        return fail(errno::EINVAL);
    }
    match Sys::sched_getparam(pid) {
        Ok(priority) => {
            unsafe { param.write(sched_param { sched_priority: priority }) };
            0
        }
        Err(Errno(n)) => fail(n),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_getscheduler.html>.
#[unsafe(no_mangle)]
pub extern "C" fn sched_getscheduler(pid: pid_t) -> c_int {
    Sys::sched_getscheduler(pid).or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_rr_get_interval.html>.
///
/// # Safety
/// `time` points to a `timespec`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_rr_get_interval(pid: pid_t, time: *mut timespec) -> c_int {
    if time.is_null() {
        return fail(errno::EINVAL);
    }
    match Sys::sched_getscheduler(pid) {
        // The quantum of the kernel's round-robin levels: 4 ms.
        Ok(_) => {
            unsafe {
                time.write(timespec {
                    tv_sec: 0,
                    tv_nsec: 4_000_000,
                })
            };
            0
        }
        Err(Errno(n)) => fail(n),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_setparam.html>.
///
/// # Safety
/// `param` points to a `sched_param`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_setparam(pid: pid_t, param: *const sched_param) -> c_int {
    if param.is_null() {
        return fail(errno::EINVAL);
    }
    let priority = unsafe { (*param).sched_priority };
    Sys::sched_set(pid, -1, priority).map(|_| 0).or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_setscheduler.html>.
///
/// The previous policy on success.
///
/// # Safety
/// `param` points to a `sched_param`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_setscheduler(
    pid: pid_t,
    policy: c_int,
    param: *const sched_param,
) -> c_int {
    if param.is_null() || policy < 0 {
        return fail(errno::EINVAL);
    }
    let priority = unsafe { (*param).sched_priority };
    Sys::sched_set(pid, policy, priority).or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_yield.html>.
///
/// Force the running thread to relinquish the processor until it again
/// becomes the head of its thread list.
#[unsafe(no_mangle)]
pub extern "C" fn sched_yield() -> c_int {
    Sys::sched_yield().map(|()| 0).or_minus_one_errno()
}

/// Linux's `sched_setaffinity`: one processor, so a mask that holds CPU 0
/// is the only one accepted.
///
/// # Safety
/// `mask` points to `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_setaffinity(
    pid: pid_t,
    size: size_t,
    mask: *const cpu_set_t,
) -> c_int {
    if mask.is_null() || size == 0 {
        return fail(errno::EFAULT);
    }
    if let Err(e) = Sys::sched_getscheduler(pid) {
        return fail(e.0);
    }
    // SAFETY: the caller's promise; the first byte holds CPU 0.
    if unsafe { *mask.cast::<u8>() } & 1 == 0 {
        return fail(errno::EINVAL);
    }
    0
}

/// Linux's `sched_getaffinity`: the set {0}.
///
/// # Safety
/// `mask` points to `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sched_getaffinity(
    pid: pid_t,
    size: size_t,
    mask: *mut cpu_set_t,
) -> c_int {
    if mask.is_null() || size == 0 {
        return fail(errno::EFAULT);
    }
    if let Err(e) = Sys::sched_getscheduler(pid) {
        return fail(e.0);
    }
    // SAFETY: the caller's promise.
    unsafe {
        core::ptr::write_bytes(mask.cast::<u8>(), 0, size);
        *mask.cast::<u8>() = 1;
    }
    0
}
