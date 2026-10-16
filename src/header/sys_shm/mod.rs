//! `sys/shm.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_shm.h.html>.
//!
//! The segments are accounted for by the pipe service of stafeto (keys,
//! permissions, the count of attachments); their pages are a file of
//! `/dev/shm` named `.sysvN`, which this library makes at `shmget` and maps at
//! `shmat`. The file goes with the segment: at `IPC_RMID` when nothing is
//! attached, at the last `shmdt` otherwise. A segment is at most 1 MiB, eight
//! in all.

use alloc::{ffi::CString, format};
use core::ptr;

use crate::{
    header::{
        errno::{EFAULT, EINVAL},
        fcntl::{O_CREAT, O_EXCL, O_RDONLY, O_RDWR},
        sys_stat::{chmod, fchmod, fstat},
        sys_ipc::{IPC_RMID, IPC_SET, IPC_STAT, ipc_perm},
        sys_mman::{MAP_SHARED, PROT_READ, PROT_WRITE, shm_open, shm_unlink},
        unistd::{chown, close, ftruncate},
    },
    platform::{
        self, ipc,
        types::{c_char, c_int, c_void, key_t, mode_t, pid_t, size_t, time_t},
    },
    sync::Mutex,
};

#[allow(non_camel_case_types)]
pub type shmatt_t = core::ffi::c_ulong;

/// Attach read-only (else read-write).
pub const SHM_RDONLY: c_int = 0o10000;
/// Round attach address to SHMLBA.
pub const SHM_RND: c_int = 0o20000;
/// Segment low boundary address multiple.
pub const SHMLBA: size_t = 4096;

/// Return value of `shmat()` indicating shared memory has not been attached.
pub const SHM_FAILED: *mut c_void = -1isize as *mut c_void;

#[repr(C)]
pub struct shmid_ds {
    pub shm_perm: ipc_perm,
    pub shm_segsz: size_t,
    pub shm_lpid: pid_t,
    pub shm_cpid: pid_t,
    pub shm_nattch: shmatt_t,
    pub shm_atime: time_t,
    pub shm_dtime: time_t,
    pub shm_ctime: time_t,
}

fn fail(errno: c_int) -> c_int {
    platform::ERRNO.set(errno);
    -1
}

/// The segments this process attached: where, how long, which.
#[derive(Clone, Copy)]
struct Attached {
    base: usize,
    len: usize,
    id: u64,
    serial: u64,
}

const ATTACHED_MAX: usize = 32;
static ATTACHED: Mutex<[Option<Attached>; ATTACHED_MAX]> = Mutex::new([None; ATTACHED_MAX]);

fn file_name(serial: u64) -> CString {
    CString::new(format!("/.sysv{serial:016x}")).expect("no NUL in a name")
}

unsafe fn remove_file(serial: u64) {
    if serial == 0 {
        return;
    }
    let name = file_name(serial);
    unsafe { shm_unlink(name.as_ptr()) };
}

/// The path of the file of a segment in /dev/shm.
fn file_path(serial: u64) -> CString {
    CString::new(format!("/dev/shm/.sysv{serial:016x}")).expect("no NUL in a path")
}

