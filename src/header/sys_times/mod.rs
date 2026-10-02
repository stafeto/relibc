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
/// stafeto: the elapsed time on CLOCK_MONOTONIC in clock ticks; the layer
/// keeps no CPU time of a process yet, so the four times are 0.
#[unsafe(no_mangle)]
pub extern "C" fn times(out: *mut tms) -> clock_t {
    if let Some(out) = unsafe { out.as_mut() } {
        *out = tms {
            tms_utime: 0,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
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
