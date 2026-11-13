//! `sys/random.h` implementation.
//!
//! Non-POSIX, see <https://www.man7.org/linux/man-pages/man2/getrandom.2.html>.

use core::slice;

use crate::{
    error::{Errno, Result, ResultExt},
    header::errno::{EINTR, EIO},
    platform::{
        Pal, Sys,
        types::{c_uint, c_void, size_t, ssize_t},
    },
};

/// Fills `buf` with bytes of the system's generator (`getrandom` without
/// flags, which waits for the first seed), asking again after a short read.
pub(crate) fn fill(buf: &mut [u8]) -> Result<()> {
    fill_with(buf, |rest| Sys::getrandom(rest, 0))
}

/// `fill::fill_with` for the errors of relibc: EINTR is asked again, and a
/// run of empty reads is EIO.
pub(crate) fn fill_with<F>(buf: &mut [u8], read: F) -> Result<()>
where
    F: FnMut(&mut [u8]) -> Result<usize>,
{
    self::fill_loop::fill_with(buf, read, |e| e.0 == EINTR, Errno(EIO))
}

#[path = "fill.rs"]
mod fill_loop;
