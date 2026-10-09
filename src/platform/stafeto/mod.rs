// SPDX-License-Identifier: MIT
//! The stafeto platform: relibc on the Linux AArch64 C ABI (target
//! `aarch64-unknown-linux-gnu` with `--cfg stafeto`) over the POSIX system
//! layer of stafeto, a capability microkernel. Each call the platform
//! supports goes to a C function `stafeto_*` of the layer, which returns a
//! value or a negated errno; the rest answer ENOSYS until the layer has them.
#![allow(unused_variables, unused_imports, unused_mut)]

use core::arch::asm;

use super::{Pal, types::*};
use crate::{
    c_str::CStr,
    error::{Errno, Result},
    header::{
        bits_sigset_t::sigset_t,
        dirent::dirent,
        errno::{EINVAL, EIO, ENOSYS},
        fcntl::AT_EMPTY_PATH,
        poll::{nfds_t, pollfd},
        signal::{SIGCHLD, sigevent},
        sys_resource::{rlimit, rusage},
        sys_select::{fd_set, timeval},
        sys_stat::{S_IFIFO, stat},
        sys_statvfs::statvfs,
        sys_time::timezone,
        sys_uio::iovec,
        sys_utsname::utsname,
        time::{itimerspec, timespec},
        unistd::{SEEK_CUR, SEEK_SET},
    },
    ld_so::tcb::{OsSpecific, Tcb},
    out::Out,
};
use core::{fmt::Write, mem, num::NonZeroU64, ptr};
use generic_rt::GenericTcb;