/// A number nobody can guess before the file of the segment is there: the
/// process's generator, keyed by the entropy service.
fn nonce() -> u64 {
    let mut bytes = [0u8; 8];
    for _ in 0..4 {
        let n = unsafe {
            crate::header::sys_random::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0)
        };
        let value = u64::from_ne_bytes(bytes);
        if n == 8 && value != 0 {
            return value;
        }
    }
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/shmget.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shmget(key: key_t, size: size_t, shmflg: c_int) -> c_int {
    // The file of a new segment is named by a number drawn here; a file of
    // that name already there (it cannot be, unless the draw was bad) draws
    // again.
    for _ in 0..5 {
        let words = [
            u64::from(key as u32),
            size as u64,
            shmflg as u64,
            nonce(),
            0,
        ];
        let reply = match ipc::ipc(ipc::op::SHM_GET, &words, 4, &[], &mut [], ipc::NO_DEADLINE) {
            Ok(reply) => reply,
            Err(crate::header::errno::EEXIST) if key == 0 => continue,
            Err(errno) => return fail(errno),
        };
        let (id, serial, created) = (reply.words[0], reply.words[1], reply.words[2] != 0);
        // A file the service says is left of a freed segment of this user.
        unsafe { remove_file(reply.words[3]) };
        if !created {
            return id as c_int;
        }
        // The pages: a file of /dev/shm, as long as the segment, owned by its
        // creator with the mode of the segment, so that the file service lets
        // in those the segment lets in (it knows who opens). It is made new
        // (O_EXCL) and closed to others (0600) until it has its mode.
        let name = file_name(serial);
        let fd = unsafe { shm_open(name.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600) };
        let taken = fd < 0 && platform::ERRNO.get() == crate::header::errno::EEXIST;
        let made = fd >= 0
            && ftruncate(fd, size as i64) == 0
            && fchmod(fd, (shmflg & 0o777) as mode_t) == 0;
        let errno = platform::ERRNO.get();
        if fd >= 0 {
            close(fd);
        }
        if made {
            return id as c_int;
        }
        // The segment goes; the file only if it is ours.
        if fd >= 0 {
            unsafe { remove_file(serial) };
        }
        unsafe { shmctl(id as c_int, IPC_RMID, ptr::null_mut()) };
        if taken {
            continue;
        }
        return fail(if errno == 0 { EINVAL } else { errno });
    }
    fail(crate::header::errno::EAGAIN)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/shmat.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shmat(
    shmid: c_int,
    shmaddr: *const c_void,
    shmflg: c_int,
) -> *mut c_void {
    if !shmaddr.is_null() {
        // The place of a mapping is the system's.
        platform::ERRNO.set(EINVAL);
        return SHM_FAILED;
    }
    let id = shmid as u32 as u64;
    let read_only = shmflg & SHM_RDONLY != 0;
    let words = [id, u64::from(read_only), 0];
    let reply = match ipc::ipc(ipc::op::SHM_AT, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
        Ok(reply) => reply,
        Err(errno) => {
            platform::ERRNO.set(errno);
            return SHM_FAILED;
        }
    };
    let (serial, len) = (reply.words[0], reply.words[1] as usize);
    let (owner, creator) = (reply.words[2] as u32, reply.words[3] as u32);
    unsafe { remove_file(reply.words[4]) };
    let detach = |errno: c_int| -> *mut c_void {
        let _ = ipc::ipc(ipc::op::SHM_DT, &[id, 0], 1, &[], &mut [], ipc::NO_DEADLINE);
        platform::ERRNO.set(errno);
        SHM_FAILED
    };
    let name = file_name(serial);
    let fd = unsafe { shm_open(name.as_ptr(), if read_only { O_RDONLY } else { O_RDWR }, 0) };
    if fd < 0 {
        return detach(platform::ERRNO.get());
    }
    // The file is the one the creator made: a regular file of one link, as
    // long as the segment, owned by the segment's owner or its creator. Another
    // user cannot put a file of that name in /dev/shm before or after.
    let mut status = crate::header::sys_stat::stat::default();
    let genuine = unsafe { fstat(fd, &raw mut status) } == 0
        && status.st_mode & crate::header::sys_stat::S_IFMT == crate::header::sys_stat::S_IFREG
        && status.st_nlink == 1
        && usize::try_from(status.st_size) == Ok(len)
        && (status.st_uid as u32 == owner || status.st_uid as u32 == creator);
    if !genuine {
        close(fd);
        return detach(crate::header::errno::EACCES);
    }
    let prot = if read_only { PROT_READ } else { PROT_READ | PROT_WRITE };
    let mapped = unsafe { crate::header::sys_mman::mmap(ptr::null_mut(), len, prot, MAP_SHARED, fd, 0) };
    close(fd);
    if mapped as isize == -1 {
        return detach(platform::ERRNO.get());
    }
    let mut table = ATTACHED.lock();
    match table.iter_mut().find(|slot| slot.is_none()) {
        Some(slot) => {
            *slot = Some(Attached {
                base: mapped as usize,
                len,
                id,
                serial,
            });
            mapped
        }
        None => {
            drop(table);
            unsafe { crate::header::sys_mman::munmap(mapped, len) };
            detach(crate::header::errno::EMFILE)
        }
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/shmdt.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shmdt(shmaddr: *const c_void) -> c_int {
    let attached = {
        let mut table = ATTACHED.lock();
        table
            .iter_mut()
            .find(|slot| slot.is_some_and(|a| a.base == shmaddr as usize))
            .and_then(Option::take)
    };
    let Some(a) = attached else {
        return fail(EINVAL);
    };
    let unmapped = unsafe { crate::header::sys_mman::munmap(a.base as *mut c_void, a.len) };
    let words = [a.id, 0];
    if let Ok(reply) = ipc::ipc(ipc::op::SHM_DT, &words, 1, &[], &mut [], ipc::NO_DEADLINE) {
        if reply.words[0] != 0 {
            unsafe { remove_file(reply.words[1]) };
        }
        unsafe { remove_file(reply.words[2]) };
    }
    unmapped
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/shmctl.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shmctl(shmid: c_int, cmd: c_int, buf: *mut shmid_ds) -> c_int {
    let id = shmid as u32 as u64;
    match cmd {
        IPC_RMID => {
            let words = [id, 0, 0];
            match ipc::ipc(ipc::op::SHM_CTL, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(reply) => {
                    if reply.words[0] != 0 {
                        unsafe { remove_file(reply.words[1]) };
                    }
                    unsafe { remove_file(reply.words[2]) };
                    0
                }
                Err(errno) => fail(errno),
            }
        }
        IPC_STAT => {
            if buf.is_null() {
                return fail(EFAULT);
            }
            let words = [id, IPC_STAT as u64, 0];
            match ipc::ipc(ipc::op::SHM_CTL, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(r) => {
                    let ds = unsafe { &mut *buf };
                    ds.shm_perm.__key = r.words[0] as key_t;
                    ds.shm_perm.uid = r.words[1] as _;
                    ds.shm_perm.gid = r.words[2] as _;
                    ds.shm_perm.cuid = r.words[3] as _;
                    ds.shm_perm.cgid = r.words[4] as _;
                    ds.shm_perm.mode = r.words[5] as mode_t;
                    ds.shm_perm.__seq = r.words[6] as _;
                    ds.shm_segsz = r.words[7] as size_t;
                    ds.shm_atime = r.words[8] as time_t;
                    ds.shm_dtime = r.words[9] as time_t;
                    ds.shm_ctime = r.words[10] as time_t;
                    ds.shm_cpid = r.words[11] as pid_t;
                    ds.shm_lpid = r.words[12] as pid_t;
                    ds.shm_nattch = r.words[13] as shmatt_t;
                    unsafe { remove_file(r.words[15]) };
                    0
                }
                Err(errno) => fail(errno),
            }
        }
        IPC_SET => {
            if buf.is_null() {
                return fail(EFAULT);
            }
            let ds = unsafe { &*buf };
            // What the segment is now, and its file.
            let words = [id, IPC_STAT as u64, 0];
            let now = match ipc::ipc(ipc::op::SHM_CTL, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(r) => r,
                Err(errno) => return fail(errno),
            };
            unsafe { remove_file(now.words[15]) };
            let (old_uid, old_gid, old_mode) = (now.words[1] as u32, now.words[2] as u32, now.words[5] as mode_t);
            let (uid, gid) = (ds.shm_perm.uid as u32, ds.shm_perm.gid as u32);
            let mode = (ds.shm_perm.mode & 0o777) as mode_t;
            let path = file_path(now.words[14]);
            // The file follows first, so that the rights of the file and of
            // the segment never part: a change the file cannot follow (a new
            // owner or group, which only the superuser may give a file) is
            // refused, and so is a mode its owner is not allowed to give.
            let moved = uid != old_uid || gid != old_gid;
            if moved && unsafe { chown(path.as_ptr(), uid as _, gid as _) } != 0 {
                return fail(crate::header::errno::EPERM);
            }
            if unsafe { chmod(path.as_ptr(), mode) } != 0 {
                if moved {
                    let _ = unsafe { chown(path.as_ptr(), old_uid as _, old_gid as _) };
                }
                return fail(crate::header::errno::EPERM);
            }
            let words = [
                id,
                IPC_SET as u64,
                0,
                u64::from(uid),
                u64::from(gid),
                u64::from(mode),
            ];
            match ipc::ipc(ipc::op::SHM_CTL, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(reply) => {
                    unsafe { remove_file(reply.words[2]) };
                    0
                }
                Err(errno) => {
                    // The segment did not change: nor does its file.
                    let _ = unsafe { chmod(path.as_ptr(), old_mode) };
                    if moved {
                        let _ = unsafe { chown(path.as_ptr(), old_uid as _, old_gid as _) };
                    }
                    fail(errno)
                }
            }
        }
        _ => fail(EINVAL),
    }
}

#[allow(dead_code)]
const _: Option<*const c_char> = None;
