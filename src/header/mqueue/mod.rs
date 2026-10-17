//! `mqueue.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/mqueue.h.html>.
//!
//! A message queue is named `/name` and opened for a descriptor. The queues
//! live in the pipe service of stafeto, at most 64 messages of 512 bytes
//! each, 64 messages in all for the queues and XSI queues together.

use alloc::boxed::Box;
use core::{ptr, sync::atomic::{AtomicU32, Ordering}};

use crate::{
    c_str::CStr,
    header::{
        errno::{EBADF, EINVAL, EMSGSIZE, ENOSYS},
        fcntl::O_NONBLOCK,
        pthread::{pthread_create, pthread_detach},
        signal::{SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, sigevent, sigqueue, sigval},
        time::{CLOCK_REALTIME, nanosleep, timespec},
        unistd::{close, getpid},
    },
    platform::{
        self, ipc,
        types::{c_char, c_int, c_long, c_uint, mode_t, pthread_t, size_t, ssize_t},
    },
};

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/mqueue.h.html>.
#[allow(non_camel_case_types)]
pub type mqd_t = c_int;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/mqueue.h.html>.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mq_attr {
    /// Message queue flags: O_NONBLOCK or 0.
    pub mq_flags: c_long,
    /// Maximum number of messages.
    pub mq_maxmsg: c_long,
    /// Maximum message size.
    pub mq_msgsize: c_long,
    /// Number of messages currently queued.
    pub mq_curmsgs: c_long,
    pub __reserved: [c_long; 4],
}

/// The most priorities: priorities are below this.
const MQ_PRIO_MAX: c_uint = 32768;