unsafe extern "C" {
    fn stafeto_write(fd: c_int, buf: *const u8, len: usize) -> isize;
    fn stafeto_read(fd: c_int, buf: *mut u8, len: usize) -> isize;
    fn stafeto_openat(dirfd: c_int, path: *const c_char, flags: c_int, mode: mode_t) -> c_int;
    fn stafeto_close(fd: c_int) -> c_int;
    fn stafeto_ftruncate(fd: c_int, length: off_t) -> c_int;
    fn stafeto_grantpt(fd: c_int) -> c_int;
    fn stafeto_lseek(fd: c_int, offset: off_t, whence: c_int) -> off_t;
    pub(crate) fn stafeto_exit(status: c_int) -> !;
    fn stafeto_clock_gettime(clock: clockid_t, out: *mut timespec) -> c_int;
    fn stafeto_clock_getres(clock: clockid_t, out: *mut timespec) -> c_int;
    fn stafeto_mmap_anonymous(len: usize) -> *mut c_void;
    fn stafeto_munmap(addr: *mut c_void, len: usize) -> c_int;
    fn stafeto_getpid() -> pid_t;
    fn stafeto_getppid() -> pid_t;
    /// pipe2: the read end and the write end of a new pipe of the pipe
    /// service into `fds` (O_NONBLOCK, O_CLOEXEC, O_CLOFORK).
    fn stafeto_pipe2(fds: *mut c_int, flags: c_int) -> c_int;
    /// getrandom: `len` bytes of the process's generator, keyed by the
    /// entropy service (GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE).
    fn stafeto_getrandom(buf: *mut u8, len: usize, flags: c_uint) -> isize;
    /// The layer's ABI word (`PLATFORM_ABI`).
    static STAFETO_PLATFORM_ABI: u64;
    /// Attaches the calling thread, whose TCB is `tcb`, to the layer: its
    /// block in the TCB, its channel, timer and entry of signals.
    fn stafeto_init(tcb: *mut c_void) -> c_int;
    /// Waits while `*addr == val`, until a wake, an entry of signals or the
    /// absolute CLOCK_MONOTONIC `deadline` in ns (u64::MAX: none).
    fn stafeto_futex_wait(addr: *mut u32, val: u32, deadline: u64) -> c_int;
    /// Wakes up to `count` waiters on `addr`, highest level first.
    fn stafeto_futex_wake(addr: *mut u32, count: u32) -> u32;
    /// Makes a thread that starts on `stack` (the words relibc pushed: the
    /// shim, then its four arguments) with the thread block `block`, in the
    /// TCB relibc made for it; returns its number (> 0).
    fn stafeto_thread_create(stack: *mut usize, block: *mut c_void) -> c_int;
    /// The calling thread's number.
    fn stafeto_thread_id() -> c_int;
    /// Ends the calling thread; the layer takes its stack back once the
    /// kernel told of its end.
    fn stafeto_exit_thread(stack: *mut c_void, size: usize) -> !;
    fn stafeto_sched_yield() -> c_int;
    fn stafeto_nanosleep(request: *const timespec, remaining: *mut timespec) -> c_int;
    fn stafeto_poll(fds: *mut pollfd, count: nfds_t, timeout: c_int) -> c_int;
    fn stafeto_ppoll(
        fds: *mut pollfd,
        count: nfds_t,
        timeout: *const timespec,
        mask: *const sigset_t,
    ) -> c_int;
    fn stafeto_select(
        count: c_int,
        read: *mut fd_set,
        write: *mut fd_set,
        except: *mut fd_set,
        timeout: *mut timeval,
    ) -> c_int;
    fn stafeto_pselect(
        count: c_int,
        read: *mut fd_set,
        write: *mut fd_set,
        except: *mut fd_set,
        timeout: *const timespec,
        mask: *const sigset_t,
    ) -> c_int;
    /// clock_nanosleep: 0 or an error number.
    fn stafeto_clock_nanosleep(
        clock: clockid_t,
        flags: c_int,
        request: *const timespec,
        remaining: *mut timespec,
    ) -> c_int;
    /// The new thread's part of its start, once its TCB is installed.
    fn stafeto_thread_started() -> c_int;
    /// The calling thread leaves: every signal masked, no new cancellation.
    fn stafeto_thread_leaving();
    /// relibc gave up thread `id` (joined, or detached and ended): its TCB
    /// and stack may go once the kernel told of its end.
    fn stafeto_thread_release(id: c_int);
    fn stafeto_ioctl(fd: c_int, request: c_ulong, arg: *mut c_void) -> c_int;
    /// The name of the terminal `fd` is, NUL-terminated, into `len` bytes at
    /// `buf`: its length, or a negated errno (ENOTTY, ERANGE).
    fn stafeto_ttyname(fd: c_int, buf: *mut u8, len: usize) -> isize;
    fn stafeto_chdir(path: *const c_char) -> c_int;
    fn stafeto_clock_settime(clock: clockid_t, time: *const timespec) -> c_int;
    fn stafeto_dup(fd: c_int) -> c_int;
    fn stafeto_dup2(fd: c_int, target: c_int) -> c_int;
    /// dup3 with O_CLOEXEC and O_CLOFORK in `flags`.
    fn stafeto_dup3(fd: c_int, target: c_int, flags: c_int) -> c_int;
    /// fstat (path null), stat and lstat in Linux's struct stat.
    fn stafeto_fstatat(fd: c_int, path: *const c_char, out: *mut stat, flags: c_int) -> c_int;
    fn stafeto_fcntl(fd: c_int, command: c_int, argument: c_ulonglong) -> c_int;
    fn stafeto_getcwd(buf: *mut u8, len: usize) -> c_int;
    /// Linux dirent64 records of the directory `fd` from position `off`.
    fn stafeto_getdents(fd: c_int, buf: *mut u8, len: usize, off: u64) -> isize;
    /// posix_getdents: the records of the directory from the descriptor's
    /// offset, which moves past them: the bytes, 0 at the end.
    fn stafeto_posix_getdents(fd: c_int, buf: *mut u8, len: usize) -> isize;
    /// The file system of a descriptor or of a path, as relibc's struct
    /// statvfs.
    fn stafeto_fstatvfs(fd: c_int, out: *mut statvfs) -> c_int;
    fn stafeto_statvfs(path: *const c_char, out: *mut statvfs) -> c_int;
    /// The current directory becomes the directory of the descriptor.
    fn stafeto_fchdir(fd: c_int) -> c_int;
    /// The canonical path of `path` into `buf` with its NUL: its length.
    fn stafeto_realpath(path: *const c_char, buf: *mut u8, len: usize) -> isize;
    fn stafeto_getuid() -> uid_t;
    fn stafeto_geteuid() -> uid_t;
    fn stafeto_getgid() -> gid_t;
    fn stafeto_getegid() -> gid_t;
    fn stafeto_getrlimit(resource: c_int, out: *mut rlimit) -> c_int;
    fn stafeto_pread(fd: c_int, buf: *mut u8, len: usize, off: off_t) -> isize;
    fn stafeto_pwrite(fd: c_int, buf: *const u8, len: usize, off: off_t) -> isize;
    fn stafeto_umask(mask: mode_t) -> mode_t;
    /// The user and group ids as setuid, seteuid, setgid and setegid set
    /// them (-1 keeps one).
    fn stafeto_setresuid(real: uid_t, effective: uid_t, saved: uid_t) -> c_int;
    fn stafeto_setresgid(real: gid_t, effective: gid_t, saved: gid_t) -> c_int;
    fn stafeto_uname(out: *mut utsname) -> c_int;
    /// Asks thread `id` to cancel (deferred: at its next point).
    fn stafeto_cancel(id: c_int) -> c_int;
    /// Whether the calling thread's point acts on a request: 1 or 0.
    fn stafeto_testcancel() -> c_int;
    fn stafeto_setcancelstate(state: c_int, old: *mut c_int) -> c_int;
    fn stafeto_setcanceltype(kind: c_int, old: *mut c_int) -> c_int;
    /// fork by a full copy of the process: the child's PID, 0 in the
    /// child, or the negated errno.
    fn stafeto_fork() -> pid_t;
    /// execve of the program at `path` with `argv` and `envp`: it
    /// returns only with the negated errno.
    fn stafeto_exec(
        path: *const c_char,
        argv: *const *const c_char,
        envp: *const *const c_char,
    ) -> c_int;
    /// posix_spawn of the program at `path` with `argv` and `envp`
    /// (NULL-ended; null for none) and the attributes at `attributes`
    /// (null for none): the child's PID.
    fn stafeto_spawn(
        path: *const c_char,
        argv: *const *const c_char,
        envp: *const *const c_char,
        attributes: *const SpawnAttributes,
        actions: *const SpawnAction,
        count: usize,
    ) -> pid_t;
    /// posix_spawnp of the name `file` with no slash: the layer searches the
    /// directories of `path` (null for no PATH) after the file actions, from
    /// the directory they left as the current one.
    fn stafeto_spawnp(
        file: *const c_char,
        path: *const c_char,
        argv: *const *const c_char,
        envp: *const *const c_char,
        attributes: *const SpawnAttributes,
        actions: *const SpawnAction,
        count: usize,
    ) -> pid_t;
    /// waitpid: the child's PID (0 for WNOHANG with none), its status in
    /// `status`.
    fn stafeto_waitpid(pid: pid_t, status: *mut c_int, options: c_int) -> pid_t;
    /// waitid: 0, with the Linux siginfo of the child at `info`.
    fn stafeto_waitid(idtype: c_int, id: id_t, info: *mut c_void, options: c_int) -> c_int;
    /// setpgid, setsid, getpgid and getsid: the value (0 for setpgid), or
    /// the negated errno.
    fn stafeto_setpgid(pid: pid_t, pgid: pid_t) -> c_int;
    /// The operations on names and metadata, the `*at` forms of Linux: 0 or
    /// the negated errno.
    fn stafeto_unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int;
    fn stafeto_mkdirat(dirfd: c_int, path: *const c_char, mode: mode_t) -> c_int;
    fn stafeto_faccessat(dirfd: c_int, path: *const c_char, mode: c_int, flags: c_int) -> c_int;
    fn stafeto_renameat(
        old_dirfd: c_int,
        old: *const c_char,
        new_dirfd: c_int,
        new: *const c_char,
    ) -> c_int;
    fn stafeto_linkat(
        old_dirfd: c_int,
        old: *const c_char,
        new_dirfd: c_int,
        new: *const c_char,
        flags: c_int,
    ) -> c_int;
    fn stafeto_symlinkat(target: *const c_char, new_dirfd: c_int, linkpath: *const c_char)
    -> c_int;
    /// The bytes of the link (no NUL), or the negated errno.
    fn stafeto_readlinkat(dirfd: c_int, path: *const c_char, buf: *mut u8, len: usize) -> isize;
    fn stafeto_fchmodat(dirfd: c_int, path: *const c_char, mode: mode_t, flags: c_int) -> c_int;
    fn stafeto_fchownat(
        dirfd: c_int,
        path: *const c_char,
        uid: uid_t,
        gid: gid_t,
        flags: c_int,
    ) -> c_int;
    fn stafeto_utimensat(
        dirfd: c_int,
        path: *const c_char,
        times: *const timespec,
        flags: c_int,
    ) -> c_int;
    fn stafeto_setsid() -> c_int;
    fn stafeto_getpgid(pid: pid_t) -> c_int;
    fn stafeto_getsid(pid: pid_t) -> c_int;
}

/// The attributes of posix_spawn the layer takes: the spawn-flags, the
/// process group, the masks of POSIX_SPAWN_SETSIGMASK and
/// POSIX_SPAWN_SETSIGDEF.
#[repr(C)]
struct SpawnAttributes {
    flags: c_int,
    pgroup: c_int,
    mask: u64,
    default: u64,
}

