//! `aio.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/aio.h.html>.
//!
//! A request joins the queue of its descriptor and a thread of the library
//! carries the queue's requests out in order, one at a time; the thread ends
//! with the queue. A request that has not begun can be cancelled. The end of
//! a request is published in the control block (`aio_error`, `aio_return`),
//! wakes `aio_suspend` and `lio_listio` and then notifies as the request's
//! `sigevent` asks. The queues are not inherited by a child of `fork`.

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::{
    mem::MaybeUninit,
    ptr,
    sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering},
};

use crate::{
    error::Errno,
    header::{
        bits_sigset_t::sigset_t,
        errno::{EAGAIN, EBADF, ECANCELED, EINPROGRESS, EINTR, EINVAL, EIO, ESPIPE, ETIMEDOUT},
        fcntl::{F_GETFL, O_APPEND, O_DSYNC, O_SYNC, fcntl},
        pthread::{pthread_create, pthread_detach},
        signal::{
            SIG_BLOCK, SIGEV_SIGNAL, SIGEV_THREAD, pthread_sigmask, sigevent, sigfillset, sigqueue,
            sigval,
        },
        time::{CLOCK_MONOTONIC, timespec},
        unistd::{fdatasync, fsync, getpid, pread, pwrite, read, write},
    },
    out::Out,
    platform::{
        self, Pal, Sys,
        types::{c_int, c_void, off_t, pid_t, pthread_t, size_t, ssize_t},
    },
    sync::{Mutex, futex_wake},
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

// The state word of a control block (`__error`): zero for a block that no
// request was made with, then the request in progress, the request that
// ended (3 and the error number), and its end taken by `aio_return`.
const NEW: c_int = 0;
const RUNNING: c_int = 1;
const TAKEN: c_int = 2;
const ENDED: c_int = 3;

/// The requests that wait or run at most ({AIO_MAX}).
pub const AIO_MAX: c_int = 128;

/// What a queued request does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Read,
    Write,
    /// `aio_fsync` with O_SYNC.
    Sync,
    /// `aio_fsync` with O_DSYNC.
    DataSync,
}

/// The end of a `lio_listio` list that was started with LIO_NOWAIT and a
/// `sigevent`: the last request to end notifies. `left` counts the requests
/// that have not ended, and one for the call itself while it is queueing.
struct Group {
    left: AtomicUsize,
    event: sigevent,
}

#[derive(Clone, Copy)]
struct Job {
    cb: *mut aiocb,
    kind: Kind,
    group: *mut Group,
}

/// The requests of one descriptor, in order, and the thread that carries
/// them out.
struct Lane {
    fd: c_int,
    jobs: VecDeque<Job>,
    /// A request of the lane has begun and has not ended.
    running: bool,
}

struct State {
    lanes: Vec<Lane>,
    /// The process the lanes belong to: a child of `fork` has none of the
    /// threads.
    owner: pid_t,
}

// The pointers are the callers' control blocks, which they keep until the
// request ends.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State {
    lanes: Vec::new(),
    owner: 0,
});

/// Whether the request has ended (a block that was never used has not).
fn ended(cb: *const aiocb) -> bool {
    state_of(cb).load(Ordering::Acquire) >= ENDED
}

/// Counts the ends of requests: a waiter sleeps while it is unchanged.
static EPOCH: AtomicU32 = AtomicU32::new(0);

fn lock() -> crate::sync::MutexGuard<'static, State> {
    let mut state = STATE.lock();
    let me = getpid();
    if state.owner != me {
        state.lanes.clear();
        state.owner = me;
    }
    state
}

fn state_of(cb: *const aiocb) -> &'static AtomicI32 {
    unsafe { AtomicI32::from_ptr(ptr::addr_of!((*cb).__error).cast_mut()) }
}

/// Tells of the end as a sigevent asks: a signal to the process, or the
/// function of SIGEV_THREAD run in a thread of its own.
pub(crate) unsafe fn notify_event(event: &sigevent) {
    if event.sigev_notify == SIGEV_SIGNAL && event.sigev_signo != 0 {
        sigqueue(getpid(), event.sigev_signo, event.sigev_value);
    }
    #[cfg(stafeto)]
    if event.sigev_notify == SIGEV_THREAD
        && let Some(function) = event.sigev_notify_function
    {
        extern "C" fn run(arg: *mut c_void) -> *mut c_void {
            let call = unsafe { Box::from_raw(arg.cast::<(extern "C" fn(sigval), sigval)>()) };
            (call.0)(call.1);
            ptr::null_mut()
        }
        let call = Box::into_raw(Box::new((function, event.sigev_value)));
        let mut thread = MaybeUninit::<pthread_t>::uninit();
        let made = unsafe {
            pthread_create(
                thread.as_mut_ptr(),
                event.sigev_notify_attributes.cast_const().cast(),
                run,
                call.cast(),
            )
        };
        if made == 0 {
            unsafe { pthread_detach(thread.assume_init()) };
        } else {
            drop(unsafe { Box::from_raw(call) });
        }
    }
}

