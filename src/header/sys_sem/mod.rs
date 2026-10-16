//! `sys/sem.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_sem.h.html>.
//!
//! The XSI semaphore sets live in the pipe service of stafeto: at most 16
//! semaphores in a set, values up to 32767, eight operations in a `semop`.

use crate::{
    header::{
        errno::{E2BIG, EFAULT, EINVAL},
        sys_ipc::{IPC_RMID, IPC_SET, IPC_STAT, ipc_perm},
    },
    platform::{
        self, ipc,
        types::{c_int, c_short, c_ulong, c_ushort, c_void, key_t, size_t, time_t},
    },
};

/// Set up adjustment on exit.
pub const SEM_UNDO: c_int = 0x1000;
/// Get the process ID of the last operation.
pub const GETPID: c_int = 11;
/// Get the value of one semaphore.
pub const GETVAL: c_int = 12;
/// Get the values of all semaphores.
pub const GETALL: c_int = 13;
/// Get the number of processes that wait for an increase.
pub const GETNCNT: c_int = 14;
/// Get the number of processes that wait for zero.
pub const GETZCNT: c_int = 15;
/// Set the value of one semaphore.
pub const SETVAL: c_int = 16;
/// Set the values of all semaphores.
pub const SETALL: c_int = 17;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_sem.h.html>.
#[repr(C)]
pub struct semid_ds {
    pub sem_perm: ipc_perm,
    pub sem_otime: time_t,
    pub sem_ctime: time_t,
    pub sem_nsems: c_ulong,
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_sem.h.html>.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sembuf {
    pub sem_num: c_ushort,
    pub sem_op: c_short,
    pub sem_flg: c_short,
}

fn fail(errno: c_int) -> c_int {
    platform::ERRNO.set(errno);
    -1
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/semget.html>.
#[unsafe(no_mangle)]
pub extern "C" fn semget(key: key_t, nsems: c_int, semflg: c_int) -> c_int {
    if nsems < 0 {
        return fail(EINVAL);
    }
    let words = [
        u64::from(key as u32),
        nsems as u64,
        semflg as u64,
        0,
    ];
    match ipc::ipc(ipc::op::SEM_GET, &words, 3, &[], &mut [], ipc::NO_DEADLINE) {
        Ok(reply) => reply.words[0] as c_int,
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/semop.html>.
///
/// # Safety
/// `sops` points to `nsops` operations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn semop(semid: c_int, sops: *mut sembuf, nsops: size_t) -> c_int {
    if nsops == 0 || nsops > 8 {
        return fail(if nsops == 0 { EINVAL } else { E2BIG });
    }
    if sops.is_null() {
        return fail(EFAULT);
    }
    let list = unsafe { core::slice::from_raw_parts(sops, nsops) };
    let mut words = [0u64; 2 + 1 + 3 * 8];
    words[0] = semid as u32 as u64;
    words[1] = nsops as u64;
    for (i, op) in list.iter().enumerate() {
        words[3 + 3 * i] = u64::from(op.sem_num);
        words[4 + 3 * i] = op.sem_op as u16 as u64;
        words[5 + 3 * i] = op.sem_flg as u16 as u64;
    }
    match ipc::ipc(ipc::op::SEM_OP, &words[..3 + 3 * nsops], 2, &[], &mut [], ipc::NO_DEADLINE) {
        Ok(_) => 0,
        Err(errno) => fail(errno),
    }
}

/// The count of semaphores of the set (IPC_STAT).
fn count(semid: c_int) -> Result<usize, c_int> {
    let words = [semid as u32 as u64, 0, IPC_STAT as u64, 0, 0];
    ipc::ipc(ipc::op::SEM_CTL, &words, 4, &[], &mut [], ipc::NO_DEADLINE)
        .map(|reply| reply.words[9] as usize)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/semctl.html>.
///
/// # Safety
/// The fourth argument is a `union semun` (a value, or a pointer to a
/// `semid_ds` or to an array of `unsigned short`) where `cmd` takes one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn semctl(
    semid: c_int,
    semnum: c_int,
    cmd: c_int,
    mut __valist: ...
) -> c_int {
    let id = semid as u32 as u64;
    let num = semnum as u32 as u64;
    let arg = match cmd {
        GETVAL | GETPID | GETNCNT | GETZCNT | IPC_RMID => 0,
        _ => unsafe { __valist.next_arg::<u64>() },
    };
    let mut words = [id, num, cmd as u32 as u64, 0, 0, 0, 0, 0];
    match cmd {
        IPC_RMID | GETVAL | GETPID | GETNCNT | GETZCNT => {
            match ipc::ipc(ipc::op::SEM_CTL, &words[..5], 4, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(reply) => {
                    if cmd == IPC_RMID {
                        0
                    } else {
                        reply.words[0] as c_int
                    }
                }
                Err(errno) => fail(errno),
            }
        }
        SETVAL => {
            words[3] = u64::from(arg as u32);
            match ipc::ipc(ipc::op::SEM_CTL, &words[..5], 4, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(_) => 0,
                Err(errno) => fail(errno),
            }
        }
        IPC_STAT => {
            let buf = arg as *mut semid_ds;
            if buf.is_null() {
                return fail(EFAULT);
            }
            match ipc::ipc(ipc::op::SEM_CTL, &words[..5], 4, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(r) => {
                    let ds = unsafe { &mut *buf };
                    ds.sem_perm.__key = r.words[0] as key_t;
                    ds.sem_perm.uid = r.words[1] as _;
                    ds.sem_perm.gid = r.words[2] as _;
                    ds.sem_perm.cuid = r.words[3] as _;
                    ds.sem_perm.cgid = r.words[4] as _;
                    ds.sem_perm.mode = r.words[5] as _;
                    ds.sem_perm.__seq = r.words[6] as _;
                    ds.sem_otime = r.words[7] as time_t;
                    ds.sem_ctime = r.words[8] as time_t;
                    ds.sem_nsems = r.words[9] as c_ulong;
                    0
                }
                Err(errno) => fail(errno),
            }
        }
        IPC_SET => {
            let buf = arg as *const semid_ds;
            if buf.is_null() {
                return fail(EFAULT);
            }
            let ds = unsafe { &*buf };
            words[5] = u64::from(ds.sem_perm.uid as u32);
            words[6] = u64::from(ds.sem_perm.gid as u32);
            words[7] = u64::from(ds.sem_perm.mode as u32);
            match ipc::ipc(ipc::op::SEM_CTL, &words[..8], 4, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(_) => 0,
                Err(errno) => fail(errno),
            }
        }
        GETALL => {
            let array = arg as *mut c_ushort;
            if array.is_null() {
                return fail(EFAULT);
            }
            let mut bytes = [0u8; 32];
            match ipc::ipc(ipc::op::SEM_CTL, &words[..5], 4, &[], &mut bytes, ipc::NO_DEADLINE) {
                Ok(reply) => {
                    for i in 0..reply.len / 2 {
                        unsafe {
                            array
                                .add(i)
                                .write(c_ushort::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]));
                        }
                    }
                    0
                }
                Err(errno) => fail(errno),
            }
        }
        SETALL => {
            let array = arg as *const c_ushort;
            if array.is_null() {
                return fail(EFAULT);
            }
            let n = match count(semid) {
                Ok(n) if n <= 16 => n,
                Ok(_) => return fail(EINVAL),
                Err(errno) => return fail(errno),
            };
            let mut bytes = [0u8; 32];
            for i in 0..n {
                let v = unsafe { array.add(i).read() };
                bytes[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
            }
            match ipc::ipc(ipc::op::SEM_CTL, &words[..5], 4, &bytes[..2 * n], &mut [], ipc::NO_DEADLINE) {
                Ok(_) => 0,
                Err(errno) => fail(errno),
            }
        }
        _ => fail(EINVAL),
    }
}

#[allow(dead_code)]
const _: Option<*const c_void> = None;