fn fail(errno: c_int) -> c_int {
    platform::ERRNO.set(errno);
    -1
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_open.html>.
///
/// # Safety
/// `name` is a C string; with O_CREAT the two arguments after `oflag` are the
/// mode and a pointer to a `mq_attr` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_open(name: *const c_char, oflag: c_int, mut __valist: ...) -> mqd_t {
    if name.is_null() {
        return fail(EINVAL);
    }
    let (mode, attr) = if oflag & crate::header::fcntl::O_CREAT != 0 {
        let mode = unsafe { __valist.next_arg::<mode_t>() };
        let attr = unsafe { __valist.next_arg::<*const mq_attr>() };
        (mode, attr)
    } else {
        (0, ptr::null())
    };
    let attr = if attr.is_null() {
        None
    } else {
        let a = unsafe { &*attr };
        if a.mq_maxmsg <= 0 || a.mq_msgsize <= 0 {
            return fail(EINVAL);
        }
        Some((a.mq_maxmsg, a.mq_msgsize))
    };
    match ipc::mq_open(name, oflag, mode, attr) {
        Ok(fd) => fd,
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_close.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_close(mqdes: mqd_t) -> c_int {
    // Only a descriptor of a queue is closed here.
    let mut out = [0u8; 0];
    if let Err(errno) = ipc::ipc_fd(mqdes, ipc::op::MQ_GETATTR, &[0], ipc::NO_CRED, &[], &mut out, ipc::NO_DEADLINE) {
        return fail(errno);
    }
    // The close removes the registration for notice that was made with it
    // (the service takes off only the registration of this descriptor), also
    // when a child of `fork` still holds a copy of the descriptor.
    let _ = ipc::ipc_fd(mqdes, ipc::op::MQ_NOTIFY, &[0, 0], ipc::NO_CRED, &[], &mut out, ipc::NO_DEADLINE);
    close(mqdes)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_unlink.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_unlink(name: *const c_char) -> c_int {
    if name.is_null() {
        return fail(EINVAL);
    }
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    let words = [0u64; 1];
    let mut out = [0u8; 0];
    match ipc::ipc(ipc::op::MQ_UNLINK, &words, 0, bytes, &mut out, ipc::NO_DEADLINE) {
        Ok(_) => 0,
        Err(errno) => fail(errno),
    }
}

unsafe fn send(
    mqdes: mqd_t,
    msg_ptr: *const c_char,
    msg_len: size_t,
    msg_prio: c_uint,
    deadline: u64,
) -> c_int {
    if msg_prio >= MQ_PRIO_MAX {
        return fail(EINVAL);
    }
    if msg_ptr.is_null() && msg_len != 0 {
        return fail(EINVAL);
    }
    let bytes = if msg_len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(msg_ptr.cast::<u8>(), msg_len) }
    };
    if bytes.len() > 512 {
        return fail(EMSGSIZE);
    }
    let words = [0, u64::from(msg_prio), 0, 0];
    let mut out = [0u8; 0];
    match ipc::ipc_fd(mqdes, ipc::op::MQ_SEND, &words, 3, bytes, &mut out, deadline) {
        Ok(reply) => {
            // This message fired a registration for notice. When it is of
            // this process, its watcher is about to give the notice: the
            // send waits for it (a moment at most), so that a signal of the
            // registration has come when the call returns.
            if reply.words[0] == 1 && WATCHES.load(Ordering::Relaxed) != 0 {
                let before = DELIVERED.load(Ordering::Acquire);
                for _ in 0..100 {
                    if DELIVERED.load(Ordering::Acquire) != before {
                        break;
                    }
                    let nap = timespec { tv_sec: 0, tv_nsec: 200_000 };
                    unsafe { nanosleep(&nap, ptr::null_mut()) };
                }
                // One more call, which the signal waits for.
                let nap = timespec { tv_sec: 0, tv_nsec: 1_000 };
                unsafe { nanosleep(&nap, ptr::null_mut()) };
            }
            0
        }
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_send.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_send(
    mqdes: mqd_t,
    msg_ptr: *const c_char,
    msg_len: size_t,
    msg_prio: c_uint,
) -> c_int {
    unsafe { send(mqdes, msg_ptr, msg_len, msg_prio, ipc::NO_DEADLINE) }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_timedsend.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_timedsend(
    mqdes: mqd_t,
    msg_ptr: *const c_char,
    msg_len: size_t,
    msg_prio: c_uint,
    abstime: *const timespec,
) -> c_int {
    if abstime.is_null() {
        return fail(EINVAL);
    }
    let deadline = match ipc::deadline(unsafe { &*abstime }, CLOCK_REALTIME) {
        Ok(deadline) => deadline,
        Err(errno) => return fail(errno),
    };
    unsafe { send(mqdes, msg_ptr, msg_len, msg_prio, deadline) }
}

unsafe fn receive(
    mqdes: mqd_t,
    msg_ptr: *mut c_char,
    msg_len: size_t,
    msg_prio: *mut c_uint,
    deadline: u64,
) -> ssize_t {
    if msg_ptr.is_null() && msg_len != 0 {
        return fail(EINVAL) as ssize_t;
    }
    let words = [0, msg_len as u64, 0];
    let mut buffer = [0u8; 512];
    match ipc::ipc_fd(mqdes, ipc::op::MQ_RECV, &words, 2, &[], &mut buffer, deadline) {
        Ok(reply) => {
            let n = reply.len.min(msg_len);
            unsafe { ptr::copy_nonoverlapping(buffer.as_ptr(), msg_ptr.cast::<u8>(), n) };
            if !msg_prio.is_null() {
                unsafe { msg_prio.write(reply.words[0] as c_uint) };
            }
            n as ssize_t
        }
        Err(errno) => fail(errno) as ssize_t,
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_receive.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_receive(
    mqdes: mqd_t,
    msg_ptr: *mut c_char,
    msg_len: size_t,
    msg_prio: *mut c_uint,
) -> ssize_t {
    unsafe { receive(mqdes, msg_ptr, msg_len, msg_prio, ipc::NO_DEADLINE) }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_timedreceive.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_timedreceive(
    mqdes: mqd_t,
    msg_ptr: *mut c_char,
    msg_len: size_t,
    msg_prio: *mut c_uint,
    abstime: *const timespec,
) -> ssize_t {
    if abstime.is_null() {
        return fail(EINVAL) as ssize_t;
    }
    let deadline = match ipc::deadline(unsafe { &*abstime }, CLOCK_REALTIME) {
        Ok(deadline) => deadline,
        Err(errno) => return fail(errno) as ssize_t,
    };
    unsafe { receive(mqdes, msg_ptr, msg_len, msg_prio, deadline) }
}

fn fill(attr: &mut mq_attr, words: &[u64]) {
    attr.mq_flags = words[0] as c_long;
    attr.mq_maxmsg = words[1] as c_long;
    attr.mq_msgsize = words[2] as c_long;
    attr.mq_curmsgs = words[3] as c_long;
    attr.__reserved = [0; 4];
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_getattr.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_getattr(mqdes: mqd_t, mqstat: *mut mq_attr) -> c_int {
    if mqstat.is_null() {
        return fail(EINVAL);
    }
    let mut out = [0u8; 0];
    match ipc::ipc_fd(mqdes, ipc::op::MQ_GETATTR, &[0], ipc::NO_CRED, &[], &mut out, ipc::NO_DEADLINE) {
        Ok(reply) => {
            fill(unsafe { &mut *mqstat }, &reply.words);
            0
        }
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_setattr.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_setattr(
    mqdes: mqd_t,
    mqstat: *const mq_attr,
    omqstat: *mut mq_attr,
) -> c_int {
    if mqstat.is_null() {
        return fail(EINVAL);
    }
    let flags = unsafe { (*mqstat).mq_flags } as u64 & O_NONBLOCK as u64;
    let mut out = [0u8; 0];
    match ipc::ipc_fd(mqdes, ipc::op::MQ_SETATTR, &[0, flags], ipc::NO_CRED, &[], &mut out, ipc::NO_DEADLINE) {
        Ok(reply) => {
            if !omqstat.is_null() {
                fill(unsafe { &mut *omqstat }, &reply.words);
            }
            0
        }
        Err(errno) => fail(errno),
    }
}

/// What the thread that waits for a queue's notification keeps.
struct Watch {
    mqdes: mqd_t,
    notify: c_int,
    signo: c_int,
    value: sigval,
    function: Option<extern "C" fn(sigval)>,
    process: c_int,
}

/// The notices the watchers of this process have given.
static DELIVERED: AtomicU32 = AtomicU32::new(0);

/// Registrations that are alive in this process: a child of `fork` has no
/// thread to wait for them.
static WATCHES: AtomicU32 = AtomicU32::new(0);

extern "C" fn watcher(arg: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
    let watch = unsafe { Box::from_raw(arg.cast::<Watch>()) };
    let mut out = [0u8; 0];
    let answer = ipc::ipc_fd(
        watch.mqdes,
        ipc::op::MQ_NOTIFY_WAIT,
        &[0],
        ipc::NO_CRED,
        &[],
        &mut out,
        ipc::NO_DEADLINE,
    );
    WATCHES.fetch_sub(1, Ordering::Relaxed);
    // 1: a message came to the empty queue; the wait of another end (the
    // registration was removed, the queue closed) sends nothing.
    if matches!(answer, Ok(reply) if reply.words[0] == 1) {
        if watch.notify == SIGEV_SIGNAL && watch.signo != 0 {
            sigqueue(watch.process, watch.signo, watch.value);
        } else if watch.notify == SIGEV_THREAD
            && let Some(function) = watch.function
        {
            function(watch.value);
        }
    }
    DELIVERED.fetch_add(1, Ordering::Release);
    ptr::null_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mq_notify.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_notify(mqdes: mqd_t, notification: *const sigevent) -> c_int {
    let mut out = [0u8; 0];
    if notification.is_null() {
        return match ipc::ipc_fd(mqdes, ipc::op::MQ_NOTIFY, &[0, 0], ipc::NO_CRED, &[], &mut out, ipc::NO_DEADLINE) {
            Ok(_) => 0,
            Err(errno) => fail(errno),
        };
    }
    let event = unsafe { &*notification };
    if !matches!(event.sigev_notify, SIGEV_NONE | SIGEV_SIGNAL | SIGEV_THREAD) {
        return fail(EINVAL);
    }
    // Signal 0 is taken as Linux takes it: the registration is made and no
    // signal is sent.
    if event.sigev_notify == SIGEV_SIGNAL && !(0..=31).contains(&event.sigev_signo) {
        return fail(EINVAL);
    }
    if let Err(errno) = ipc::ipc_fd(mqdes, ipc::op::MQ_NOTIFY, &[0, 1], ipc::NO_CRED, &[], &mut out, ipc::NO_DEADLINE) {
        return fail(errno);
    }
    let watch = Box::into_raw(Box::new(Watch {
        mqdes,
        notify: event.sigev_notify,
        signo: event.sigev_signo,
        value: event.sigev_value,
        function: event.sigev_notify_function,
        process: getpid(),
    }));
    WATCHES.fetch_add(1, Ordering::Relaxed);
    let mut thread = core::mem::MaybeUninit::<pthread_t>::uninit();
    let made = unsafe {
        pthread_create(
            thread.as_mut_ptr(),
            ptr::null(),
            watcher,
            watch.cast(),
        )
    };
    if made != 0 {
        drop(unsafe { Box::from_raw(watch) });
        WATCHES.fetch_sub(1, Ordering::Relaxed);
        let _ = ipc::ipc_fd(mqdes, ipc::op::MQ_NOTIFY, &[0, 0], ipc::NO_CRED, &[], &mut out, ipc::NO_DEADLINE);
        return fail(made);
    }
    unsafe { pthread_detach(thread.assume_init()) };
    0
}

#[allow(dead_code)]
const _: (c_int, c_int) = (EBADF, ENOSYS);
