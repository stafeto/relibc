//! `semaphore.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/semaphore.h.html>.

use core::sync::atomic::{AtomicPtr, Ordering};

use crate::{
    c_str::CStr,
    error::ResultExt,
    header::{
        errno,
        fcntl::O_CREAT,
        time::{self, timespec},
    },
    platform::{
        self, ipc,
        types::{c_char, c_int, c_long, c_uint, clockid_t, mode_t},
    },
    sync::Mutex,
};

/// {SEM_VALUE_MAX}: the value of a semaphore at most.
pub const SEM_VALUE_MAX: c_int = 32767;
/// The value `sem_open` returns for a failure (SEM_FAILED of the header is 0).
const SEM_FAILED: *mut sem_t = core::ptr::null_mut();

/// The named semaphores this process has open: the objects `sem_open` gives
/// are in this table, and the calls on them go to the pipe service, which
/// keeps the value. One entry for each semaphore, whatever the count of opens.
const NAMED_MAX: usize = 16;

/// What is known of an open named semaphore: its id in the service and how
/// many times this process opened it.
#[derive(Clone, Copy)]
struct Meta {
    used: bool,
    id: u64,
    opens: u32,
}

/// The `sem_t` the process holds for each: a place with a fixed address,
/// which names the entry of META. No lock is taken to tell whether a `sem_t`
/// is one of these, so that the calls on other semaphores stay what they were.
struct Cells([core::cell::UnsafeCell<sem_t>; NAMED_MAX]);
// SAFETY: the cells are never read or written here, only their addresses used.
unsafe impl Sync for Cells {}

static CELLS: Cells = Cells([const { core::cell::UnsafeCell::new(sem_t { align: 0 }) }; NAMED_MAX]);
static META: Mutex<[Meta; NAMED_MAX]> = Mutex::new(
    [Meta {
        used: false,
        id: 0,
        opens: 0,
    }; NAMED_MAX],
);

/// The entry a `sem_t` is, by its address alone.
fn cell_of(sem: *mut sem_t) -> Option<usize> {
    let base = CELLS.0.as_ptr() as usize;
    let at = (sem as usize).checked_sub(base)?;
    let stride = core::mem::size_of::<core::cell::UnsafeCell<sem_t>>();
    (at / stride < NAMED_MAX && at % stride == 0).then_some(at / stride)
}

/// What the calls on a `sem_t` need to know of the named semaphores. A
/// program that never opens one does not carry their code: the table is
/// reached from `sem_open` alone, which publishes it.
struct Hooks {
    id: fn(*mut sem_t) -> Option<u64>,
    wait: fn(u64, u64, u64) -> c_int,
    timed: fn(u64, clockid_t, *const timespec) -> c_int,
    post: fn(u64) -> c_int,
    value: fn(u64, *mut c_int) -> c_int,
}

static HOOKS: Hooks = Hooks {
    id: named_id,
    wait: named_wait,
    timed: named_timed,
    post: named_post,
    value: named_value,
};

static PUBLISHED: AtomicPtr<Hooks> = AtomicPtr::new(core::ptr::null_mut());

fn hooks() -> Option<&'static Hooks> {
    // SAFETY: the pointer is null or that of HOOKS.
    unsafe { PUBLISHED.load(Ordering::Acquire).as_ref() }
}

fn named_post(id: u64) -> c_int {
    match ipc::ipc(ipc::op::NSEM_POST, &[id, 0], 1, &[], &mut [], ipc::NO_DEADLINE) {
        Ok(_) => 0,
        Err(e) => {
            platform::ERRNO.set(e);
            -1
        }
    }
}

fn named_value(id: u64, sval: *mut c_int) -> c_int {
    match ipc::ipc(ipc::op::NSEM_VALUE, &[id], ipc::NO_CRED, &[], &mut [], ipc::NO_DEADLINE) {
        Ok(reply) => {
            unsafe { sval.write(reply.words[0] as c_int) };
            0
        }
        Err(e) => {
            platform::ERRNO.set(e);
            -1
        }
    }
}

