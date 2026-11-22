use crate::platform::types::{c_int, c_void};
use alloc::vec::Vec;
use core::cell::RefCell;
use spin::Mutex;

#[derive(Clone, Copy)]
struct CxaAtExitFunc {
    func: extern "C" fn(*mut c_void),
    arg: usize,
    dso: usize,
}

#[derive(Clone, Copy)]
struct CxaThreadAtExitFunc {
    func: extern "C" fn(*mut c_void),
    obj: *mut c_void,
    dso: *mut c_void,
}

static CXA_ATEXIT_FUNCS: Mutex<Vec<Option<CxaAtExitFunc>>> = Mutex::new(Vec::new());

/// fork on stafeto, in the child: the lock of the functions of exit is
/// free again. It is a spin lock, which the forking thread may not wait
/// for on one processor: a thread of the parent that held it is no thread
/// of the child.
#[cfg(stafeto)]
pub(crate) fn fork_reset() {
    if CXA_ATEXIT_FUNCS.is_locked() {
        unsafe { CXA_ATEXIT_FUNCS.force_unlock() };
    }
}
#[thread_local]
static DTORS: RefCell<Vec<CxaThreadAtExitFunc>> = RefCell::new(Vec::new());

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __cxa_atexit(
    func: Option<extern "C" fn(*mut c_void)>,
    arg: *mut c_void,
    dso: *mut c_void,
) -> c_int {
    let Some(func) = func else {
        return 0;
    };

    let entry = CxaAtExitFunc {
        func,
        arg: arg as usize,
        dso: dso as usize,
    };

    // The newest entry is the last: a registration made while the functions
    // run (a handler that calls atexit, a static local built in a
    // destructor) runs before the older ones.
    CXA_ATEXIT_FUNCS.lock().push(Some(entry));
    0
}

/// Runs the functions registered by `__cxa_atexit` and `atexit` (a null
/// `dso` takes all of them, otherwise those of that object), the newest
/// first. Each entry is taken from the list before it runs, and the list is
/// not locked during the call: a function may register more, and those run
/// next.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __cxa_finalize(dso: *mut c_void) {
    let dso_usize = dso as usize;
    loop {
        let entry = {
            let mut funcs = CXA_ATEXIT_FUNCS.lock();
            let found = funcs.iter().rposition(|slot| {
                slot.as_ref()
                    .is_some_and(|entry| dso.is_null() || entry.dso == dso_usize)
            });
            let Some(index) = found else {
                if dso.is_null() {
                    funcs.clear();
                } else {
                    funcs.retain(Option::is_some);
                }
                return;
            };
            let entry = funcs[index].take();
            while matches!(funcs.last(), Some(None)) {
                funcs.pop();
            }
            entry
        };
        if let Some(entry) = entry {
            (entry.func)(entry.arg as *mut c_void);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __cxa_thread_atexit_impl(
    func: extern "C" fn(*mut c_void),
    obj: *mut c_void,
    dso: *mut c_void,
) {
    let entry = CxaThreadAtExitFunc { func, obj, dso };
    DTORS.borrow_mut().push(entry);
}

// called internally
pub unsafe fn __cxa_thread_finalize() {
    // The list is not borrowed during a call: a destructor may register
    // another one (a thread-local object made while an object is destroyed),
    // which runs next.
    loop {
        let Some(entry) = DTORS.borrow_mut().pop() else {
            break;
        };
        (entry.func)(entry.obj);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ITM_deregisterTMCloneTable(_ptr: *mut c_void) {
    // No-op
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ITM_registerTMCloneTable(_ptr: *mut c_void, _len: usize) {
    // No-op
}
