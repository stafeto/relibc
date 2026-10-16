//! `dlfcn.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/dlfcn.h.html>.

// FIXME(andypython): remove this when #![allow(warnings, unused_variables)] is
// dropped from src/lib.rs.
#![warn(warnings, unused_variables)]

use core::{
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(not(stafeto))]
use alloc::sync::Arc;
#[cfg(not(stafeto))]
use core::str;

#[cfg(not(stafeto))]
use crate::{
    c_str::CStr,
    ld_so::{
        dso::DSO,
        linker::{DlError, Resolve, ScopeKind},
        tcb::Tcb,
    },
};
use crate::platform::types::{c_char, c_int, c_void};

/// Relocations are performed at an implementation-defined time.
pub const RTLD_LAZY: c_int = 1 << 0;
/// Relocations are performed when the object is loaded.
pub const RTLD_NOW: c_int = 1 << 1;
/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man3/dlopen.3.html>.
/// Don't load the shared object.
pub const RTLD_NOLOAD: c_int = 1 << 2;
/// All symbols are available for relocation processing of other modules.
pub const RTLD_GLOBAL: c_int = 1 << 8;
/// All symbold are not made available for relocation processing by other
/// modules.
pub const RTLD_LOCAL: c_int = 0x0000;

/// Special purpose value for `handle` for `dlsym()` reserved by POSIX for
/// future use.
/// The identifier lookup happens in the normal global scope; that is, a
/// search for an identifier using `handle` would find the same definition
/// as a direct use of this identifier in the program code.
#[expect(
    clippy::zero_ptr,
    reason = "cbindgen issue: https://github.com/mozilla/cbindgen/issues/948"
)]
pub const RTLD_DEFAULT: *mut c_void = 0 as *mut c_void; // XXX: cbindgen doesn't like ptr::null_mut() for publically exported constants

#[cfg_attr(stafeto, allow(dead_code))]
static ERROR_NOT_SUPPORTED: &core::ffi::CStr = c"dlfcn not supported";

/// stafeto links every program statically: no shared object is loaded and a
/// program carries no table of symbols. `dlopen(NULL)` answers a handle of
/// the program all the same, as the standard says, which `dlsym` finds no
/// symbol in and `dlclose` accepts; every other call says why it fails.
#[cfg(stafeto)]
mod fixed {
    pub static PROGRAM: u8 = 0;
    pub const NO_OBJECTS: &core::ffi::CStr =
        c"dlopen: this system links programs statically and loads no shared object";
    pub const NO_SYMBOLS: &core::ffi::CStr =
        c"dlsym: the program carries no table of symbols to search by name";
    pub const NO_HANDLE: &core::ffi::CStr = c"dlclose: not a handle that dlopen gave";
    pub const NO_MODE: &core::ffi::CStr = c"dlopen: the mode has neither RTLD_LAZY nor RTLD_NOW";
}

