// SPDX-License-Identifier: MIT
//! Message queues, semaphores and shared memory segments through the layer of
//! stafeto: one call of the platform (`stafeto_ipc`) carries the words of the
//! protocol of the pipe service and the bytes of a message, and returns the
//! words and bytes of the answer. Elsewhere the calls answer ENOSYS.

use crate::{
    header::{
        errno::EINVAL,
        time::{CLOCK_MONOTONIC, clock_gettime, timespec},
    },
    platform::types::{c_int, clockid_t},
};

/// The words of an answer at most.
pub const WORDS: usize = 32;
/// No credentials in the words of a request.
pub const NO_CRED: usize = usize::MAX;
/// No deadline.
pub const NO_DEADLINE: u64 = u64::MAX;

/// The operations of the protocol (proto_pipe::ipc::op).
pub mod op {
    pub const MQ_OPEN: u32 = 1;
    pub const MQ_UNLINK: u32 = 2;
    pub const MQ_SEND: u32 = 3;
    pub const MQ_RECV: u32 = 4;
    pub const MQ_GETATTR: u32 = 5;
    pub const MQ_SETATTR: u32 = 6;
    pub const MQ_NOTIFY: u32 = 7;
    pub const MQ_NOTIFY_WAIT: u32 = 8;
    pub const MSG_GET: u32 = 16;
    pub const MSG_SND: u32 = 17;
    pub const MSG_RCV: u32 = 18;
    pub const MSG_CTL: u32 = 19;
    pub const SEM_GET: u32 = 20;
    pub const SEM_OP: u32 = 21;
    pub const SEM_CTL: u32 = 22;
    pub const SHM_GET: u32 = 24;
    pub const SHM_AT: u32 = 25;
    pub const SHM_DT: u32 = 26;
    pub const SHM_CTL: u32 = 27;
    pub const NSEM_OPEN: u32 = 32;
    pub const NSEM_UNLINK: u32 = 33;
    pub const NSEM_CLOSE: u32 = 34;
    pub const NSEM_WAIT: u32 = 35;
    pub const NSEM_POST: u32 = 36;
    pub const NSEM_VALUE: u32 = 37;
}

/// What a call gave: the words of the answer and the count of bytes copied.
#[derive(Clone, Copy)]
pub struct Reply {
    pub words: [u64; WORDS],
    pub len: usize,
}

/// The monotonic time in ns at which the absolute time `abs` of `clock` comes
/// (now, when it is past); EINVAL for a `tv_nsec` out of its range.
pub fn deadline(abs: &timespec, clock: clockid_t) -> Result<u64, c_int> {
    if abs.tv_nsec < 0 || abs.tv_nsec >= 1_000_000_000 {
        return Err(EINVAL);
    }
    let mut now = timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let mut mono = timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { clock_gettime(clock, &raw mut now) } < 0
        || unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut mono) } < 0
    {
        return Err(EINVAL);
    }
    let ns = |t: &timespec| i128::from(t.tv_sec) * 1_000_000_000 + i128::from(t.tv_nsec);
    let wait = (ns(abs) - ns(&now)).max(0);
    let at = (ns(&mono) + wait).clamp(0, i128::from(u64::MAX - 1));
    Ok(at as u64)
}

#[cfg(stafeto)]
mod layer {
    use super::*;
    use crate::platform::types::c_char;

    unsafe extern "C" {
        fn stafeto_ipc(
            operation: u32,
            words: *const u64,
            nwords: usize,
            cred_at: usize,
            bytes: *const u8,
            nbytes: usize,
            words_out: *mut u64,
            nwords_out: usize,
            bytes_out: *mut u8,
            cap_out: usize,
            deadline: u64,
        ) -> isize;
        fn stafeto_ipc_fd(
            fd: c_int,
            operation: u32,
            words: *const u64,
            nwords: usize,
            cred_at: usize,
            bytes: *const u8,
            nbytes: usize,
            words_out: *mut u64,
            nwords_out: usize,
            bytes_out: *mut u8,
            cap_out: usize,
            deadline: u64,
        ) -> isize;
        fn stafeto_mq_open(
            name: *const c_char,
            oflag: c_int,
            mode: u32,
            has_attr: c_int,
            maxmsg: i64,
            msgsize: i64,
        ) -> c_int;
    }

    fn result(n: isize, words: [u64; WORDS]) -> Result<Reply, c_int> {
        if n < 0 {
            Err((-n) as c_int)
        } else {
            Ok(Reply {
                words,
                len: n as usize,
            })
        }
    }

    /// An operation with `words`, the credentials at `cred_at`, `bytes`; its
    /// bytes go into `out`.
    pub fn ipc(
        operation: u32,
        words: &[u64],
        cred_at: usize,
        bytes: &[u8],
        out: &mut [u8],
        deadline: u64,
    ) -> Result<Reply, c_int> {
        let mut answer = [0u64; WORDS];
        let n = unsafe {
            stafeto_ipc(
                operation,
                words.as_ptr(),
                words.len(),
                cred_at,
                bytes.as_ptr(),
                bytes.len(),
                answer.as_mut_ptr(),
                WORDS,
                out.as_mut_ptr(),
                out.len(),
                deadline,
            )
        };
        result(n, answer)
    }

    /// The same on the message queue of the descriptor `fd`; the first word
    /// is replaced by its description.
    pub fn ipc_fd(
        fd: c_int,
        operation: u32,
        words: &[u64],
        cred_at: usize,
        bytes: &[u8],
        out: &mut [u8],
        deadline: u64,
    ) -> Result<Reply, c_int> {
        let mut answer = [0u64; WORDS];
        let n = unsafe {
            stafeto_ipc_fd(
                fd,
                operation,
                words.as_ptr(),
                words.len(),
                cred_at,
                bytes.as_ptr(),
                bytes.len(),
                answer.as_mut_ptr(),
                WORDS,
                out.as_mut_ptr(),
                out.len(),
                deadline,
            )
        };
        result(n, answer)
    }

    /// A descriptor for the queue `name` (a C string with its `/`).
    pub fn mq_open(
        name: *const c_char,
        oflag: c_int,
        mode: u32,
        attr: Option<(i64, i64)>,
    ) -> Result<c_int, c_int> {
        let (maxmsg, msgsize) = attr.unwrap_or((0, 0));
        let n = unsafe {
            stafeto_mq_open(
                name,
                oflag,
                mode,
                c_int::from(attr.is_some()),
                maxmsg,
                msgsize,
            )
        };
        if n < 0 { Err(-n) } else { Ok(n) }
    }
}

#[cfg(not(stafeto))]
mod layer {
    use super::*;
    use crate::{header::errno::ENOSYS, platform::types::c_char};

    pub fn ipc(
        _: u32,
        _: &[u64],
        _: usize,
        _: &[u8],
        _: &mut [u8],
        _: u64,
    ) -> Result<Reply, c_int> {
        Err(ENOSYS)
    }

    pub fn ipc_fd(
        _: c_int,
        _: u32,
        _: &[u64],
        _: usize,
        _: &[u8],
        _: &mut [u8],
        _: u64,
    ) -> Result<Reply, c_int> {
        Err(ENOSYS)
    }

    pub fn mq_open(_: *const c_char, _: c_int, _: u32, _: Option<(i64, i64)>) -> Result<c_int, c_int> {
        Err(ENOSYS)
    }
}

pub use layer::{ipc, ipc_fd, mq_open};
