//! `aio.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/aio.h.html>.
//!
//! A request is carried out in the call that makes it, so it is complete
//! when the call returns: `aio_error` never answers EINPROGRESS and
//! `aio_cancel` finds every request done.

use crate::{
    header::{
        errno::{EAGAIN, EBADF, EINVAL, EIO},
        fcntl::{F_GETFL, fcntl},
        signal::{SIGEV_SIGNAL, kill, sigevent},
        time::timespec,
        unistd::{fdatasync, fsync, getpid, pread, pwrite},
    },
    platform::{
        self,
        types::{c_int, c_void, off_t, size_t, ssize_t},
    },
};

pub const AIO_CANCELED: c_int = 0;
pub const AIO_NOTCANCELED: c_int = 1;
pub const AIO_ALLDONE: c_int = 2;

pub const LIO_READ: c_int = 0;
pub const LIO_WRITE: c_int = 1;
pub const LIO_NOP: c_int = 2;

pub const LIO_WAIT: c_int = 0;
pub const LIO_NOWAIT: c_int = 1;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/aio.h.html>.
#[repr(C)]
pub struct aiocb {
    pub aio_fildes: c_int,
    pub aio_offset: off_t,
    pub aio_buf: *mut c_void,
    pub aio_nbytes: size_t,
    pub aio_reqprio: c_int,
    pub aio_sigevent: sigevent,
    pub aio_lio_opcode: c_int,
    /// The error of the request: 0 or an error number.
    __error: c_int,
    /// The value the operation gave.
    __return: ssize_t,
}

/// Tells of the end of the request as its sigevent asks.
unsafe fn notify(aiocbp: *mut aiocb) {
    let event = unsafe { &(*aiocbp).aio_sigevent };
    if event.sigev_notify == SIGEV_SIGNAL && event.sigev_signo != 0 {
        kill(getpid(), event.sigev_signo);
    }
}

/// Carries the request out; `operation` is LIO_READ or LIO_WRITE.
unsafe fn perform(aiocbp: *mut aiocb, operation: c_int) -> c_int {
    let request = unsafe { &mut *aiocbp };
    if unsafe { fcntl(request.aio_fildes, F_GETFL, 0) } < 0 {
        platform::ERRNO.set(EBADF);
        return -1;
    }
    if request.aio_offset < 0 || request.aio_reqprio < 0 {
        platform::ERRNO.set(EINVAL);
        return -1;
    }
    platform::ERRNO.set(0);
    let done = unsafe {
        if operation == LIO_READ {
            pread(
                request.aio_fildes,
                request.aio_buf,
                request.aio_nbytes,
                request.aio_offset,
            )
        } else {
            pwrite(
                request.aio_fildes,
                request.aio_buf,
                request.aio_nbytes,
                request.aio_offset,
            )
        }
    };
    if done < 0 {
        request.__error = platform::ERRNO.get();
        request.__return = -1;
    } else {
        request.__error = 0;
        request.__return = done;
    }
    unsafe { notify(aiocbp) };
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_read.html>.
///
/// # Safety
/// `aiocbp` points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_read(aiocbp: *mut aiocb) -> c_int {
    unsafe { perform(aiocbp, LIO_READ) }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_write.html>.
///
/// # Safety
/// `aiocbp` points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_write(aiocbp: *mut aiocb) -> c_int {
    unsafe { perform(aiocbp, LIO_WRITE) }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/lio_listio.html>.
///
/// # Safety
/// `list` holds `nent` pointers to requests or null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lio_listio(
    mode: c_int,
    list: *const *mut aiocb,
    nent: c_int,
    sig: *mut sigevent,
) -> c_int {
    if (mode != LIO_WAIT && mode != LIO_NOWAIT) || nent < 0 {
        platform::ERRNO.set(EINVAL);
        return -1;
    }
    let mut failed = false;
    for i in 0..nent as usize {
        let request = unsafe { *list.add(i) };
        if request.is_null() {
            continue;
        }
        let opcode = unsafe { (*request).aio_lio_opcode };
        match opcode {
            LIO_NOP => {}
            LIO_READ | LIO_WRITE => {
                if unsafe { perform(request, opcode) } < 0 {
                    failed = true;
                } else if unsafe { (*request).__error } != 0 {
                    failed = true;
                }
            }
            _ => {
                platform::ERRNO.set(EINVAL);
                return -1;
            }
        }
    }
    if !sig.is_null() && mode == LIO_NOWAIT && unsafe { (*sig).sigev_notify } == SIGEV_SIGNAL {
        kill(getpid(), unsafe { (*sig).sigev_signo });
    }
    if failed {
        platform::ERRNO.set(EIO);
        return -1;
    }
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_error.html>.
///
/// # Safety
/// `aiocbp` points to a request that was made.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_error(aiocbp: *const aiocb) -> c_int {
    unsafe { (*aiocbp).__error }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_return.html>.
///
/// # Safety
/// `aiocbp` points to a request that is complete.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_return(aiocbp: *mut aiocb) -> ssize_t {
    unsafe { (*aiocbp).__return }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_cancel.html>.
///
/// # Safety
/// `aiocbp` is null or points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_cancel(fildes: c_int, aiocbp: *mut aiocb) -> c_int {
    if !aiocbp.is_null() && unsafe { (*aiocbp).aio_fildes } != fildes {
        platform::ERRNO.set(EINVAL);
        return -1;
    }
    if unsafe { fcntl(fildes, F_GETFL, 0) } < 0 {
        platform::ERRNO.set(EBADF);
        return -1;
    }
    // Every request is done when its call returns.
    AIO_ALLDONE
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_suspend.html>.
///
/// # Safety
/// `list` holds `nent` pointers to requests or null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_suspend(
    list: *const *const aiocb,
    nent: c_int,
    _timeout: *const timespec,
) -> c_int {
    if nent < 0 {
        platform::ERRNO.set(EINVAL);
        return -1;
    }
    // The requests are done: the wait is over at once.
    let _ = list;
    let _ = EAGAIN;
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_fsync.html>.
///
/// # Safety
/// `aiocbp` points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_fsync(operation: c_int, aiocbp: *mut aiocb) -> c_int {
    let request = unsafe { &mut *aiocbp };
    let flush = match operation {
        o if o == crate::header::fcntl::O_SYNC => fsync(request.aio_fildes),
        o if o == crate::header::fcntl::O_DSYNC => fdatasync(request.aio_fildes),
        _ => {
            platform::ERRNO.set(EINVAL);
            return -1;
        }
    };
    if flush < 0 {
        request.__error = platform::ERRNO.get();
        request.__return = -1;
    } else {
        request.__error = 0;
        request.__return = 0;
    }
    unsafe { notify(aiocbp) };
    0
}
