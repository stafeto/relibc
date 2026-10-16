//! `sys/msg.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_msg.h.html>.
//!
//! The XSI message queues live in the pipe service of stafeto: at most 16
//! messages of 512 bytes in a queue, 8192 bytes by default.

use core::ptr;

use crate::{
    header::{
        errno::{EFAULT, EINVAL},
        sys_ipc::{IPC_RMID, IPC_SET, IPC_STAT, ipc_perm},
    },
    platform::{
        self, ipc,
        types::{c_char, c_int, c_long, c_ulong, c_void, key_t, pid_t, size_t, ssize_t, time_t},
    },
};

/// No error if the message is too long (the text is cut).
pub const MSG_NOERROR: c_int = 0o10000;
/// Receive the first message of a type other than `msgtyp` (GNU).
pub const MSG_EXCEPT: c_int = 0o20000;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_msg.h.html>.
#[allow(non_camel_case_types)]
pub type msgqnum_t = c_ulong;
/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_msg.h.html>.
#[allow(non_camel_case_types)]
pub type msglen_t = c_ulong;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_msg.h.html>.
#[repr(C)]
pub struct msqid_ds {
    pub msg_perm: ipc_perm,
    pub msg_stime: time_t,
    pub msg_rtime: time_t,
    pub msg_ctime: time_t,
    pub msg_cbytes: c_ulong,
    pub msg_qnum: msgqnum_t,
    pub msg_qbytes: msglen_t,
    pub msg_lspid: pid_t,
    pub msg_lrpid: pid_t,
}

fn fail(errno: c_int) -> c_int {
    platform::ERRNO.set(errno);
    -1
}

/// The text of a message is at most this long.
const TEXT_MAX: usize = 512;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/msgget.html>.
#[unsafe(no_mangle)]
pub extern "C" fn msgget(key: key_t, msgflg: c_int) -> c_int {
    let words = [u64::from(key as u32), msgflg as u64, 0];
    match ipc::ipc(ipc::op::MSG_GET, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
        Ok(reply) => reply.words[0] as c_int,
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/msgsnd.html>.
///
/// # Safety
/// `msgp` points to a `long` type and `msgsz` bytes of text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn msgsnd(
    msqid: c_int,
    msgp: *const c_void,
    msgsz: size_t,
    msgflg: c_int,
) -> c_int {
    if msgp.is_null() {
        return fail(EFAULT);
    }
    if msgsz > TEXT_MAX {
        return fail(EINVAL);
    }
    let kind = unsafe { msgp.cast::<c_long>().read() };
    let text = unsafe { core::slice::from_raw_parts(msgp.cast::<u8>().add(8), msgsz) };
    let nowait = u64::from(msgflg & crate::header::sys_ipc::IPC_NOWAIT != 0);
    let words = [msqid as u32 as u64, kind as u64, nowait, 0];
    match ipc::ipc(ipc::op::MSG_SND, &words, 3, text, &mut [], ipc::NO_DEADLINE) {
        Ok(_) => 0,
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/msgrcv.html>.
///
/// # Safety
/// `msgp` points to a `long` type and room for `msgsz` bytes of text.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn msgrcv(
    msqid: c_int,
    msgp: *mut c_void,
    msgsz: size_t,
    msgtyp: c_long,
    msgflg: c_int,
) -> ssize_t {
    if msgp.is_null() {
        return fail(EFAULT) as ssize_t;
    }
    let words = [
        msqid as u32 as u64,
        msgsz as u64,
        msgtyp as u64,
        msgflg as u64,
        0,
    ];
    let mut text = [0u8; TEXT_MAX];
    match ipc::ipc(ipc::op::MSG_RCV, &words, 4, &[], &mut text, ipc::NO_DEADLINE) {
        Ok(reply) => {
            let n = reply.len.min(msgsz);
            unsafe {
                msgp.cast::<c_long>().write(reply.words[0] as c_long);
                ptr::copy_nonoverlapping(text.as_ptr(), msgp.cast::<u8>().add(8), n);
            }
            n as ssize_t
        }
        Err(errno) => fail(errno) as ssize_t,
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/msgctl.html>.
///
/// # Safety
/// `buf` is a `msqid_ds` for IPC_STAT and IPC_SET.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn msgctl(msqid: c_int, cmd: c_int, buf: *mut msqid_ds) -> c_int {
    match cmd {
        IPC_RMID => {
            let words = [msqid as u32 as u64, 0, 0];
            match ipc::ipc(ipc::op::MSG_CTL, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(_) => 0,
                Err(errno) => fail(errno),
            }
        }
        IPC_STAT => {
            if buf.is_null() {
                return fail(EFAULT);
            }
            let words = [msqid as u32 as u64, IPC_STAT as u64, 0];
            match ipc::ipc(ipc::op::MSG_CTL, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(r) => {
                    let ds = unsafe { &mut *buf };
                    ds.msg_perm.__key = r.words[0] as key_t;
                    ds.msg_perm.uid = r.words[1] as _;
                    ds.msg_perm.gid = r.words[2] as _;
                    ds.msg_perm.cuid = r.words[3] as _;
                    ds.msg_perm.cgid = r.words[4] as _;
                    ds.msg_perm.mode = r.words[5] as _;
                    ds.msg_perm.__seq = r.words[6] as _;
                    ds.msg_stime = r.words[7] as time_t;
                    ds.msg_rtime = r.words[8] as time_t;
                    ds.msg_ctime = r.words[9] as time_t;
                    ds.msg_cbytes = r.words[10] as c_ulong;
                    ds.msg_qnum = r.words[11] as msgqnum_t;
                    ds.msg_qbytes = r.words[12] as msglen_t;
                    ds.msg_lspid = r.words[13] as pid_t;
                    ds.msg_lrpid = r.words[14] as pid_t;
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
            let words = [
                msqid as u32 as u64,
                IPC_SET as u64,
                0,
                u64::from(ds.msg_perm.uid as u32),
                u64::from(ds.msg_perm.gid as u32),
                u64::from(ds.msg_perm.mode as u32),
                ds.msg_qbytes as u64,
            ];
            match ipc::ipc(ipc::op::MSG_CTL, &words, 2, &[], &mut [], ipc::NO_DEADLINE) {
                Ok(_) => 0,
                Err(errno) => fail(errno),
            }
        }
        _ => fail(EINVAL),
    }
}

#[allow(dead_code)]
const _: Option<*const c_char> = None;
