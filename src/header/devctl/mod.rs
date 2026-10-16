//! `devctl.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/devctl.h.html>.

use crate::{
    error::Errno,
    header::{errno, fcntl},
    platform::{
        Pal, Sys,
        types::{c_int, c_void, size_t},
    },
};

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_devctl.html>.
///
/// No device of the system takes a device control command: ENOTTY for an
/// open descriptor, EBADF for another.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_devctl(
    fildes: c_int,
    dcmd: c_int,
    dev_data_ptr: *mut c_void,
    nbyte: size_t,
    dev_info_ptr: *mut c_int,
) -> c_int {
    let _ = (dcmd, dev_data_ptr, nbyte, dev_info_ptr);
    match Sys::fcntl(fildes, fcntl::F_GETFD, 0) {
        Ok(_) => errno::ENOTTY,
        Err(Errno(code)) => code,
    }
}
