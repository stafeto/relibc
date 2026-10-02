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
        dirent::dirent,
        errno::{EINVAL, EIO, ENOSYS},
        fcntl::AT_EMPTY_PATH,
        signal::{SIGCHLD, sigevent},
        sys_resource::{rlimit, rusage},
        sys_select::timeval,
        sys_stat::{S_IFIFO, stat},
        sys_statvfs::statvfs,
        sys_time::timezone,
        sys_uio::iovec,
        sys_utsname::utsname,
        time::{itimerspec, timespec},
        unistd::{SEEK_CUR, SEEK_SET},
    },
    ld_so::tcb::OsSpecific,
    out::Out,
};
use core::{num::NonZeroU64, ptr};

unsafe extern "C" {
    fn stafeto_write(fd: c_int, buf: *const u8, len: usize) -> isize;
    fn stafeto_read(fd: c_int, buf: *mut u8, len: usize) -> isize;
    fn stafeto_openat(dirfd: c_int, path: *const c_char, flags: c_int, mode: mode_t) -> c_int;
    fn stafeto_close(fd: c_int) -> c_int;
    fn stafeto_lseek(fd: c_int, offset: off_t, whence: c_int) -> off_t;
    pub(crate) fn stafeto_exit(status: c_int) -> !;
    fn stafeto_clock_gettime(clock: clockid_t, out: *mut timespec) -> c_int;
    fn stafeto_clock_getres(clock: clockid_t, out: *mut timespec) -> c_int;
    fn stafeto_mmap_anonymous(len: usize) -> *mut c_void;
    fn stafeto_munmap(addr: *mut c_void, len: usize) -> c_int;
    fn stafeto_getpid() -> pid_t;
    fn stafeto_getppid() -> pid_t;
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

/// The stafeto implementation of [`Pal`].
pub struct Sys;

impl Sys {
    pub unsafe fn ioctl(fd: c_int, request: c_ulong, out: *mut c_void) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }
}

