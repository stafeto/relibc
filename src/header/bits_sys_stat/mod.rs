#[allow(unused_imports)]
use crate::platform::types::{c_int, c_long, c_longlong};

/// Used for file block counts.
#[allow(non_camel_case_types)]
pub type blkcnt_t = c_longlong;

/// Used for block sizes.
#[allow(non_camel_case_types)]
#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
pub type blksize_t = c_long;

/// Used for block sizes: 32 bits on AArch64 Linux.
#[allow(non_camel_case_types)]
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub type blksize_t = c_int;