#[thread_local]
static ERROR: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(stafeto))]
fn set_last_error(error: DlError) {
    ERROR.store(error.repr().as_ptr() as usize, Ordering::SeqCst);
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/dlfcn.h.html>.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct Dl_info_t {
    /// Pathname of mapped object file.
    dli_fname: *const c_char,
    /// Base of mapped address range.
    dli_fbase: *mut c_void,
    /// Symbol name or null pointer.
    dli_sname: *const c_char,
    /// Symbol address of null pointer.
    dli_saddr: *mut c_void,
}

/// alias as per spec update: <https://www.austingroupbugs.net/view.php?id=1847>
pub type Dl_info = Dl_info_t;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dladdr.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dladdr(_addr: *const c_void, info: *mut Dl_info_t) -> c_int {
    //TODO
    unsafe {
        (*info).dli_fname = ptr::null();
        (*info).dli_fbase = ptr::null_mut();
        (*info).dli_sname = ptr::null();
        (*info).dli_saddr = ptr::null_mut();
    }
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dlopen.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlopen(cfilename: *const c_char, flags: c_int) -> *mut c_void {
    #[cfg(stafeto)]
    {
        if flags & (RTLD_LAZY | RTLD_NOW) == 0 {
            ERROR.store(fixed::NO_MODE.as_ptr() as usize, Ordering::SeqCst);
            return ptr::null_mut();
        }
        if cfilename.is_null() {
            return (&raw const fixed::PROGRAM).cast_mut().cast::<c_void>();
        }
        ERROR.store(fixed::NO_OBJECTS.as_ptr() as usize, Ordering::SeqCst);
        return ptr::null_mut();
    }
    #[cfg(not(stafeto))]
    {
    //TODO support all sort of flags
    let resolve = if flags & RTLD_NOW == RTLD_NOW {
        Resolve::Now
    } else {
        Resolve::Lazy
    };

    let scope = if flags & RTLD_GLOBAL == RTLD_GLOBAL {
        ScopeKind::Global
    } else {
        ScopeKind::Local
    };

    let noload = flags & RTLD_NOLOAD == RTLD_NOLOAD;

    let filename = if cfilename.is_null() {
        None
    } else {
        unsafe {
            Some(str::from_utf8_unchecked(
                CStr::from_ptr(cfilename).to_bytes(),
            ))
        }
    };

    let Some(tcb) = (unsafe { Tcb::current() }) else {
        ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
        return ptr::null_mut();
    };

    if tcb.linker_ptr.is_null() {
        ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
        return ptr::null_mut();
    }

    let mut linker = unsafe { (*tcb.linker_ptr).lock() };

    match linker.load_library(filename, resolve, scope, noload) {
        Ok(handle) => Arc::into_raw(handle).cast::<c_void>().cast_mut(),
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dlsym.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    #[cfg(stafeto)]
    {
        let _ = (handle, symbol);
        ERROR.store(fixed::NO_SYMBOLS.as_ptr() as usize, Ordering::SeqCst);
        return ptr::null_mut();
    }
    #[cfg(not(stafeto))]
    {
    let handle = unsafe { handle.cast::<DSO>().as_ref() };

    if symbol.is_null() {
        ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
        return ptr::null_mut();
    }

    let symbol_str = unsafe { str::from_utf8_unchecked(CStr::from_ptr(symbol).to_bytes()) };

    // FIXME(andypython): just call obj.scope.get_sym() directly or search the
    // global scope.  The rest is unnecessary as Linker::get_sym() does not
    // depend on the Linker state.
    let Some(tcb) = (unsafe { Tcb::current() }) else {
        ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
        return ptr::null_mut();
    };

    if tcb.linker_ptr.is_null() {
        ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
        return ptr::null_mut();
    }

    let linker = unsafe { (*tcb.linker_ptr).lock() };

    match linker.get_sym(handle, symbol_str) {
        Some(sym) => sym,
        _ => {
            ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
            ptr::null_mut()
        }
    }
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dlclose.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dlclose(handle: *mut c_void) -> c_int {
    #[cfg(stafeto)]
    {
        if handle.cast_const() == (&raw const fixed::PROGRAM).cast::<c_void>() {
            return 0;
        }
        ERROR.store(fixed::NO_HANDLE.as_ptr() as usize, Ordering::SeqCst);
        return -1;
    }
    #[cfg(not(stafeto))]
    {
    let Some(tcb) = (unsafe { Tcb::current() }) else {
        ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
        return -1;
    };

    if tcb.linker_ptr.is_null() {
        ERROR.store(ERROR_NOT_SUPPORTED.as_ptr() as usize, Ordering::SeqCst);
        return -1;
    };

    if handle.is_null() {
        set_last_error(DlError::InvalidHandle);
        return -1;
    }

    let mut linker = unsafe { (*tcb.linker_ptr).lock() };
    let obj = unsafe { Arc::from_raw(handle.cast::<DSO>()) };
    linker.unload(obj);
    0
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dlerror.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dlerror() -> *mut c_char {
    ERROR.swap(0, Ordering::SeqCst) as *mut c_char
}