impl Pal for Sys {
    fn faccessat(fd: c_int, path: CStr, amode: c_int, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn brk(addr: *mut c_void) -> Result<*mut c_void> {
        Err(Errno(ENOSYS))
    }

    fn chdir(path: CStr) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fchownat(fildes: c_int, path: CStr, owner: uid_t, group: gid_t, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
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
        Err(Errno(ENOSYS))
    }

    fn close(fildes: c_int) -> Result<()> {
        ret(unsafe { stafeto_close(fildes) } as isize).map(|_| ())
    }

    fn dup(fildes: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn dup2(fildes: c_int, fildes2: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    unsafe fn execve(path: CStr, argv: *const *mut c_char, envp: *const *mut c_char) -> Result<()> {
        Err(Errno(ENOSYS))
    }
    unsafe fn fexecve(
        fildes: c_int,
        argv: *const *mut c_char,
        envp: *const *mut c_char,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn exit(status: c_int) -> ! {
        unsafe { stafeto_exit(status) }
    }
    unsafe fn exit_thread(_stack_base: *mut (), _stack_size: usize) -> ! {
        unsafe { stafeto_exit(0) }
    }

    fn fchdir(fildes: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fchmod(fildes: c_int, mode: mode_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fchmodat(dirfd: c_int, path: Option<CStr>, mode: mode_t, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fdatasync(fildes: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn flock(fd: c_int, operation: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fstatat(fildes: c_int, path: Option<CStr>, mut buf: Out<stat>, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fstatvfs(fildes: c_int, buf: Out<statvfs>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn fcntl(fildes: c_int, cmd: c_int, arg: c_ulonglong) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    unsafe fn fork() -> Result<pid_t> {
        Err(Errno(ENOSYS))
    }

    fn fpath(fildes: c_int, out: &mut [u8]) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn fsync(fildes: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn ftruncate(fildes: c_int, length: off_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    #[inline]
    unsafe fn futex_wait(addr: *mut u32, val: u32, deadline: Option<&timespec>) -> Result<()> {
        // One thread until the layer's waits by address come: nobody
        // else can change the word, so a wait would never end.
        if unsafe { core::ptr::read_volatile(addr) } != val {
            return Err(Errno(crate::header::errno::EAGAIN));
        }
        Err(Errno(ENOSYS))
    }
    #[inline]
    unsafe fn futex_wake(addr: *mut u32, num: u32) -> Result<u32> {
        // One thread: nobody waits.
        Ok(0)
    }

    unsafe fn utimensat(
        dirfd: c_int,
        path: CStr,
        times: *const timespec,
        flag: c_int,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn getcwd(buf: Out<[u8]>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn getdents(fd: c_int, buf: &mut [u8], _off: u64) -> Result<usize> {
        Err(Errno(ENOSYS))
    }
    fn dir_seek(fd: c_int, off: u64) -> Result<()> {
        Err(Errno(ENOSYS))
    }
    // FIXME use offset or remove it
    unsafe fn dent_reclen_offset(this_dent: &[u8], _offset: usize) -> Option<(u16, u64)> {
        None
    }

    fn getegid() -> gid_t {
        0
    }

    fn geteuid() -> uid_t {
        0
    }

    fn getgid() -> gid_t {
        0
    }

    fn getgroups(list: Out<[gid_t]>) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn getpagesize() -> usize {
        4096
    }

    fn getpgid(pid: pid_t) -> Result<pid_t> {
        Err(Errno(ENOSYS))
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
        Err(Errno(ENOSYS))
    }

    fn getrlimit(resource: c_int, rlim: Out<rlimit>) -> Result<()> {
        Err(Errno(ENOSYS))
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
        Err(Errno(ENOSYS))
    }

    fn gettid() -> pid_t {
        unsafe { stafeto_getpid() }
    }

    fn gettimeofday(tp: Out<timeval>, tzp: Option<Out<timezone>>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn getuid() -> uid_t {
        0
    }

    fn linkat(fd1: c_int, path1: CStr, fd2: c_int, path2: CStr, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn lseek(fildes: c_int, offset: off_t, whence: c_int) -> Result<off_t> {
        ret(unsafe { stafeto_lseek(fildes, offset, whence) } as isize).map(|v| v as off_t)
    }

    fn mkdirat(dir_fildes: c_int, path: CStr, mode: mode_t) -> Result<()> {
        Err(Errno(ENOSYS))
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
        Err(Errno(ENOSYS))
    }

    fn openat(dirfd: c_int, path: CStr, oflag: c_int, mode: mode_t) -> Result<c_int> {
        ret(unsafe { stafeto_openat(dirfd, path.as_ptr(), oflag, mode) } as isize)
            .map(|v| v as c_int)
    }

    fn pipe2(fildes: Out<[c_int; 2]>, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn posix_fallocate(fd: c_int, offset: u64, length: NonZeroU64) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn posix_getdents(fildes: c_int, buf: &mut [u8]) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn rlct_clone(
        stack: *mut usize,
        _os_specific: &mut OsSpecific,
    ) -> Result<crate::pthread::OsTid> {
        Err(Errno(ENOSYS))
    }

    unsafe fn rlct_kill(os_tid: crate::pthread::OsTid, signal: usize) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn current_os_tid() -> crate::pthread::OsTid {
        crate::pthread::OsTid { thread_id: 1 }
    }

    fn read(fildes: c_int, buf: &mut [u8]) -> Result<usize> {
        ret(unsafe { stafeto_read(fildes, buf.as_mut_ptr(), buf.len()) }).map(|v| v as usize)
    }
    fn pread(fildes: c_int, buf: &mut [u8], off: off_t) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn readv(fildes: c_int, iov: *const iovec, iovcnt: c_int) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn readlinkat(dirfd: c_int, pathname: CStr, out: &mut [u8]) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn renameat2(
        old_dir: c_int,
        old_path: CStr,
        new_dir: c_int,
        new_path: CStr,
        flags: c_uint,
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn sched_yield() -> Result<()> {
        Err(Errno(ENOSYS))
    }

    unsafe fn setgroups(size: size_t, list: *const gid_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setpgid(pid: pid_t, pgid: pid_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setpriority(which: c_int, who: id_t, prio: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setresgid(rgid: gid_t, egid: gid_t, sgid: gid_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setresuid(ruid: uid_t, euid: uid_t, suid: uid_t) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn setsid() -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn symlinkat(path1: CStr, fd: c_int, path2: CStr) -> Result<()> {
        Err(Errno(ENOSYS))
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
        0o022
    }

    fn uname(utsname: Out<utsname>) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn unlinkat(fd: c_int, path: CStr, flags: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn waitpid(pid: pid_t, stat_loc: Option<Out<c_int>>, options: c_int) -> Result<pid_t> {
        Err(Errno(ENOSYS))
    }

    fn write(fildes: c_int, buf: &[u8]) -> Result<usize> {
        ret(unsafe { stafeto_write(fildes, buf.as_ptr(), buf.len()) }).map(|v| v as usize)
    }
    fn pwrite(fildes: c_int, buf: &[u8], off: off_t) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn writev(fildes: c_int, iov: *const iovec, iovcnt: c_int) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn verify() -> bool {
        true
    }

    #[expect(unused_variables, reason = "function not yet implemented")]
    unsafe fn spawn(
        program: CStr,
        fac: Option<&crate::header::spawn::posix_spawn_file_actions_t>,
        fat: Option<&crate::header::spawn::posix_spawnattr_t>,
        argv: crate::iter::NulTerminated<*mut c_char>,
        envp: Option<crate::iter::NulTerminated<*mut c_char>>,
    ) -> Result<pid_t> {
        Err(Errno(ENOSYS))
    }
}
