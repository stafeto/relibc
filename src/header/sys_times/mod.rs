//! `sys/times.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_times.h.html>.

use crate::platform::types::clock_t;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_times.h.html>.
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct tms {
    tms_utime: clock_t,
    tms_stime: clock_t,
    tms_cutime: clock_t,
    tms_cstime: clock_t,
}

/// Clock ticks a second (`sysconf(_SC_CLK_TCK)`).
const TICKS: i64 = 100;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/times.html>.
///
/// stafeto: the elapsed time on CLOCK_MONOTONIC in clock ticks; the CPU
/// time of the process and of its children that were waited for, in user time
/// and system time (the time in the kernel).
#[unsafe(no_mangle)]
pub extern "C" fn times(out: *mut tms) -> clock_t {
    if let Some(out) = unsafe { out.as_mut() } {
        let ticks = |ns: u64| (ns / (1_000_000_000 / TICKS as u64)) as clock_t;
        let split = |who| {
            crate::platform::sys::cpu_split_ns(who).map_or((0, 0), |(user, system)| {
                (ticks(user), ticks(system))
            })
        };
        let (utime, stime) = split(0);
        let (cutime, cstime) = split(-1);
        *out = tms {
            tms_utime: utime,
            tms_stime: stime,
            tms_cutime: cutime,
            tms_cstime: cstime,
        };
    }
    let mut now = crate::header::time::timespec::default();
    if unsafe { crate::header::time::clock_gettime(crate::header::time::CLOCK_MONOTONIC, &mut now) }
        != 0
    {
        return -1;
    }
    (now.tv_sec * TICKS + now.tv_nsec / (1_000_000_000 / TICKS)) as clock_t
}