/// The id of the named semaphore a `sem_t` is, or None for another.
fn named_id(sem: *mut sem_t) -> Option<u64> {
    let index = cell_of(sem)?;
    let meta = META.lock();
    meta[index].used.then(|| meta[index].id)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/semaphore.h.html>.
// TODO: Statically verify size and align
#[repr(C)]
#[derive(Clone, Copy)]
pub union sem_t {
    pub size: [c_char; 4],
    pub align: c_long,
}
pub type RlctSempahore = crate::sync::Semaphore;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_close.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_close(sem: *mut sem_t) -> c_int {
    let Some(id) = named_id(sem) else {
        platform::ERRNO.set(errno::EINVAL);
        return -1;
    };
    let _ = ipc::ipc(ipc::op::NSEM_CLOSE, &[id], ipc::NO_CRED, &[], &mut [], ipc::NO_DEADLINE);
    let index = cell_of(sem).expect("a named semaphore");
    let mut meta = META.lock();
    meta[index].opens -= 1;
    if meta[index].opens == 0 {
        meta[index].used = false;
    }
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_destroy.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_destroy(sem: *mut sem_t) -> c_int {
    unsafe { core::ptr::drop_in_place(sem.cast::<RlctSempahore>()) };
    let _ = UNNAMED.fetch_update(
        core::sync::atomic::Ordering::SeqCst,
        core::sync::atomic::Ordering::SeqCst,
        |n| n.checked_sub(1),
    );
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_getvalue.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_getvalue(sem: *mut sem_t, sval: *mut c_int) -> c_int {
    if let Some(h) = hooks()
        && let Some(id) = (h.id)(sem)
    {
        return (h.value)(id, sval);
    }
    unsafe { sval.write(get(sem).value() as c_int) };

    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_init.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_init(sem: *mut sem_t, _pshared: c_int, value: c_uint) -> c_int {
    // A value past {SEM_VALUE_MAX} is EINVAL; {SEM_NSEMS_MAX} semaphores at a
    // time (counted from `sem_init` to `sem_destroy`) is ENOSPC beyond.
    if value > SEM_VALUE_MAX as c_uint {
        platform::ERRNO.set(errno::EINVAL);
        return -1;
    }
    if UNNAMED.fetch_add(1, core::sync::atomic::Ordering::SeqCst) >= SEM_NSEMS_MAX {
        UNNAMED.fetch_sub(1, core::sync::atomic::Ordering::SeqCst);
        platform::ERRNO.set(errno::ENOSPC);
        return -1;
    }
    unsafe { sem.cast::<RlctSempahore>().write(RlctSempahore::new(value)) };

    0
}

/// {SEM_NSEMS_MAX}: the semaphores of `sem_init` a process has at a time.
pub const SEM_NSEMS_MAX: usize = 256;
static UNNAMED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_open.html>.
///
/// With O_CREAT the two arguments after `oflag` are the mode and the value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_open(name: *const c_char, oflag: c_int, mut __valist: ...) -> *mut sem_t {
    if name.is_null() {
        platform::ERRNO.set(errno::EINVAL);
        return SEM_FAILED;
    }
    let (mode, value) = if oflag & O_CREAT != 0 {
        let mode = unsafe { __valist.next_arg::<mode_t>() };
        let value = unsafe { __valist.next_arg::<c_uint>() };
        (mode, value)
    } else {
        (0, 0)
    };
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    let words = [oflag as u64, u64::from(mode), u64::from(value), 0];
    let id = match ipc::ipc(ipc::op::NSEM_OPEN, &words, 3, bytes, &mut [], ipc::NO_DEADLINE) {
        Ok(reply) => reply.words[0],
        Err(e) => {
            platform::ERRNO.set(e);
            return SEM_FAILED;
        }
    };
    PUBLISHED.store((&raw const HOOKS).cast_mut(), Ordering::Release);
    let mut meta = META.lock();
    if let Some(index) = meta.iter().position(|n| n.used && n.id == id) {
        meta[index].opens += 1;
        return CELLS.0[index].get();
    }
    match meta.iter().position(|n| !n.used) {
        Some(index) => {
            meta[index] = Meta {
                used: true,
                id,
                opens: 1,
            };
            CELLS.0[index].get()
        }
        None => {
            drop(meta);
            let _ = ipc::ipc(ipc::op::NSEM_CLOSE, &[id], ipc::NO_CRED, &[], &mut [], ipc::NO_DEADLINE);
            platform::ERRNO.set(errno::EMFILE);
            SEM_FAILED
        }
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_post.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_post(sem: *mut sem_t) -> c_int {
    if let Some(h) = hooks()
        && let Some(id) = (h.id)(sem)
    {
        return (h.post)(id);
    }
    unsafe { get(sem) }.post(1);

    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_trywait.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_trywait(sem: *mut sem_t) -> c_int {
    if let Some(h) = hooks()
        && let Some(id) = (h.id)(sem)
    {
        return (h.wait)(id, 1, u64::MAX);
    }
    if unsafe { get(sem) }.try_wait() {
        0
    } else {
        platform::ERRNO.set(errno::EAGAIN);
        -1
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_unlink.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_unlink(name: *const c_char) -> c_int {
    if name.is_null() {
        platform::ERRNO.set(errno::EINVAL);
        return -1;
    }
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    // A semaphore with no name does not exist.
    if bytes.is_empty() {
        platform::ERRNO.set(errno::ENOENT);
        return -1;
    }
    match ipc::ipc(ipc::op::NSEM_UNLINK, &[0; 1], 0, bytes, &mut [], ipc::NO_DEADLINE) {
        Ok(_) => 0,
        Err(e) => {
            platform::ERRNO.set(e);
            -1
        }
    }
}

/// The wait on a named semaphore: `nowait` is 1 for `sem_trywait`.
fn named_wait(id: u64, nowait: u64, deadline: u64) -> c_int {
    loop {
        match ipc::ipc(ipc::op::NSEM_WAIT, &[id, nowait, 0], 2, &[], &mut [], deadline) {
            Ok(_) => return 0,
            // A blocking wait the service has no room to keep (EAGAIN: it
            // keeps a bounded number of waiting operations) is asked again
            // a moment later, until its deadline.
            Err(errno::EAGAIN) if nowait == 0 => {
                let mut now = time::timespec::default();
                unsafe { time::clock_gettime(time::CLOCK_MONOTONIC, &mut now) };
                let at = (now.tv_sec as u64)
                    .saturating_mul(1_000_000_000)
                    .saturating_add(now.tv_nsec as u64);
                if deadline != ipc::NO_DEADLINE && at >= deadline {
                    platform::ERRNO.set(errno::ETIMEDOUT);
                    return -1;
                }
                let pause = time::timespec {
                    tv_sec: 0,
                    tv_nsec: 2_000_000,
                };
                unsafe { time::nanosleep(&pause, core::ptr::null_mut()) };
            }
            Err(e) => {
                platform::ERRNO.set(e);
                return -1;
            }
        }
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_trywait.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_wait(sem: *mut sem_t) -> c_int {
    if let Some(h) = hooks()
        && let Some(id) = (h.id)(sem)
    {
        return (h.wait)(id, 0, u64::MAX);
    }
    unsafe { get(sem) }
        .wait(None, time::CLOCK_MONOTONIC)
        .map(|()| 0)
        .or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_clockwait.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_clockwait(
    sem: *mut sem_t,
    clock_id: clockid_t,
    abstime: *const timespec,
) -> c_int {
    if let Some(h) = hooks()
        && let Some(id) = (h.id)(sem)
    {
        return (h.timed)(id, clock_id, abstime);
    }
    unsafe { get(sem) }
        .wait(Some(&unsafe { (*abstime).clone() }), clock_id)
        .map(|()| 0)
        .or_minus_one_errno()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sem_timedwait.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sem_timedwait(sem: *mut sem_t, abstime: *const timespec) -> c_int {
    if let Some(h) = hooks()
        && let Some(id) = (h.id)(sem)
    {
        return (h.timed)(id, time::CLOCK_REALTIME, abstime);
    }
    unsafe { get(sem) }
        .wait(Some(&unsafe { (*abstime).clone() }), time::CLOCK_REALTIME)
        .map(|()| 0)
        .or_minus_one_errno()
}

unsafe fn get<'any>(sem: *mut sem_t) -> &'any RlctSempahore {
    unsafe { &*sem.cast() }
}

/// A timed wait on a named semaphore: the time is checked before it waits.
fn named_timed(id: u64, clock: clockid_t, abstime: *const timespec) -> c_int {
    if abstime.is_null() {
        platform::ERRNO.set(errno::EINVAL);
        return -1;
    }
    match ipc::deadline(unsafe { &*abstime }, clock) {
        Ok(deadline) => named_wait(id, 0, deadline),
        Err(e) => {
            platform::ERRNO.set(e);
            -1
        }
    }
}