/// A file action of posix_spawn as the layer takes it: OPEN (1) of `path`
/// with `flags` at `fd`, CLOSE (2), DUP2 (3) of `fd` to `newfd`, CHDIR (4)
/// to `path`, FCHDIR (5) to `fd`.
#[repr(C)]
struct SpawnAction {
    kind: c_int,
    fd: c_int,
    newfd: c_int,
    flags: c_int,
    mode: u32,
    path: *const c_char,
}

/// The parts of a readv or writev before the first that has bytes and no
/// buffer: the bytes of the parts before it move, and the call gives
/// their count; EFAULT when the first part is such.
fn valid_parts(parts: &[iovec]) -> Result<&[iovec]> {
    match parts
        .iter()
        .position(|p| p.iov_base.is_null() && p.iov_len > 0)
    {
        Some(0) => Err(Errno(crate::header::errno::EFAULT)),
        Some(n) => Ok(&parts[..n]),
        None => Ok(parts),
    }
}

/// The most bytes one read of the layer gives (proto_fs::MAX_READ).
const STAFETO_READ_MAX: usize = 1016;

/// The version of the interface of the `stafeto_*` functions.
const PLATFORM_INTERFACE: u64 = 14;

/// The ABI word relibc and the layer must agree on: the size of the
/// thread block in bits 0 to 15, its offset in the TCB in bits 16 to 31,
/// the interface in bits 32 to 63.
const PLATFORM_ABI: u64 = mem::size_of::<OsSpecific>() as u64
    | (mem::offset_of!(GenericTcb<OsSpecific>, os_specific) as u64) << 16
    | PLATFORM_INTERFACE << 32;

/// The platform's part of the start of a process, once its TCB exists:
/// checks the layer's ABI word and attaches the main thread. A process
/// whose layer does not match says why on fd 2 and ends with status 125,
/// the status of a failed start of the layer's own (posix-crt).
pub(crate) unsafe fn init() {
    // SAFETY: the layer defines the word and never writes it.
    let layer = unsafe { ptr::read_volatile(&raw const STAFETO_PLATFORM_ABI) };
    if layer != PLATFORM_ABI {
        let _ = writeln!(
            super::FileWriter::new(2),
            "relibc: stafeto platform ABI {layer:#x}, relibc expects {PLATFORM_ABI:#x}"
        );
        Sys::exit(125);
    }
    unsafe { crate::ld_so::native_tcb::publish() };
    let tcb = unsafe { Tcb::current() }.map_or(ptr::null_mut(), |tcb| ptr::from_mut(tcb).cast());
    if let Err(Errno(errno)) = ret(unsafe { stafeto_init(tcb) } as isize) {
        let _ = writeln!(
            super::FileWriter::new(2),
            "relibc: the stafeto layer did not attach the main thread: errno {errno}"
        );
        Sys::exit(125);
    }
    // The first handlers of fork, whose prepare handler runs after all
    // others: relibc's own locks are the forking thread's across the copy.
    crate::header::pthread::pthread_atfork(Some(fork_prepare), Some(fork_parent), Some(fork_child));
}

/// fork's prepare handler: relibc's locks in a fixed order, that of their
/// nesting elsewhere (each of them may call the allocator, whose lock
/// comes last). The streams' locks are not taken: a thread may hold one
/// through a read of the console with no end, and the child frees them.
extern "C" fn fork_prepare() {
    crate::header::pthread::tls::fork_lock();
    crate::header::time::fork_lock();
    crate::pthread::fork_lock();
    crate::platform::fork_lock_allocator();
}

/// fork's parent handler: the locks of `fork_prepare` go, last first.
extern "C" fn fork_parent() {
    crate::platform::fork_unlock_allocator();
    crate::pthread::fork_unlock_parent();
    crate::header::time::fork_unlock();
    crate::header::pthread::tls::fork_unlock();
}

/// fork's child handler: the locks of `fork_prepare` go, the table of
/// threads keeps the child's one, and the spin lock of the functions of
/// exit and the standard streams' locks are free (the threads that may
/// have held them are the parent's).
extern "C" fn fork_child() {
    crate::platform::fork_unlock_allocator();
    crate::pthread::fork_unlock_child();
    crate::header::time::fork_unlock();
    crate::header::pthread::tls::fork_unlock();
    crate::cxa::fork_reset();
    use crate::header::stdio;
    for stream in unsafe { [stdio::stdin, stdio::stdout, stdio::stderr] } {
        if let Some(stream) = unsafe { stream.as_mut() } {
            stream.reset_lock_after_fork();
        }
    }
}

/// The stafeto layer returns a value or a negated errno.
fn ret(value: isize) -> Result<isize> {
    if value < 0 {
        Err(Errno(-value as c_int))
    } else {
        Ok(value)
    }
}

mod epoll;
mod ptrace;
mod signal;
mod socket;

/// The new thread's part of its start (`new_thread_shim`, after its TCB
/// is installed): its entry of signals. A failure ends the process.
pub(crate) fn thread_started() {
    if let Err(Errno(errno)) = ret(unsafe { stafeto_thread_started() } as isize) {
        let _ = writeln!(
            super::FileWriter::new(2),
            "relibc: the stafeto layer did not attach a new thread: errno {errno}"
        );
        Sys::exit(125);
    }
}

/// The calling thread leaves (`exit_current_thread`, before relibc gives
/// its TCB away): no signal handler and no cancellation from here on.
pub(crate) fn thread_leaving() {
    unsafe { stafeto_thread_leaving() }
}

/// relibc gave up the thread `os_tid` (`dealloc_thread`).
pub(crate) fn thread_release(os_tid: crate::pthread::OsTid) {
    unsafe { stafeto_thread_release(os_tid.thread_id as c_int) }
}

/// clock_nanosleep on the platform: 0 or an error number.
pub(crate) unsafe fn clock_nanosleep(
    clock: clockid_t,
    flags: c_int,
    request: *const timespec,
    remaining: *mut timespec,
) -> c_int {
    unsafe { stafeto_clock_nanosleep(clock, flags, request, remaining) }
}

/// Asks thread `os_tid` to cancel at its next cancellation point.
pub(crate) fn cancel(os_tid: crate::pthread::OsTid) -> Result<()> {
    ret(unsafe { stafeto_cancel(os_tid.thread_id as c_int) } as isize).map(|_| ())
}

/// Whether the calling thread's cancellation point acts now.
pub(crate) fn testcancel() -> bool {
    (unsafe { stafeto_testcancel() }) != 0
}

pub(crate) fn set_cancel_state(state: c_int) -> Result<c_int> {
    let mut old = 0;
    ret(unsafe { stafeto_setcancelstate(state, &raw mut old) } as isize).map(|_| old)
}

