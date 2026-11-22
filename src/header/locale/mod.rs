//! `locale.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/locale.h.html>.

use alloc::{boxed::Box, ffi::CString, string::String};
use core::{ptr, str::FromStr};

use crate::{
    c_str::CStr,
    casting::ByteLiteral,
    error::{Errno, ResultExtPtrMut},
    fs::File,
    header::{errno, fcntl},
    io::Read,
    platform::types::{c_char, c_int},
};

// Can't use &str because of the mutability
static mut C_LOCALE: [c_char; 2] = [ByteLiteral::cast_cchar(b'C'), 0];

mod constants;
use constants::*;
mod data;
use data::*;

use super::bits_locale_t::locale_t;
/// constant struct to "C" or "POSIX" locale
/// mutable because POSIX demands a mutable pointer
static mut POSIX_LOCALE: lconv = posix_lconv();
pub const LC_GLOBAL_LOCALE: locale_t = -1isize as locale_t;
/// process-wide locale, used by setlocale() and localeconv()
static mut GLOBAL_LOCALE: *mut GlobalLocaleData = ptr::null_mut();
/// thread-wide locale, used by uselocale() and localeconv()
#[thread_local]
pub(crate) static mut THREAD_LOCALE: *mut LocaleData = ptr::null_mut();

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/localeconv.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn localeconv() -> *mut lconv {
    let current = unsafe { uselocale(ptr::null_mut()) };
    if current == LC_GLOBAL_LOCALE || current.is_null() {
        if !unsafe { GLOBAL_LOCALE.is_null() } {
            // safety: GLOBAL_LOCALE is never set to null again
            unsafe { &raw mut (*GLOBAL_LOCALE).data.lconv }
        } else {
            &raw mut POSIX_LOCALE
        }
    } else {
        let current = current.cast::<LocaleData>();
        unsafe { &raw mut (*current).lconv }
    }
}

