//! `sched.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
//!
//! stafeto: the scheduling policies are SCHED_OTHER, SCHED_FIFO and SCHED_RR
//! of the process service's layer; the priorities of FIFO and RR run from 1
//! to 32, those of SCHED_OTHER are 0. SCHED_SPORADIC (4) is the sporadic server of the layer: the
//! members of `sched_param` give its budget, period, low priority and the number
//! of replenishments.

use crate::{
    error::{Errno, ResultExt},
    header::{errno, time::timespec},
    platform::{
        Pal, Sys,
        types::{c_int, c_ulong, pid_t, size_t},
    },
};

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
///
/// Scheduling parameters required for each supported scheduling policy: the
/// priority, and the members of the sporadic server option (48 bytes on
/// AArch64). The policies SCHED_OTHER, SCHED_FIFO and SCHED_RR read the
/// priority alone. `pthread_attr_t` and `posix_spawnattr_t` (96 bytes each,
/// 16 of them reserved) hold the whole structure.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct sched_param {
    /// Process or thread execution scheduling priority.
    pub sched_priority: c_int,
    /// Low scheduling priority for a sporadic server.
    pub sched_ss_low_priority: c_int,
    /// Replenishment period of a sporadic server.
    pub sched_ss_repl_period: timespec,
    /// Initial budget of a sporadic server.
    pub sched_ss_init_budget: timespec,
    /// Maximum pending replenishments of a sporadic server.
    pub sched_ss_max_repl: c_int,
}

impl sched_param {
    /// A parameter structure with this priority and every other member zero.
    pub const fn with_priority(sched_priority: c_int) -> Self {
        sched_param {
            sched_priority,
            sched_ss_low_priority: 0,
            sched_ss_repl_period: timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            sched_ss_init_budget: timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            sched_ss_max_repl: 0,
        }
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<sched_param>() == 48);

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
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sched.h.html>.
///
/// Sporadic server scheduling policy (the number Linux leaves free).
pub const SCHED_SPORADIC: c_int = 4;

/// Sets errno and gives -1.
fn fail(error: c_int) -> c_int {
    crate::platform::ERRNO.set(error);
    -1
}

/// The bits of a CPU set (glibc's `CPU_SETSIZE`).
pub const CPU_SETSIZE: c_int = 1024;

/// A set of CPUs (glibc's layout: 1024 bits).
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
        Ok(got) => {
            // SAFETY: the caller's promise: `param` points to a `sched_param`.
            unsafe { param.write(got) };
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
    // SAFETY: `param` is not null and, by the caller's promise, points to a `sched_param`.
    let param = unsafe { &*param };
    Sys::sched_set(pid, -1, param).map(|_| 0).or_minus_one_errno()
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
    // SAFETY: `param` is not null and, by the caller's promise, points to a `sched_param`.
    let param = unsafe { &*param };
    Sys::sched_set(pid, policy, param).or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sched_yield.html>.
///
/// Force the running thread to relinquish the processor until it again
/// becomes the head of its thread list.
#[unsafe(no_mangle)]
pub extern "C" fn sched_yield() -> c_int {
    Sys::sched_yield().map(|()| 0).or_minus_one_errno()
}

/// Linux's `sched_setaffinity` on stafeto: the layer writes the mask to the
/// process's page. The threads the process makes later and the image of its
/// next exec go on the CPUs the mask names; a thread that runs stays on its
/// CPU. EINVAL when the mask names no CPU that is up, ESRCH for a process
/// that does not exist, EPERM for another process.
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
    Sys::sched_setaffinity(pid, size, mask.cast()).map(|_| 0).or_minus_one_errno()
}

/// Linux's `sched_getaffinity` on stafeto: the CPUs the process may use, all
/// that are up unless it set a mask. The bytes past the CPUs are zeros;
/// EINVAL when `size` holds fewer bits than there are CPUs.
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
    Sys::sched_getaffinity(pid, size, mask.cast()).map(|_| 0).or_minus_one_errno()
}

/// glibc's `sched_getcpu`: the number of the CPU the calling thread runs on.
#[unsafe(no_mangle)]
pub extern "C" fn sched_getcpu() -> c_int {
    Sys::cpu_current()
}
