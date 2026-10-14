//! `libintl.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/libintl.h.html>.
//!
//! Only the C and POSIX locales exist, and the specification requires the
//! message functions to return the message they were given in them. The
//! domain, directory and code set bindings are kept so that the functions that
//! set and read them answer as they should.

use alloc::{string::String, vec::Vec};

use crate::{
    c_str::{CStr, CString},
    header::{bits_locale_t::locale_t, errno::ENOMEM},
    platform::{
        self,
        types::{c_char, c_int, c_ulong},
    },
    sync::Mutex,
};

/// The domain that `textdomain` starts with.
const DEFAULT_DOMAIN: &[u8] = b"messages\0";
/// The directory of the message catalogs that `bindtextdomain` starts with.
const DEFAULT_DIRECTORY: &[u8] = b"/usr/share/locale\0";

/// A domain with the bindings made for it. The strings are leaked, so a
/// pointer that was returned stays valid after the binding is replaced.
struct Binding {
    domain: String,
    directory: *const c_char,
    codeset: *const c_char,
}

// The raw pointers refer to leaked strings that nothing mutates.
unsafe impl Send for Binding {}

/// cbindgen:ignore
static BINDINGS: Mutex<Vec<Binding>> = Mutex::new(Vec::new());
/// cbindgen:ignore
static CURRENT_DOMAIN: Mutex<usize> = Mutex::new(0);

fn leak(string: CStr) -> *const c_char {
    CString::from(string.to_cstr()).into_raw().cast_const()
}

/// The binding of `domain`, made when it is not there yet.
fn binding<'a>(list: &'a mut Vec<Binding>, domain: CStr) -> Option<&'a mut Binding> {
    let name = domain.to_string_lossy().into_owned();
    if let Some(at) = list.iter().position(|b| b.domain == name) {
        return list.get_mut(at);
    }
    list.try_reserve(1).ok()?;
    list.push(Binding {
        domain: name,
        directory: DEFAULT_DIRECTORY.as_ptr().cast(),
        codeset: core::ptr::null(),
    });
    list.last_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/gettext.html>.
#[unsafe(no_mangle)]
pub extern "C" fn gettext(msgid: *const c_char) -> *mut c_char {
    msgid.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/gettext_l.html>.
#[unsafe(no_mangle)]
pub extern "C" fn gettext_l(msgid: *const c_char, _locale: locale_t) -> *mut c_char {
    msgid.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dgettext.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dgettext(_domainname: *const c_char, msgid: *const c_char) -> *mut c_char {
    msgid.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dgettext_l.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dgettext_l(
    _domainname: *const c_char,
    msgid: *const c_char,
    _locale: locale_t,
) -> *mut c_char {
    msgid.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dcgettext.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dcgettext(
    _domainname: *const c_char,
    msgid: *const c_char,
    _category: c_int,
) -> *mut c_char {
    msgid.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dcgettext_l.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dcgettext_l(
    _domainname: *const c_char,
    msgid: *const c_char,
    _category: c_int,
    _locale: locale_t,
) -> *mut c_char {
    msgid.cast_mut()
}

fn plural(msgid1: *const c_char, msgid2: *const c_char, n: c_ulong) -> *mut c_char {
    if n == 1 { msgid1 } else { msgid2 }.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/ngettext.html>.
#[unsafe(no_mangle)]
pub extern "C" fn ngettext(
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    plural(msgid1, msgid2, n)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/ngettext_l.html>.
#[unsafe(no_mangle)]
pub extern "C" fn ngettext_l(
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    _locale: locale_t,
) -> *mut c_char {
    plural(msgid1, msgid2, n)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dngettext.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dngettext(
    _domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    plural(msgid1, msgid2, n)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dngettext_l.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dngettext_l(
    _domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    _locale: locale_t,
) -> *mut c_char {
    plural(msgid1, msgid2, n)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dcngettext.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dcngettext(
    _domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    _category: c_int,
) -> *mut c_char {
    plural(msgid1, msgid2, n)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/dcngettext_l.html>.
#[unsafe(no_mangle)]
pub extern "C" fn dcngettext_l(
    _domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    _category: c_int,
    _locale: locale_t,
) -> *mut c_char {
    plural(msgid1, msgid2, n)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/textdomain.html>.
///
/// # Safety
/// `domainname` is null or points to a string that ends with a nul.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn textdomain(domainname: *const c_char) -> *mut c_char {
    let mut current = CURRENT_DOMAIN.lock();
    if !domainname.is_null() {
        let name = unsafe { CStr::from_ptr(domainname) };
        *current = if name.to_bytes().is_empty() {
            DEFAULT_DOMAIN.as_ptr() as usize
        } else {
            leak(name) as usize
        };
    }
    if *current == 0 {
        *current = DEFAULT_DOMAIN.as_ptr() as usize;
    }
    *current as *mut c_char
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/bindtextdomain.html>.
///
/// # Safety
/// `domainname` and `dirname` are null or point to strings that end with a nul.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bindtextdomain(
    domainname: *const c_char,
    dirname: *const c_char,
) -> *mut c_char {
    if domainname.is_null() {
        return core::ptr::null_mut();
    }
    let domain = unsafe { CStr::from_ptr(domainname) };
    if domain.to_bytes().is_empty() {
        return core::ptr::null_mut();
    }
    let mut list = BINDINGS.lock();
    let Some(entry) = binding(&mut list, domain) else {
        platform::ERRNO.set(ENOMEM);
        return core::ptr::null_mut();
    };
    if !dirname.is_null() {
        entry.directory = leak(unsafe { CStr::from_ptr(dirname) });
    }
    entry.directory.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/bind_textdomain_codeset.html>.
///
/// # Safety
/// `domainname` and `codeset` are null or point to strings that end with a nul.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bind_textdomain_codeset(
    domainname: *const c_char,
    codeset: *const c_char,
) -> *mut c_char {
    if domainname.is_null() {
        return core::ptr::null_mut();
    }
    let domain = unsafe { CStr::from_ptr(domainname) };
    if domain.to_bytes().is_empty() {
        return core::ptr::null_mut();
    }
    let mut list = BINDINGS.lock();
    let Some(entry) = binding(&mut list, domain) else {
        platform::ERRNO.set(ENOMEM);
        return core::ptr::null_mut();
    };
    if !codeset.is_null() {
        entry.codeset = leak(unsafe { CStr::from_ptr(codeset) });
    }
    entry.codeset.cast_mut()
}