pub(crate) fn set_cancel_type(kind: c_int) -> Result<c_int> {
    let mut old = 0;
    ret(unsafe { stafeto_setcanceltype(kind, &raw mut old) } as isize).map(|_| old)
}

/// Linux AArch64 layouts passed directly to posix-platform's wait calls.
const _: () = {
    assert!(mem::size_of::<pollfd>() == 8);
    assert!(mem::offset_of!(pollfd, fd) == 0);
    assert!(mem::offset_of!(pollfd, events) == 4);
    assert!(mem::offset_of!(pollfd, revents) == 6);
    assert!(mem::size_of::<fd_set>() == 128);
    assert!(mem::align_of::<fd_set>() == 8);
    assert!(mem::size_of::<timespec>() == 16);
    assert!(mem::offset_of!(timespec, tv_nsec) == 8);
    assert!(mem::size_of::<timeval>() == 16);
    assert!(mem::offset_of!(timeval, tv_usec) == 8);
};

/// The terminal interface as the layer's posix-platform and the terminal
/// service's proto_tty have it: struct termios of Linux AArch64 (glibc's,
/// 60 bytes: four flag words, the line, 32 control characters, two
/// speeds) and the numbers of its flags, control characters and ioctl
/// requests. Their const assertions say the same on the other side.
const _: () = {
    use crate::header::{sys_ioctl, termios};
    assert!(mem::size_of::<termios::termios>() == 60);
    assert!(mem::offset_of!(termios::termios, c_line) == 16);
    assert!(mem::offset_of!(termios::termios, c_cc) == 17);
    assert!(mem::offset_of!(termios::termios, __c_ispeed) == 52);
    assert!(mem::offset_of!(termios::termios, __c_ospeed) == 56);
    assert!(termios::NCCS == 32);
    assert!(termios::VINTR == 0 && termios::VQUIT == 1 && termios::VERASE == 2);
    assert!(termios::VKILL == 3 && termios::VEOF == 4 && termios::VTIME == 5);
    assert!(termios::VMIN == 6 && termios::VSTART == 8 && termios::VSTOP == 9);
    assert!(termios::VSUSP == 10 && termios::VEOL == 11 && termios::VWERASE == 14);
    assert!(termios::ISTRIP == 0o40 && termios::INLCR == 0o100);
    assert!(termios::IGNCR == 0o200 && termios::ICRNL == 0o400);
    assert!(termios::OPOST == 1 && termios::ONLCR == 4 && termios::OCRNL == 0o10);
    assert!(termios::ISIG == 1 && termios::ICANON == 2 && termios::ECHO == 0o10);
    assert!(termios::ECHOE == 0o20 && termios::ECHOK == 0o40 && termios::ECHONL == 0o100);
    assert!(termios::NOFLSH == 0o200 && termios::TOSTOP == 0o400);
    assert!(termios::ECHOCTL == 0o1000 && termios::ECHOKE == 0o4000);
    assert!(termios::IEXTEN == 0o100000);
    assert!(termios::B38400 == 0o17 && termios::CS8 == 0o60 && termios::CREAD == 0o200);
    assert!(termios::TCSANOW == 0 && termios::TCSADRAIN == 1 && termios::TCSAFLUSH == 2);
    assert!(termios::TCOOFF == 0 && termios::TCOON == 1);
    assert!(termios::TCIOFF == 2 && termios::TCION == 3);
    assert!(termios::TCIFLUSH == 0 && termios::TCOFLUSH == 1 && termios::TCIOFLUSH == 2);
    assert!(sys_ioctl::TCGETS == 0x5401 && sys_ioctl::TCSETS == 0x5402);
    assert!(sys_ioctl::TCSETSW == 0x5403 && sys_ioctl::TCSETSF == 0x5404);
    assert!(sys_ioctl::TCSBRK == 0x5409 && sys_ioctl::TCXONC == 0x540A);
    assert!(sys_ioctl::TCFLSH == 0x540B);
};

/// The stafeto implementation of [`Pal`].
pub struct Sys;

impl Sys {
    /// Assign the slave device's ownership through its master description.
    pub fn grantpt(fd: c_int) -> Result<c_int> {
        // SAFETY: the layer validates the descriptor; this call takes no pointers.
        ret(unsafe { stafeto_grantpt(fd) } as isize).map(|value| value as c_int)
    }

    /// Multiplexes descriptors directly through the layer's bounded watches.
    pub unsafe fn poll_direct(fds: *mut pollfd, count: nfds_t, timeout: c_int) -> Result<c_int> {
        ret(unsafe { stafeto_poll(fds, count, timeout) } as isize).map(|count| count as c_int)
    }

    pub unsafe fn ppoll_direct(
        fds: *mut pollfd,
        count: nfds_t,
        timeout: *const timespec,
        mask: *const sigset_t,
    ) -> Result<c_int> {
        ret(unsafe { stafeto_ppoll(fds, count, timeout, mask) } as isize)
            .map(|count| count as c_int)
    }

    pub unsafe fn select_direct(
        count: c_int,
        read: *mut fd_set,
        write: *mut fd_set,
        except: *mut fd_set,
        timeout: *mut timeval,
    ) -> Result<c_int> {
        ret(unsafe { stafeto_select(count, read, write, except, timeout) } as isize)
            .map(|count| count as c_int)
    }

    pub unsafe fn pselect_direct(
        count: c_int,
        read: *mut fd_set,
        write: *mut fd_set,
        except: *mut fd_set,
        timeout: *const timespec,
        mask: *const sigset_t,
    ) -> Result<c_int> {
        ret(unsafe { stafeto_pselect(count, read, write, except, timeout, mask) } as isize)
            .map(|count| count as c_int)
    }
    pub unsafe fn ioctl(fd: c_int, request: c_ulong, out: *mut c_void) -> Result<c_int> {
        ret(unsafe { stafeto_ioctl(fd, request, out) } as isize).map(|v| v as c_int)
    }

    /// The name of the terminal `fd` is, into `out` (ttyname_r): its
    /// length without the NUL; `out` has a byte beyond it for the NUL.
    pub fn ttyname(fd: c_int, out: &mut [u8]) -> Result<usize> {
        ret(unsafe { stafeto_ttyname(fd, out.as_mut_ptr(), out.len() + 1) }).map(|len| len as usize)
    }
}

impl Pal for Sys {
    fn faccessat(fd: c_int, path: CStr, amode: c_int, flags: c_int) -> Result<()> {
        ret(unsafe { stafeto_faccessat(fd, path.as_ptr(), amode, flags) } as isize).map(|_| ())
    }

    unsafe fn brk(addr: *mut c_void) -> Result<*mut c_void> {
        // The heap is the layer's: dlmalloc takes its memory by mmap.
        Err(Errno(crate::header::errno::ENOMEM))
    }

