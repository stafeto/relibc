//! `sys/auxv.h` implementation.
//!
//! Non-POSIX, see <https://www.man7.org/linux/man-pages/man3/getauxval.3.html>.

use crate::platform::types::c_ulong;

pub use crate::platform::auxv_defs::*;

#[cfg(stafeto)]
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// The auxiliary vector of the start, sorted by key, which getauxval reads
/// on stafeto: AT_SECURE tells a set-ID program it starts in the secure
/// mode.
#[cfg(stafeto)]
static AUXVS: AtomicPtr<[usize; 2]> = AtomicPtr::new(core::ptr::null_mut());
#[cfg(stafeto)]
static AUXVS_LEN: AtomicUsize = AtomicUsize::new(0);

/// Keeps the start's auxiliary vector for getauxval, once, before any
/// other thread runs.
#[cfg(stafeto)]
pub(crate) fn keep(auxvs: alloc::boxed::Box<[[usize; 2]]>) {
    let len = auxvs.len();
    let leaked = alloc::boxed::Box::leak(auxvs);
    AUXVS_LEN.store(len, Ordering::Relaxed);
    AUXVS.store(leaked.as_mut_ptr(), Ordering::Release);
}

/// See <https://www.man7.org/linux/man-pages/man3/getauxval.3.html>.
#[cfg(not(stafeto))]
#[unsafe(no_mangle)]
pub extern "C" fn getauxval(_t: c_ulong) -> c_ulong {
    0
}

/// See <https://www.man7.org/linux/man-pages/man3/getauxval.3.html>: the
/// value of `t` in the start's auxiliary vector; 0 with ENOENT for none.
#[cfg(stafeto)]
#[unsafe(no_mangle)]
pub extern "C" fn getauxval(t: c_ulong) -> c_ulong {
    let pointer = AUXVS.load(Ordering::Acquire);
    let auxvs = if pointer.is_null() {
        &[][..]
    } else {
        // SAFETY: `keep` leaked the vector, which lives for good.
        unsafe { core::slice::from_raw_parts(pointer, AUXVS_LEN.load(Ordering::Relaxed)) }
    };
    match crate::platform::get_auxv(auxvs, t as usize) {
        Some(value) => value as c_ulong,
        None => {
            crate::platform::ERRNO.set(crate::header::errno::ENOENT);
            0
        }
    }
}