/// Publishes the end of a request: the value, then the error, which tells a
/// reader that the value is there; then the waiters are woken.
unsafe fn publish(cb: *mut aiocb, error: c_int, value: ssize_t) {
    unsafe { ptr::addr_of_mut!((*cb).__return).write_volatile(value) };
    state_of(cb).store(ENDED + error, Ordering::Release);
    EPOCH.fetch_add(1, Ordering::SeqCst);
    futex_wake(&EPOCH, i32::MAX);
}

/// The request `job` has ended with `error` and `value`: published, then
/// told as its `sigevent` and the end of its list ask. The control block is
/// not touched after it is published (the caller may free it), so the
/// notification is copied first.
unsafe fn finish(job: Job, error: c_int, value: ssize_t) {
    let event = unsafe { (*job.cb).aio_sigevent.clone() };
    unsafe { publish(job.cb, error, value) };
    unsafe { notify_event(&event) };
    unsafe { group_done(job.group) };
}

unsafe fn group_done(group: *mut Group) {
    if group.is_null() {
        return;
    }
    if unsafe { (*group).left.fetch_sub(1, Ordering::AcqRel) } == 1 {
        let group = unsafe { Box::from_raw(group) };
        unsafe { notify_event(&group.event) };
    }
}

fn block_signals() {
    let mut set = MaybeUninit::<sigset_t>::uninit();
    unsafe {
        sigfillset(set.as_mut_ptr());
        pthread_sigmask(SIG_BLOCK, set.as_ptr(), ptr::null_mut());
    }
}