    fn chdir(path: CStr) -> Result<()> {
        ret(unsafe { stafeto_chdir(path.as_ptr()) } as isize).map(|_| ())
    }

    fn fchownat(fildes: c_int, path: CStr, owner: uid_t, group: gid_t, flags: c_int) -> Result<()> {
        ret(unsafe { stafeto_fchownat(fildes, path.as_ptr(), owner, group, flags) } as isize)
            .map(|_| ())
    }

    fn clock_getres(clk_id: clockid_t, res: Option<Out<timespec>>) -> Result<()> {
        ret(unsafe {
            stafeto_clock_getres(clk_id, res.map_or(ptr::null_mut(), |mut p| p.as_mut_ptr()))
        } as isize)
        .map(|_| ())
    }

    fn clock_gettime(clk_id: clockid_t, mut tp: Out<timespec>) -> Result<()> {
        ret(unsafe { stafeto_clock_gettime(clk_id, tp.as_mut_ptr()) } as isize).map(|_| ())
    }

    unsafe fn clock_settime(clk_id: clockid_t, tp: *const timespec) -> Result<()> {
        ret(unsafe { stafeto_clock_settime(clk_id, tp) } as isize).map(|_| ())
    }

    fn close(fildes: c_int) -> Result<()> {
        ret(unsafe { stafeto_close(fildes) } as isize).map(|_| ())
    }

    fn dup(fildes: c_int) -> Result<c_int> {
        ret(unsafe { stafeto_dup(fildes) } as isize).map(|v| v as c_int)
    }

    fn dup2(fildes: c_int, fildes2: c_int) -> Result<c_int> {
        ret(unsafe { stafeto_dup2(fildes, fildes2) } as isize).map(|v| v as c_int)
    }