fn is_posix_locale_name(name: &str) -> bool {
    matches!(
        name,
        "" | "C" | "POSIX" | "C.UTF-8" | "C.utf8" | "POSIX.UTF-8" | "POSIX.utf8"
    )
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/setlocale.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char {
    if unsafe { GLOBAL_LOCALE.is_null() } {
        let new_global = GlobalLocaleData::new();
        unsafe { GLOBAL_LOCALE = Box::into_raw(new_global) };
    };
    let Some(global) = (unsafe { GLOBAL_LOCALE.as_mut() }) else {
        return ptr::null_mut();
    };

    if locale.is_null() {
        let Some(name) = global.get_name(category) else {
            return ptr::null_mut();
        };
        return name.as_ptr().cast_mut();
    }

    let name = unsafe { CStr::from_ptr(locale).to_str().unwrap_or("C") };
    // "" names the locale of the environment: LC_ALL, then LC_<category>,
    // then LANG, and "C" when none of them is set and non-empty.
    let from_environment = name.is_empty();
    let resolved = if from_environment {
        environment_locale_name(category)
    } else {
        String::from(name)
    };
    let name = resolved.as_str();

    match locale_for_name(name, from_environment) {
        Ok(loc_ptr) => {
            global.data.copy_category(&loc_ptr, category);
            let Some(name) = global.set_name(category, CString::from_str(name).unwrap()) else {
                return ptr::null_mut();
            };
            name.as_ptr().cast_mut()
        }
        Err(_) => ptr::null_mut(),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/uselocale.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uselocale(newloc: locale_t) -> locale_t {
    let old_loc = if unsafe { THREAD_LOCALE.is_null() } {
        LC_GLOBAL_LOCALE
    } else {
        (unsafe { THREAD_LOCALE }) as locale_t
    };

    if !newloc.is_null() {
        unsafe {
            THREAD_LOCALE = if newloc == LC_GLOBAL_LOCALE {
                ptr::null_mut()
            } else {
                newloc.cast::<LocaleData>()
            }
        };
    }

    old_loc
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/newlocale.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newlocale(mask: c_int, locale: *const c_char, base: locale_t) -> locale_t {
    let name = unsafe { CStr::from_ptr(locale) }
        .to_string_lossy()
        .into_owned();
    let from_environment = name.is_empty();
    let name = if from_environment {
        // The environment's name for the categories of the mask: LC_ALL,
        // then LANG (a single category's variable needs the category, which
        // a mask of several does not have).
        environment_locale_name(match mask {
            LC_COLLATE_MASK => LC_COLLATE,
            LC_CTYPE_MASK => LC_CTYPE,
            LC_MESSAGES_MASK => LC_MESSAGES,
            LC_MONETARY_MASK => LC_MONETARY,
            LC_NUMERIC_MASK => LC_NUMERIC,
            LC_TIME_MASK => LC_TIME,
            _ => LC_ALL,
        })
    } else {
        name
    };
    let name = name.as_str();
    let mut new_locale = locale_for_name(name, from_environment);
    if base != LC_GLOBAL_LOCALE {
        // borrowing here
        let base = base.cast_const().cast::<LocaleData>();
        if let Ok(new_locale) = new_locale.as_mut()
            && let Some(base) = unsafe { base.as_ref() }
        {
            // copy old values if not containing the mask
            if (mask & LC_NUMERIC_MASK) == 0 {
                new_locale.copy_category(base, LC_NUMERIC);
            }
            if (mask & LC_MONETARY_MASK) == 0 {
                new_locale.copy_category(base, LC_MONETARY);
            }
            // TODO: other categories?
        }
    }
    new_locale.or_errno_null_mut().cast()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/freelocale.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn freelocale(loc: locale_t) {
    if !loc.is_null() && loc != LC_GLOBAL_LOCALE {
        drop(unsafe { Box::from_raw(loc.cast::<LocaleData>()) });
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/duplocale.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duplocale(loc: locale_t) -> locale_t {
    if loc.is_null() {
        // TODO: errno?
        loc
    } else if loc == LC_GLOBAL_LOCALE {
        Box::into_raw(LocaleData::posix()) as locale_t
    } else {
        // borrowing here
        let loc = loc.cast_const().cast::<LocaleData>();
        Box::into_raw(unsafe { Box::from((*loc).clone()) }) as locale_t
    }
}

/// The value of an environment variable when it is set and not empty.
fn non_empty_env(name: &[u8]) -> Option<String> {
    let mut key = alloc::vec::Vec::from(name);
    key.push(0);
    let value = unsafe { crate::header::stdlib::getenv(key.as_ptr().cast()) };
    if value.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(value) }.to_str().ok()?;
    if value.is_empty() {
        None
    } else {
        Some(String::from(value))
    }
}

/// The name that "" stands for in `category`: LC_ALL, then the variable of
/// the category, then LANG, and "C" when none is set and non-empty.
fn environment_locale_name(category: c_int) -> String {
    let own: Option<&[u8]> = match category {
        LC_COLLATE => Some(b"LC_COLLATE"),
        LC_CTYPE => Some(b"LC_CTYPE"),
        LC_MESSAGES => Some(b"LC_MESSAGES"),
        LC_MONETARY => Some(b"LC_MONETARY"),
        LC_NUMERIC => Some(b"LC_NUMERIC"),
        LC_TIME => Some(b"LC_TIME"),
        _ => None,
    };
    non_empty_env(b"LC_ALL")
        .or_else(|| own.and_then(non_empty_env))
        .or_else(|| non_empty_env(b"LANG"))
        .unwrap_or_else(|| String::from("C"))
}

/// Whether the process started in the secure mode (AT_SECURE).
fn secure_mode() -> bool {
    let errno = crate::platform::ERRNO.get();
    let secure = crate::header::sys_auxv::getauxval(crate::header::elf::AT_SECURE as _) != 0;
    crate::platform::ERRNO.set(errno);
    secure
}

/// The data of the locale `name` (not empty). The multibyte encoding is
/// always UTF-8, so the names of the UTF-8 forms of the POSIX locale name
/// that locale. Any other name is a file of the locale definitions; one with
/// no file fails (POSIX: an unsupported name), and so does a locale asked for
/// by name with `newlocale`. A name that came from the environment (`""`)
/// of the form `<something>.UTF-8` or `.utf8` with no file takes the data of
/// C.UTF-8 under its own name: the user's variable names a locale this
/// system lacks, and the program goes on with the one encoding it has.
fn locale_for_name(name: &str, from_environment: bool) -> Result<Box<LocaleData>, Errno> {
    if is_posix_locale_name(name) {
        return Ok(LocaleData::posix());
    }
    match load_locale_file(name) {
        Err(Errno(errno::ENOENT))
            if from_environment
                && !name.contains('/')
                && (name.ends_with(".UTF-8") || name.ends_with(".utf8")) =>
        {
            let mut data = LocaleData::posix();
            data.name = CString::from_str(name).map_err(|_| Errno(errno::EINVAL))?;
            Ok(data)
        }
        other => other,
    }
}

pub(crate) fn load_locale_file(name: &str) -> Result<Box<LocaleData>, Errno> {
    // A name with a slash would leave the directory of the definitions: a
    // set-ID program does not follow it.
    if name.contains('/') && secure_mode() {
        return Err(Errno(errno::EINVAL));
    }
    let mut path = String::from("/usr/share/i18n/locales/");
    path.push_str(name);

    let path_c = CString::new(path).map_err(|_| Errno(errno::EINVAL))?;
    let mut content = String::new();

    let mut file = File::open(path_c.as_c_str().into(), fcntl::O_RDONLY)?;
    file.read_to_string(&mut content)
        .map_err(|_| Errno(errno::EIO))?;

    let toml = PosixLocaleDef::parse(&content);
    Ok(LocaleData::new(CString::from_str(name).unwrap(), toml))
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/getlocalename_l.html>.
///
/// # Safety
/// `locale` is `LC_GLOBAL_LOCALE` or a locale made by `newlocale` or `duplocale`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getlocalename_l(category: c_int, locale: locale_t) -> *const c_char {
    if locale == LC_GLOBAL_LOCALE {
        return unsafe { setlocale(category, ptr::null()) };
    }
    if locale.is_null() {
        return ptr::null();
    }
    unsafe { (*locale.cast::<LocaleData>()).name.as_ptr() }
}