/// Carries a read or a write out; the position is the request's, and a file
/// that has none (a pipe, a terminal) is read or written where it is.
unsafe fn transfer(cb: *mut aiocb, kind: Kind) -> (c_int, ssize_t) {
    let request = unsafe { &*cb };
    platform::ERRNO.set(0);
    // A file opened for appending gets the data at its end, wherever the
    // request's offset points.
    let append =
        kind == Kind::Write && unsafe { fcntl(request.aio_fildes, F_GETFL, 0) } & O_APPEND != 0;
    let mut done = unsafe {
        if append {
            write(request.aio_fildes, request.aio_buf, request.aio_nbytes)
        } else if kind == Kind::Read {
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
    if done < 0 && platform::ERRNO.get() == ESPIPE {
        done = unsafe {
            if kind == Kind::Read {
                read(request.aio_fildes, request.aio_buf, request.aio_nbytes)
            } else {
                write(request.aio_fildes, request.aio_buf, request.aio_nbytes)
            }
        };
    }
    if done < 0 {
        (platform::ERRNO.get(), -1)
    } else {
        (0, done)
    }
}

unsafe fn carry_out(job: Job) {
    let (error, value) = match job.kind {
        Kind::Read | Kind::Write => unsafe { transfer(job.cb, job.kind) },
        Kind::Sync | Kind::DataSync => {
            let fd = unsafe { (*job.cb).aio_fildes };
            let flushed = if job.kind == Kind::Sync {
                fsync(fd)
            } else {
                fdatasync(fd)
            };
            if flushed < 0 {
                (platform::ERRNO.get(), -1)
            } else {
                (0, 0)
            }
        }
    };
    unsafe { finish(job, error, value) };
}

/// The thread of a lane: its requests in order, and the end of the lane
/// when none is left.
extern "C" fn worker(arg: *mut c_void) -> *mut c_void {
    block_signals();
    let fd = arg as usize as c_int;
    loop {
        let job = {
            let mut state = lock();
            let Some(at) = state.lanes.iter().position(|l| l.fd == fd) else {
                return ptr::null_mut();
            };
            match state.lanes[at].jobs.pop_front() {
                Some(job) => {
                    state.lanes[at].running = true;
                    job
                }
                None => {
                    state.lanes.swap_remove(at);
                    return ptr::null_mut();
                }
            }
        };
        unsafe { carry_out(job) };
        let mut state = lock();
        if let Some(lane) = state.lanes.iter_mut().find(|l| l.fd == fd) {
            lane.running = false;
        }
    }
}

/// Queues a request whose descriptor is checked; the error number if not.
unsafe fn submit(cb: *mut aiocb, kind: Kind, group: *mut Group) -> Result<(), c_int> {
    let request = unsafe { &mut *cb };
    // A descriptor that is no good (or not open for the operation) is the
    // error of the request, which the transfer finds, and not of this call.
    if kind == Kind::Read || kind == Kind::Write {
        if request.aio_offset < 0 || request.aio_reqprio < 0 {
            return Err(EINVAL);
        }
        if request.aio_nbytes > isize::MAX as size_t {
            return Err(EINVAL);
        }
    }
    request.__return = -1;
    state_of(cb).store(RUNNING, Ordering::Release);
    let fd = request.aio_fildes;
    let mut state = lock();
    let outstanding: usize = state
        .lanes
        .iter()
        .map(|l| l.jobs.len() + usize::from(l.running))
        .sum();
    if outstanding >= AIO_MAX as usize {
        state_of(cb).store(NEW, Ordering::Release);
        return Err(EAGAIN);
    }
    let job = Job { cb, kind, group };
    if let Some(lane) = state.lanes.iter_mut().find(|l| l.fd == fd) {
        lane.jobs.push_back(job);
        return Ok(());
    }
    let mut jobs = VecDeque::new();
    jobs.push_back(job);
    state.lanes.push(Lane {
        fd,
        jobs,
        running: false,
    });
    let mut thread = MaybeUninit::<pthread_t>::uninit();
    let made = unsafe {
        pthread_create(
            thread.as_mut_ptr(),
            ptr::null(),
            worker,
            fd as usize as *mut c_void,
        )
    };
    if made != 0 {
        state.lanes.pop();
        state_of(cb).store(NEW, Ordering::Release);
        return Err(EAGAIN);
    }
    unsafe { pthread_detach(thread.assume_init()) };
    Ok(())
}

fn fail(errno: c_int) -> c_int {
    platform::ERRNO.set(errno);
    -1
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_read.html>.
///
/// # Safety
/// `aiocbp` points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_read(aiocbp: *mut aiocb) -> c_int {
    match unsafe { submit(aiocbp, Kind::Read, ptr::null_mut()) } {
        Ok(()) => 0,
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_write.html>.
///
/// # Safety
/// `aiocbp` points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_write(aiocbp: *mut aiocb) -> c_int {
    match unsafe { submit(aiocbp, Kind::Write, ptr::null_mut()) } {
        Ok(()) => 0,
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_fsync.html>.
///
/// # Safety
/// `aiocbp` points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_fsync(operation: c_int, aiocbp: *mut aiocb) -> c_int {
    let kind = match operation {
        o if o == O_SYNC => Kind::Sync,
        o if o == O_DSYNC => Kind::DataSync,
        _ => return fail(EINVAL),
    };
    if unsafe { fcntl((*aiocbp).aio_fildes, F_GETFL, 0) } < 0 {
        return fail(EBADF);
    }
    match unsafe { submit(aiocbp, kind, ptr::null_mut()) } {
        Ok(()) => 0,
        Err(errno) => fail(errno),
    }
}

/// Sleeps until the count of ends is no longer `seen`, the `deadline`
/// (CLOCK_MONOTONIC) passes or a signal comes: 0, ETIMEDOUT or EINTR.
fn sleep_until_change(seen: u32, deadline: Option<&timespec>) -> c_int {
    match unsafe { Sys::futex_wait(ptr::from_ref(&EPOCH).cast_mut().cast(), seen, deadline) } {
        Err(Errno(EINTR)) => EINTR,
        Err(Errno(ETIMEDOUT)) if deadline.is_some() => ETIMEDOUT,
        _ => 0,
    }
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
        return fail(EINVAL);
    }
    if nent > AIO_MAX {
        return fail(EINVAL);
    }
    let group = if mode == LIO_NOWAIT && !sig.is_null() {
        Box::into_raw(Box::new(Group {
            left: AtomicUsize::new(1),
            event: unsafe { (*sig).clone() },
        }))
    } else {
        ptr::null_mut()
    };
    let mut failed = false;
    let mut again = false;
    let mut queued: Vec<*mut aiocb> = Vec::new();
    for i in 0..nent as usize {
        let request = unsafe { *list.add(i) };
        if request.is_null() {
            continue;
        }
        let kind = match unsafe { (*request).aio_lio_opcode } {
            LIO_NOP => continue,
            LIO_READ => Kind::Read,
            LIO_WRITE => Kind::Write,
            _ => {
                // The block tells of the error; the list goes on.
                unsafe {
                    (*request).__return = -1;
                    state_of(request).store(ENDED + EINVAL, Ordering::Release);
                }
                failed = true;
                continue;
            }
        };
        if !group.is_null() {
            unsafe { (*group).left.fetch_add(1, Ordering::AcqRel) };
        }
        match unsafe { submit(request, kind, group) } {
            Ok(()) => queued.push(request),
            Err(errno) => {
                if !group.is_null() {
                    unsafe { (*group).left.fetch_sub(1, Ordering::AcqRel) };
                }
                unsafe {
                    (*request).__return = -1;
                    state_of(request).store(ENDED + errno, Ordering::Release);
                }
                if errno == EAGAIN {
                    again = true;
                } else {
                    failed = true;
                }
            }
        }
    }
    unsafe { group_done(group) };
    if mode == LIO_WAIT {
        for request in &queued {
            loop {
                let seen = EPOCH.load(Ordering::SeqCst);
                let state = state_of(*request).load(Ordering::Acquire);
                if state != RUNNING {
                    if state > ENDED {
                        failed = true;
                    }
                    break;
                }
                if sleep_until_change(seen, None) == EINTR {
                    return fail(EINTR);
                }
            }
        }
    }
    if again {
        return fail(EAGAIN);
    }
    if failed {
        return fail(EIO);
    }
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_error.html>.
///
/// EINPROGRESS while the request runs, the error number of its end (0 for
/// success), and EINVAL for a block no request was made with or one whose
/// end `aio_return` took.
///
/// # Safety
/// `aiocbp` points to a control block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_error(aiocbp: *const aiocb) -> c_int {
    match state_of(aiocbp).load(Ordering::Acquire) {
        NEW | TAKEN => EINVAL,
        RUNNING => EINPROGRESS,
        state => state - ENDED,
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_return.html>.
///
/// The value of the end, once: a second call, a block that was never used
/// and a request in progress give -1 and EINVAL.
///
/// # Safety
/// `aiocbp` points to a control block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_return(aiocbp: *mut aiocb) -> ssize_t {
    let state = state_of(aiocbp);
    let seen = state.load(Ordering::Acquire);
    if seen < ENDED {
        return fail(EINVAL) as ssize_t;
    }
    let value = unsafe { ptr::addr_of!((*aiocbp).__return).read_volatile() };
    match state.compare_exchange(seen, TAKEN, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => value,
        Err(_) => fail(EINVAL) as ssize_t,
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_cancel.html>.
///
/// # Safety
/// `aiocbp` is null or points to a request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_cancel(fildes: c_int, aiocbp: *mut aiocb) -> c_int {
    if unsafe { fcntl(fildes, F_GETFL, 0) } < 0 {
        return fail(EBADF);
    }
    if !aiocbp.is_null() {
        if unsafe { (*aiocbp).aio_fildes } != fildes {
            return fail(EINVAL);
        }
        if state_of(aiocbp).load(Ordering::Acquire) != RUNNING {
            return AIO_ALLDONE;
        }
    }
    let mut cancelled: Vec<Job> = Vec::new();
    let mut not_cancelled = false;
    {
        let mut state = lock();
        if let Some(lane) = state.lanes.iter_mut().find(|l| l.fd == fildes) {
            if aiocbp.is_null() {
                cancelled.extend(lane.jobs.drain(..));
                not_cancelled = lane.running;
            } else if let Some(at) = lane.jobs.iter().position(|j| j.cb == aiocbp) {
                cancelled.extend(lane.jobs.remove(at));
            } else {
                // It is the one that runs.
                not_cancelled = true;
            }
        }
    }
    let any = !cancelled.is_empty();
    for job in cancelled {
        unsafe { finish(job, ECANCELED, -1) };
    }
    if not_cancelled {
        AIO_NOTCANCELED
    } else if any {
        AIO_CANCELED
    } else {
        AIO_ALLDONE
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/aio_suspend.html>.
///
/// # Safety
/// `list` holds `nent` pointers to requests or null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aio_suspend(
    list: *const *const aiocb,
    nent: c_int,
    timeout: *const timespec,
) -> c_int {
    if nent < 0 {
        return fail(EINVAL);
    }
    let deadline = if timeout.is_null() {
        None
    } else {
        let t = unsafe { &*timeout };
        if t.tv_sec < 0 || !(0..1_000_000_000).contains(&t.tv_nsec) {
            return fail(EINVAL);
        }
        let mut now = MaybeUninit::<timespec>::uninit();
        let _ = Sys::clock_gettime(CLOCK_MONOTONIC, Out::from_uninit_mut(&mut now));
        let now = unsafe { now.assume_init() };
        let mut sec = now.tv_sec.saturating_add(t.tv_sec);
        let mut nsec = now.tv_nsec + t.tv_nsec;
        if nsec >= 1_000_000_000 {
            nsec -= 1_000_000_000;
            sec = sec.saturating_add(1);
        }
        Some(timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        })
    };
    loop {
        let seen = EPOCH.load(Ordering::SeqCst);
        for i in 0..nent as usize {
            let request = unsafe { *list.add(i) };
            if !request.is_null() && ended(request) {
                return 0;
            }
        }
        match sleep_until_change(seen, deadline.as_ref()) {
            0 => {}
            ETIMEDOUT => return fail(EAGAIN),
            errno => return fail(errno),
        }
    }
}