    /// The layer's exec: a new process for the record with the loader,
    /// the record moved to it once the image is ready (stafeto 5c).
    /// fexecve stays ENOSYS: the layer opens a program by its path.
    unsafe fn execve(path: CStr, argv: *const *mut c_char, envp: *const *mut c_char) -> Result<()> {
        static NONE: [usize; 1] = [0];
        let none = NONE.as_ptr().cast::<*const c_char>();
        let list = |p: *const *mut c_char| {
            if p.is_null() {
                none
            } else {
                p.cast::<*const c_char>()
            }
        };
        ret(unsafe { stafeto_exec(path.as_ptr(), list(argv), list(envp)) } as isize).map(drop)
    }
    unsafe fn fexecve(
        fildes: c_int,
        argv: *const *mut c_char,
        envp: *const *mut c_char,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn dup3(fildes: c_int, fildes2: c_int, flag: c_int) -> Result<c_int> {
        ret(unsafe { stafeto_dup3(fildes, fildes2, flag) } as isize).map(|v| v as c_int)
    }

    fn exit(status: c_int) -> ! {
        unsafe { stafeto_exit(status) }
    }
    unsafe fn exit_thread(stack_base: *mut (), stack_size: usize) -> ! {
        unsafe { stafeto_exit_thread(stack_base.cast(), stack_size) }
    }

    fn fchdir(fildes: c_int) -> Result<()> {
        ret(unsafe { stafeto_fchdir(fildes) } as isize).map(|_| ())
    }

    fn fchmodat(dirfd: c_int, path: Option<CStr>, mode: mode_t, flags: c_int) -> Result<()> {
        let path = path.map_or(ptr::null(), |path| path.as_ptr());
        ret(unsafe { stafeto_fchmodat(dirfd, path, mode, flags) } as isize).map(|_| ())
    }

    fn fdatasync(fildes: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn flock(fd: c_int, operation: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fstatat(fildes: c_int, path: Option<CStr>, mut buf: Out<stat>, flags: c_int) -> Result<()> {
        let path = path.map_or(ptr::null(), |path| path.as_ptr());
        ret(unsafe { stafeto_fstatat(fildes, path, buf.as_mut_ptr(), flags) } as isize).map(|_| ())
    }

    fn fstatvfs(fildes: c_int, mut buf: Out<statvfs>) -> Result<()> {
        ret(unsafe { stafeto_fstatvfs(fildes, buf.as_mut_ptr()) } as isize).map(|_| ())
    }

    fn statvfs(path: CStr, mut buf: Out<statvfs>) -> Result<()> {
        ret(unsafe { stafeto_statvfs(path.as_ptr(), buf.as_mut_ptr()) } as isize).map(|_| ())
    }

    fn realpath(path: CStr, out: &mut [u8]) -> Result<()> {
        ret(unsafe { stafeto_realpath(path.as_ptr(), out.as_mut_ptr(), out.len()) }).map(|_| ())
    }

    fn fcntl(fildes: c_int, cmd: c_int, arg: c_ulonglong) -> Result<c_int> {
        ret(unsafe { stafeto_fcntl(fildes, cmd, arg) } as isize).map(|v| v as c_int)
    }

    unsafe fn fork() -> Result<pid_t> {
        ret(unsafe { stafeto_fork() } as isize).map(|pid| pid as pid_t)
    }

    fn fpath(fildes: c_int, out: &mut [u8]) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn fsync(fildes: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn ftruncate(fildes: c_int, length: off_t) -> Result<()> {
        ret(unsafe { stafeto_ftruncate(fildes, length) } as isize).map(|_| ())
    }

    #[inline]
    unsafe fn futex_wait(addr: *mut u32, val: u32, deadline: Option<&timespec>) -> Result<()> {
        // The deadline is absolute on CLOCK_MONOTONIC, as for FUTEX_WAIT_BITSET.
        let deadline = deadline.map_or(u64::MAX, |d| {
            if d.tv_sec < 0 {
                0
            } else {
                (d.tv_sec as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(d.tv_nsec.clamp(0, 999_999_999) as u64)
            }
        });
        ret(unsafe { stafeto_futex_wait(addr, val, deadline) } as isize).map(|_| ())
    }
    #[inline]
    unsafe fn futex_wake(addr: *mut u32, num: u32) -> Result<u32> {
        Ok(unsafe { stafeto_futex_wake(addr, num) })
    }

    unsafe fn utimensat(
        dirfd: c_int,
        path: CStr,
        times: *const timespec,
        flag: c_int,
    ) -> Result<()> {
        ret(unsafe { stafeto_utimensat(dirfd, path.as_ptr(), times, flag) } as isize).map(|_| ())
    }

    fn getcwd(mut buf: Out<[u8]>) -> Result<()> {
        let len = buf.len();
        let pointer = buf.as_mut_ptr().cast::<u8>();
        ret(unsafe { stafeto_getcwd(pointer, len) } as isize).map(|_| ())
    }

    fn getdents(fd: c_int, buf: &mut [u8], off: u64) -> Result<usize> {
        // Stateless: `off` is the position after the last entry relibc
        // took (its d_off), a position of the directory's descriptor.
        ret(unsafe { stafeto_getdents(fd, buf.as_mut_ptr(), buf.len(), off) }).map(|v| v as usize)
    }
    fn dir_seek(fd: c_int, off: u64) -> Result<()> {
        ret(unsafe { stafeto_lseek(fd, off as off_t, SEEK_SET) } as isize).map(|_| ())
    }
    // FIXME use offset or remove it
    unsafe fn dent_reclen_offset(this_dent: &[u8], _offset: usize) -> Option<(u16, u64)> {
        // Linux's struct dirent64, as the layer writes it.
        let dent = this_dent.as_ptr().cast::<dirent>();
        Some((
            unsafe { (*dent).d_reclen },
            unsafe { (*dent).d_off }.cast_unsigned(),
        ))
    }

    fn getegid() -> gid_t {
        unsafe { stafeto_getegid() }
    }

    fn geteuid() -> uid_t {
        unsafe { stafeto_geteuid() }
    }

    fn getgid() -> gid_t {
        unsafe { stafeto_getgid() }
    }

    fn getgroups(list: Out<[gid_t]>) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn getpagesize() -> usize {
        4096
    }

    fn getpgid(pid: pid_t) -> Result<pid_t> {
        ret(unsafe { stafeto_getpgid(pid) } as isize).map(|v| v as pid_t)
    }

    fn getpid() -> pid_t {
        unsafe { stafeto_getpid() }
    }

    fn getppid() -> pid_t {
        unsafe { stafeto_getppid() }
    }

    fn getpriority(which: c_int, who: id_t) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn getrandom(buf: &mut [u8], flags: c_uint) -> Result<usize> {
        ret(unsafe { stafeto_getrandom(buf.as_mut_ptr(), buf.len(), flags) }).map(|n| n as usize)
    }

    fn getrlimit(resource: c_int, mut rlim: Out<rlimit>) -> Result<()> {
        ret(unsafe { stafeto_getrlimit(resource, rlim.as_mut_ptr()) } as isize).map(|_| ())
    }

    fn getresgid(
        rgid: Option<Out<gid_t>>,
        egid: Option<Out<gid_t>>,
        sgid: Option<Out<gid_t>>,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }
    fn getresuid(
        ruid: Option<Out<uid_t>>,
        euid: Option<Out<uid_t>>,
        suid: Option<Out<uid_t>>,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn setrlimit(resource: c_int, rlimit: *const rlimit) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn getrusage(who: c_int, r_usage: Out<rusage>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn getsid(pid: pid_t) -> Result<pid_t> {
        ret(unsafe { stafeto_getsid(pid) } as isize).map(|v| v as pid_t)
    }

    fn gettid() -> pid_t {
        unsafe { stafeto_thread_id() }
    }

    fn gettimeofday(tp: Out<timeval>, tzp: Option<Out<timezone>>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn getuid() -> uid_t {
        unsafe { stafeto_getuid() }
    }

    fn linkat(fd1: c_int, path1: CStr, fd2: c_int, path2: CStr, flags: c_int) -> Result<()> {
        ret(unsafe { stafeto_linkat(fd1, path1.as_ptr(), fd2, path2.as_ptr(), flags) } as isize)
            .map(|_| ())
    }

    fn lseek(fildes: c_int, offset: off_t, whence: c_int) -> Result<off_t> {
        ret(unsafe { stafeto_lseek(fildes, offset, whence) } as isize).map(|v| v as off_t)
    }

    fn mkdirat(dir_fildes: c_int, path: CStr, mode: mode_t) -> Result<()> {
        ret(unsafe { stafeto_mkdirat(dir_fildes, path.as_ptr(), mode) } as isize).map(|_| ())
    }

    fn mknodat(dir_fildes: c_int, path: CStr, mode: mode_t, dev: dev_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn mkfifoat(dir_fd: c_int, path: CStr, mode: mode_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn mlock(addr: *const c_void, len: usize) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn mlockall(flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fildes: c_int,
        off: off_t,
    ) -> Result<*mut c_void> {
        if len == 0 {
            return Err(Errno(EINVAL));
        }
        // POSIX mmap: EINVAL when neither MAP_PRIVATE nor MAP_SHARED is set;
        // ENOTSUP for a flag the implementation does not support. A shared
        // anonymous mapping must stay shared with a forked child, and the
        // layer has no shared memory yet, so MAP_SHARED is refused.
        let kinds = crate::header::sys_mman::MAP_PRIVATE | crate::header::sys_mman::MAP_SHARED;
        if flags & kinds == 0 {
            return Err(Errno(EINVAL));
        }
        if flags & crate::header::sys_mman::MAP_SHARED != 0 {
            return Err(Errno(crate::header::errno::ENOTSUP));
        }
        // Anonymous private memory only, from the layer's heap.
        if fildes != -1 || flags & crate::header::sys_mman::MAP_ANONYMOUS == 0 || !addr.is_null() {
            return Err(Errno(ENOSYS));
        }
        let pointer = unsafe { stafeto_mmap_anonymous(len) };
        if pointer.is_null() {
            Err(Errno(crate::header::errno::ENOMEM))
        } else {
            Ok(pointer)
        }
    }

    unsafe fn mremap(
        addr: *mut c_void,
        len: usize,
        new_len: usize,
        flags: c_int,
        args: *mut c_void,
    ) -> Result<*mut c_void> {
        Err(Errno(ENOSYS))
    }

    unsafe fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn msync(addr: *mut c_void, len: usize, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn munlock(addr: *const c_void, len: usize) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn munlockall() -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn munmap(addr: *mut c_void, len: usize) -> Result<()> {
        ret(unsafe { stafeto_munmap(addr, len) } as isize).map(|_| ())
    }

    unsafe fn madvise(addr: *mut c_void, len: usize, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn nanosleep(rqtp: *const timespec, rmtp: *mut timespec) -> Result<()> {
        ret(unsafe { stafeto_nanosleep(rqtp, rmtp) } as isize).map(|_| ())
    }

    fn openat(dirfd: c_int, path: CStr, oflag: c_int, mode: mode_t) -> Result<c_int> {
        ret(unsafe { stafeto_openat(dirfd, path.as_ptr(), oflag, mode) } as isize)
            .map(|v| v as c_int)
    }

    fn pipe2(mut fildes: Out<[c_int; 2]>, flags: c_int) -> Result<()> {
        let mut ends = [0; 2];
        ret(unsafe { stafeto_pipe2(ends.as_mut_ptr(), flags) } as isize)?;
        fildes.write(ends);
        Ok(())
    }

    fn posix_fallocate(fd: c_int, offset: u64, length: NonZeroU64) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn posix_getdents(fildes: c_int, buf: &mut [u8]) -> Result<usize> {
        ret(unsafe { stafeto_posix_getdents(fildes, buf.as_mut_ptr(), buf.len()) })
            .map(|used| used as usize)
    }

    unsafe fn rlct_clone(
        stack: *mut usize,
        os_specific: &mut OsSpecific,
    ) -> Result<crate::pthread::OsTid> {
        let block = os_specific.0.get().cast();
        ret(unsafe { stafeto_thread_create(stack, block) } as isize).map(|id| {
            crate::pthread::OsTid {
                thread_id: id as usize,
            }
        })
    }

    unsafe fn rlct_kill(os_tid: crate::pthread::OsTid, signal: usize) -> Result<()> {
        signal::thread_kill(os_tid, signal)
    }

    fn current_os_tid() -> crate::pthread::OsTid {
        crate::pthread::OsTid {
            thread_id: unsafe { stafeto_thread_id() } as usize,
        }
    }

    fn read(fildes: c_int, buf: &mut [u8]) -> Result<usize> {
        ret(unsafe { stafeto_read(fildes, buf.as_mut_ptr(), buf.len()) }).map(|v| v as usize)
    }
    fn pread(fildes: c_int, buf: &mut [u8], off: off_t) -> Result<usize> {
        ret(unsafe { stafeto_pread(fildes, buf.as_mut_ptr(), buf.len(), off) }).map(|v| v as usize)
    }

    unsafe fn readv(fildes: c_int, iov: *const iovec, iovcnt: c_int) -> Result<usize> {
        // One read, as read(2) makes it: what is there now, at most the
        // layer's extent (proto_fs::MAX_READ), spread over the parts in
        // order. A second read would wait on a pipe that held exactly what
        // the first part took; POSIX gives a pipe's reader what is there.
        // POSIX: EINVAL for iovcnt outside 1..=IOV_MAX.
        if !(1..=1024).contains(&iovcnt) {
            return Err(Errno(EINVAL));
        }
        let parts = unsafe { core::slice::from_raw_parts(iov, iovcnt as usize) };
        // The parts up to one without its buffer, which the read never
        // reaches (EFAULT when it is the first).
        let parts = valid_parts(parts)?;
        let total = parts
            .iter()
            .fold(0usize, |sum, p| sum.saturating_add(p.iov_len));
        if let Some(first) = parts.iter().find(|p| p.iov_len > 0)
            && first.iov_len >= total.min(STAFETO_READ_MAX)
        {
            // The first part takes all one read gives: no copy.
            return ret(unsafe { stafeto_read(fildes, first.iov_base.cast(), first.iov_len) })
                .map(|got| got as usize);
        }
        let mut bytes = [0u8; STAFETO_READ_MAX];
        let want = total.min(bytes.len());
        let got = ret(unsafe { stafeto_read(fildes, bytes.as_mut_ptr(), want) })? as usize;
        let mut at = 0;
        for part in parts {
            if at == got {
                break;
            }
            let n = part.iov_len.min(got - at);
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr().add(at), part.iov_base.cast::<u8>(), n)
            };
            at += n;
        }
        Ok(got)
    }

    fn readlinkat(dirfd: c_int, pathname: CStr, out: &mut [u8]) -> Result<usize> {
        ret(unsafe { stafeto_readlinkat(dirfd, pathname.as_ptr(), out.as_mut_ptr(), out.len()) })
            .map(|len| len as usize)
    }

    fn renameat2(
        old_dir: c_int,
        old_path: CStr,
        new_dir: c_int,
        new_path: CStr,
        flags: c_uint,
    ) -> Result<()> {
        // The flags of renameat2 (RENAME_NOREPLACE and the others) are not
        // taken.
        if flags != 0 {
            return Err(Errno(EINVAL));
        }
        ret(
            unsafe { stafeto_renameat(old_dir, old_path.as_ptr(), new_dir, new_path.as_ptr()) }
                as isize,
        )
        .map(|_| ())
    }

    fn sched_yield() -> Result<()> {
        ret(unsafe { stafeto_sched_yield() } as isize).map(|_| ())
    }

    unsafe fn setgroups(size: size_t, list: *const gid_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setpgid(pid: pid_t, pgid: pid_t) -> Result<()> {
        ret(unsafe { stafeto_setpgid(pid, pgid) } as isize).map(|_| ())
    }

    fn setpriority(which: c_int, who: id_t, prio: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setresgid(rgid: gid_t, egid: gid_t, sgid: gid_t) -> Result<()> {
        ret(unsafe { stafeto_setresgid(rgid, egid, sgid) } as isize).map(|_| ())
    }

    fn setresuid(ruid: uid_t, euid: uid_t, suid: uid_t) -> Result<()> {
        ret(unsafe { stafeto_setresuid(ruid, euid, suid) } as isize).map(|_| ())
    }

    fn setsid() -> Result<c_int> {
        ret(unsafe { stafeto_setsid() } as isize).map(|v| v as c_int)
    }

    fn symlinkat(path1: CStr, fd: c_int, path2: CStr) -> Result<()> {
        ret(unsafe { stafeto_symlinkat(path1.as_ptr(), fd, path2.as_ptr()) } as isize).map(|_| ())
    }

    fn sync() -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn timer_create(clock_id: clockid_t, evp: &sigevent) -> Result<timer_t> {
        Err(Errno(ENOSYS))
    }

    fn timer_delete(timerid: timer_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn timer_gettime(timerid: timer_t) -> Result<itimerspec> {
        Err(Errno(ENOSYS))
    }

    fn timer_settime(
        timerid: timer_t,
        flags: c_int,
        value: &itimerspec,
        ovalue: Option<Out<itimerspec>>,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn umask(mask: mode_t) -> mode_t {
        unsafe { stafeto_umask(mask) }
    }

    fn uname(mut utsname: Out<utsname>) -> Result<()> {
        ret(unsafe { stafeto_uname(utsname.as_mut_ptr()) } as isize).map(|_| ())
    }

    fn unlinkat(fd: c_int, path: CStr, flags: c_int) -> Result<()> {
        ret(unsafe { stafeto_unlinkat(fd, path.as_ptr(), flags) } as isize).map(|_| ())
    }

    fn waitpid(pid: pid_t, stat_loc: Option<Out<c_int>>, options: c_int) -> Result<pid_t> {
        let mut status = 0;
        let pid = ret(unsafe { stafeto_waitpid(pid, &raw mut status, options) } as isize)?;
        if let Some(mut out) = stat_loc {
            out.write(status);
        }
        Ok(pid as pid_t)
    }

    fn waitid(
        idtype: crate::header::sys_wait::idtype_t,
        id: id_t,
        infop: *mut crate::header::signal::siginfo_t,
        options: c_int,
    ) -> Result<()> {
        if infop.is_null() {
            return Err(Errno(EINVAL));
        }
        ret(unsafe { stafeto_waitid(idtype, id, infop.cast(), options) } as isize).map(|_| ())
    }

    fn write(fildes: c_int, buf: &[u8]) -> Result<usize> {
        ret(unsafe { stafeto_write(fildes, buf.as_ptr(), buf.len()) }).map(|v| v as usize)
    }
    fn pwrite(fildes: c_int, buf: &[u8], off: off_t) -> Result<usize> {
        ret(unsafe { stafeto_pwrite(fildes, buf.as_ptr(), buf.len(), off) }).map(|v| v as usize)
    }

    unsafe fn writev(fildes: c_int, iov: *const iovec, iovcnt: c_int) -> Result<usize> {
        // POSIX: EINVAL for iovcnt outside 1..=IOV_MAX.
        if !(1..=1024).contains(&iovcnt) {
            return Err(Errno(EINVAL));
        }
        let all = unsafe { core::slice::from_raw_parts(iov, iovcnt as usize) };
        let parts = valid_parts(all)?;
        let total = parts
            .iter()
            .fold(0usize, |sum, p| sum.saturating_add(p.iov_len));
        // Up to {PIPE_BUF} bytes go as one write: a pipe takes them whole,
        // and no other writer's bytes come between the parts.
        if parts.len() > 1 && total <= crate::header::limits::PIPE_BUF as usize {
            let mut bytes = [0u8; crate::header::limits::PIPE_BUF as usize];
            let mut at = 0;
            for part in parts {
                unsafe {
                    ptr::copy_nonoverlapping(
                        part.iov_base.cast::<u8>(),
                        bytes.as_mut_ptr().add(at),
                        part.iov_len,
                    )
                };
                at += part.iov_len;
            }
            return ret(unsafe { stafeto_write(fildes, bytes.as_ptr(), at) }).map(|n| n as usize);
        }
        // Past it, each part in turn; a short write ends the call.
        let mut total = 0;
        for part in all {
            // An error after bytes moved gives the bytes; the next call
            // meets the error.
            let wrote =
                match ret(unsafe { stafeto_write(fildes, part.iov_base.cast(), part.iov_len) }) {
                    Ok(wrote) => wrote,
                    Err(_) if total > 0 => break,
                    Err(error) => return Err(error),
                };
            total += wrote as usize;
            if (wrote as usize) < part.iov_len {
                break;
            }
        }
        Ok(total)
    }

    fn verify() -> bool {
        true
    }

    /// The layer's loader starts the program in the file at `program`
    /// with `argv`, `envp`, the attributes and the file actions, which
    /// the layer applies to a copy of the caller's descriptors (stafeto
    /// 5c).
    unsafe fn spawn(
        program: CStr,
        fac: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
        fat: Option<&crate::header::spawn::posix_spawnattr_t>,
        argv: crate::iter::NulTerminated<*mut c_char>,
        envp: Option<crate::iter::NulTerminated<*mut c_char>>,
    ) -> Result<pid_t> {
        unsafe { spawn_search(program, None, fac, fat, argv, envp) }
    }

    unsafe fn spawnp(
        file: CStr,
        path: Option<CStr>,
        fac: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
        fat: Option<&crate::header::spawn::posix_spawnattr_t>,
        argv: crate::iter::NulTerminated<*mut c_char>,
        envp: Option<crate::iter::NulTerminated<*mut c_char>>,
    ) -> Result<pid_t> {
        // PATH is searched by the layer, after the file actions.
        unsafe { spawn_search(file, Some(path), fac, fat, argv, envp) }
    }
}

/// posix_spawn of `program` (`search` None), or posix_spawnp of the name
/// `program` in the directories of the PATH `search` holds (Some(None) for
/// no PATH): the file actions and the attributes go to the layer.
unsafe fn spawn_search(
    program: CStr,
    search: Option<Option<CStr>>,
    fac: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
    fat: Option<&crate::header::spawn::posix_spawnattr_t>,
    mut argv: crate::iter::NulTerminated<*mut c_char>,
    envp: Option<crate::iter::NulTerminated<*mut c_char>>,
) -> Result<pid_t> {
    {
        use crate::header::spawn::Action;
        // The actions own their paths, which the layer reads through the
        // list built from them.
        let owned: alloc::vec::Vec<Action> = fac.map_or(alloc::vec::Vec::new(), |actions| {
            actions.into_iter().collect()
        });
        let file_actions: alloc::vec::Vec<SpawnAction> = owned
            .iter()
            .map(|action| {
                let (kind, fd, newfd, flags, mode, path) = match action {
                    Action::Open {
                        fd,
                        path,
                        flag,
                        mode,
                    } => (1, *fd, 0, *flag, *mode as u32, path.as_ptr()),
                    Action::Close(fd) => (2, *fd, 0, 0, 0, core::ptr::null()),
                    Action::Dup2(fd, newfd) => (3, *fd, *newfd, 0, 0, core::ptr::null()),
                    Action::Chdir(path) => (4, 0, 0, 0, 0, path.as_ptr()),
                    Action::FChdir(fd) => (5, *fd, 0, 0, 0, core::ptr::null()),
                };
                SpawnAction {
                    kind,
                    fd,
                    newfd,
                    flags,
                    mode,
                    path,
                }
            })
            .collect();
        static NONE: [usize; 1] = [0];
        let none = NONE.as_ptr().cast::<*const c_char>();
        // The first element's place is the array's: each list is NULL-ended.
        let list = |first: Option<&*mut c_char>| {
            first.map_or(none, |p| (p as *const *mut c_char).cast::<*const c_char>())
        };
        let argv = list(argv.next());
        let envp = envp.map_or(none, |mut e| list(e.next()));
        let attributes = fat.map(|attr| SpawnAttributes {
            flags: c_int::from(attr.flags),
            pgroup: attr.pgroup,
            mask: attr.sigmask,
            default: attr.sigdefault,
        });
        let attributes = attributes
            .as_ref()
            .map_or(core::ptr::null(), |a| a as *const SpawnAttributes);
        ret(unsafe {
            match search {
                None => stafeto_spawn(
                    program.as_ptr(),
                    argv,
                    envp,
                    attributes,
                    file_actions.as_ptr(),
                    file_actions.len(),
                ),
                Some(path) => stafeto_spawnp(
                    program.as_ptr(),
                    path.map_or(ptr::null(), |path| path.as_ptr()),
                    argv,
                    envp,
                    attributes,
                    file_actions.as_ptr(),
                    file_actions.len(),
                ),
            }
        } as isize)
        .map(|v| v as pid_t)
    }
}
