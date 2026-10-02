#[allow(unused_imports)]
use crate::platform::types::{c_uint, c_ulong};

/// Used for link counts.
#[allow(non_camel_case_types)]
#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
pub type nlink_t = c_ulong;

/// Used for link counts: 32 bits on AArch64 Linux.
#[allow(non_camel_case_types)]
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub type nlink_t = c_uint;
