//! `float.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/float.h.html>.

#[cfg(not(all(stafeto, target_arch = "aarch64")))]
use crate::header::_fenv::{FE_TONEAREST, fegetround};
use crate::platform::types::c_int;

#[cfg(all(stafeto, target_arch = "aarch64"))]
unsafe extern "C" {
    fn fegetround() -> c_int;
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/float.h.html>.
pub const FLT_RADIX: c_int = 2;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/float.h.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flt_rounds() -> c_int {
    #[cfg(all(stafeto, target_arch = "aarch64"))]
    return match unsafe { fegetround() } {
        // OpenLibm's AArch64 FE_* values map to C's FLT_ROUNDS values.
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 0,
        _ => -1,
    };
    #[cfg(not(all(stafeto, target_arch = "aarch64")))]
    match unsafe { fegetround() } {
        FE_TONEAREST => 1,
        _ => -1,
    }
}
