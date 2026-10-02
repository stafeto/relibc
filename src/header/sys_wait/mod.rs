//! `sys/wait.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_wait.h.html>.

use crate::{
    error::ResultExt,
    out::Out,
    header::signal::siginfo_t,
    platform::{
        Pal, Sys,
        types::{c_int, id_t, pid_t},
    },
};

/// Do not hang if no status is available; return immediately.
pub const WNOHANG: c_int = 1;
/// Report status of stopped child process.
pub const WUNTRACED: c_int = 2;
/// Report status of continued child process.
pub const WCONTINUED: c_int = 8;

/// Status is returned for any child that has stopped upon receipt of a signal.
pub const WSTOPPED: c_int = 2;
/// Wait for processes that have terminated.
pub const WEXITED: c_int = 4;
/// Keep the process whose status is returned in `infop` in a waitable state.
pub const WNOWAIT: c_int = 0x0100_0000;

/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man2/waitpid.2.html>.
///
/// Do not wait for children of other threads in the same thread group.
pub const __WNOTHREAD: c_int = 0x2000_0000;
/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man2/waitpid.2.html>.
///
/// Wait for all children regardless of type ("clone" or "non-clone").
pub const __WALL: c_int = 0x4000_0000;
/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man2/waitpid.2.html>.
///
/// Wait for "clone" children only.
#[expect(overflowing_literals, reason = "intentional")]
pub const __WCLONE: c_int = 0x8000_0000;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/wait.html>.
///
/// Obtains status information pertaining to one of the caller's child
/// processes.
///
/// If returning because the status of a child process is available, returns a
/// value equal to the process ID of the child process for which status is
/// reported. If returning due to the delivery of a signal to the calling
/// process, returns `-1` and sets errno to `EINTR`. If otherwise failing,
/// returns `-1` and sets errno.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wait(stat_loc: *mut c_int) -> pid_t {
    unsafe { waitpid(!0, stat_loc, 0) }
}

/// The kind of the `id` of [`waitid()`]: any child, the child of a process
/// ID, or the children of a process group ID.
pub type idtype_t = c_int;
/// Any child, `id` ignored.
pub const P_ALL: idtype_t = 0;
/// The child whose process ID is `id`.
pub const P_PID: idtype_t = 1;
/// The children whose process group ID is `id`.
pub const P_PGID: idtype_t = 2;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/waitid.html>.
///
/// Waits for a change of the state of a child that `idtype` and `id`
/// name, as `options` say, and describes it in `infop`.
///
/// Returns `0`, with `si_pid` 0 in `infop` for `WNOHANG` when no child
/// changed state; on failure returns `-1` and sets errno.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn waitid(
    idtype: idtype_t,
    id: id_t,
    infop: *mut siginfo_t,
    options: c_int,
) -> c_int {
    Sys::waitid(idtype, id, infop, options)
        .map(|()| 0)
        .or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/waitpid.html>.
///
/// Obtains status information pertaining to one of the caller's child
/// processes.
///
/// If returning because the status of a child process is available, returns a
/// value equal to the process ID of the child process for which status is
/// reported. If returning due to the delivery of a signal to the calling
/// process, returns `-1` and sets errno to `EINTR`. if invoked with `WNOHANG`
/// set in `options`, it has at least one child process specified by `pid` for
/// wchich status is not available, and status is not available for any process
/// specified by `pid`, `0` is returned. If otherwise failing, returns `-1` and
/// sets errno.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn waitpid(pid: pid_t, stat_loc: *mut c_int, options: c_int) -> pid_t {
    Sys::waitpid(pid, unsafe { Out::nullable(stat_loc) }, options).or_minus_one_errno()
}
