//! `nl_types.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/nl_types.h.html>.
//!
//! The catalog file is the format of the BSDs (the one `gencat` writes):
//! `catopen` reads it whole, `catgets` searches the sets and the messages
//! by their numbers, as the BSD library does.

use alloc::{boxed::Box, vec::Vec};
use core::{ptr, slice};

use crate::{
    c_str::CStr,
    fs::File,
    header::{
        errno::{EBADF, EINVAL, ENOENT, ENOMSG},
        fcntl::O_RDONLY,
        stdlib::getenv,
    },
    io::Read,
    platform::{
        self,
        types::{c_char, c_int, c_void},
    },
};

/// The default set number.
pub const NL_SETD: c_int = 1;
/// `catopen`: the language comes from the locale, not from LANG.
pub const NL_CAT_LOCALE: c_int = 1;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/nl_types.h.html>.
#[repr(C)]
pub struct __nl_cat_d {
    data: *mut c_void,
    size: c_int,
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/nl_types.h.html>.
#[allow(non_camel_case_types)]
pub type nl_catd = *mut __nl_cat_d;

const NLERR: nl_catd = -1isize as nl_catd;
const MAGIC: u32 = 0xff88_ff89;
const HEADER: usize = 20;
/// The catalogs of the system, as the BSDs have them.
const DEFAULT_NLSPATH: &[u8] = b"/usr/share/nls/%L/%N.cat:/usr/share/nls/%N/%L";
/// A catalog is read whole: 1 MiB at most.
const SIZE_MAX: usize = 1 << 20;

fn be(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Whether the bytes are a catalog whose headers lie inside them.
fn valid(data: &[u8]) -> bool {
    let (Some(magic), Some(nsets), Some(msgs), Some(texts)) =
        (be(data, 0), be(data, 4), be(data, 12), be(data, 16))
    else {
        return false;
    };
    let (nsets, msgs, texts) = (nsets as usize, msgs as usize, texts as usize);
    magic == MAGIC
        && HEADER + nsets * 12 <= data.len()
        && HEADER + msgs <= data.len()
        && HEADER + texts <= data.len()
}

fn load(path: &[u8]) -> Result<Vec<u8>, c_int> {
    let mut name = Vec::with_capacity(path.len() + 1);
    name.extend_from_slice(path);
    name.push(0);
    let cstr = unsafe { CStr::from_ptr(name.as_ptr().cast()) };
    let file = File::open(cstr, O_RDONLY).map_err(|e| e.0)?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 1024];
    let mut reader = &file;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if data.len() + n > SIZE_MAX {
                    return Err(ENOENT);
                }
                data.extend_from_slice(&chunk[..n]);
            }
            Err(_) => return Err(ENOENT),
        }
    }
    if valid(&data) { Ok(data) } else { Err(ENOENT) }
}

/// The catalog `name` expands to under the template `nlspath` (%N the name,
/// %L the language, %l its language part, %t its territory, %c its code set).
fn expand(template: &[u8], name: &[u8], lang: &[u8]) -> Vec<u8> {
    let mut path = Vec::new();
    let mut parts = lang.splitn(2, |&b| b == b'.');
    let locale = parts.next().unwrap_or(&[]);
    let code = parts.next().unwrap_or(&[]);
    let mut halves = locale.splitn(2, |&b| b == b'_');
    let language = halves.next().unwrap_or(&[]);
    let territory = halves.next().unwrap_or(&[]);
    let mut i = 0;
    while i < template.len() {
        if template[i] == b'%' && i + 1 < template.len() {
            i += 1;
            match template[i] {
                b'N' => path.extend_from_slice(name),
                b'L' => path.extend_from_slice(lang),
                b'l' => path.extend_from_slice(language),
                b't' => path.extend_from_slice(territory),
                b'c' => path.extend_from_slice(code),
                b'%' => path.push(b'%'),
                other => {
                    path.push(b'%');
                    path.push(other);
                }
            }
        } else {
            path.push(template[i]);
        }
        i += 1;
    }
    path
}

fn environment(name: &core::ffi::CStr) -> Option<&'static [u8]> {
    let value = unsafe { getenv(name.as_ptr()) };
    (!value.is_null()).then(|| unsafe { CStr::from_ptr(value) }.to_bytes())
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/catopen.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catopen(name: *const c_char, oflag: c_int) -> nl_catd {
    let fail = |errno: c_int| -> nl_catd {
        platform::ERRNO.set(errno);
        NLERR
    };
    if name.is_null() {
        return fail(EINVAL);
    }
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    if name.is_empty() {
        return fail(EINVAL);
    }
    let found = if name.contains(&b'/') {
        // A path is taken as it is.
        load(name)
    } else {
        // The language: LC_MESSAGES by the locale's variables, else LANG.
        let lang = if oflag & NL_CAT_LOCALE != 0 {
            environment(c"LC_ALL")
                .filter(|v| !v.is_empty())
                .or_else(|| environment(c"LC_MESSAGES").filter(|v| !v.is_empty()))
                .or_else(|| environment(c"LANG").filter(|v| !v.is_empty()))
        } else {
            environment(c"LANG").filter(|v| !v.is_empty())
        }
        .unwrap_or(b"C");
        let nlspath = environment(c"NLSPATH")
            .filter(|v| !v.is_empty())
            .unwrap_or(DEFAULT_NLSPATH);
        let mut result = Err(ENOENT);
        for template in nlspath.split(|&b| b == b':') {
            if template.is_empty() {
                continue;
            }
            let path = expand(template, name, lang);
            if let Ok(data) = load(&path) {
                result = Ok(data);
                break;
            }
        }
        result
    };
    match found {
        Ok(data) => {
            let size = data.len() as c_int;
            let data = Box::into_raw(data.into_boxed_slice()).cast::<c_void>();
            Box::into_raw(Box::new(__nl_cat_d { data, size }))
        }
        Err(errno) => fail(errno),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/catgets.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catgets(
    catd: nl_catd,
    set_id: c_int,
    msg_id: c_int,
    s: *const c_char,
) -> *mut c_char {
    if catd.is_null() || catd == NLERR {
        platform::ERRNO.set(EBADF);
        return s.cast_mut();
    }
    let catalog = unsafe { &*catd };
    let data = unsafe { slice::from_raw_parts(catalog.data.cast::<u8>(), catalog.size as usize) };
    let nsets = be(data, 4).unwrap_or(0) as i64;
    let msg_headers = HEADER + be(data, 12).unwrap_or(0) as usize;
    let texts = HEADER + be(data, 16).unwrap_or(0) as usize;
    // The sets by their numbers, then the messages of the set by theirs:
    // binary searches.
    let (mut lo, mut hi) = (0i64, nsets - 1);
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let at = HEADER + mid as usize * 12;
        let (Some(number), Some(count), Some(first)) =
            (be(data, at), be(data, at + 4), be(data, at + 8))
        else {
            break;
        };
        let order = i64::from(set_id) - i64::from(number as i32);
        if order == 0 {
            let (mut l, mut u) = (i64::from(first), i64::from(first) + i64::from(count) - 1);
            while l <= u {
                let m = (l + u) / 2;
                let at = msg_headers + m as usize * 12;
                let (Some(number), Some(offset)) = (be(data, at), be(data, at + 8)) else {
                    break;
                };
                let order = i64::from(msg_id) - i64::from(number as i32);
                if order == 0 {
                    let text = texts + offset as usize;
                    // The text ends inside the file with its NUL.
                    if data.get(text..).is_some_and(|rest| rest.contains(&0)) {
                        return unsafe { catalog.data.cast::<u8>().add(text) }.cast::<c_char>();
                    }
                    break;
                } else if order < 0 {
                    u = m - 1;
                } else {
                    l = m + 1;
                }
            }
            break;
        } else if order < 0 {
            hi = mid - 1;
        } else {
            lo = mid + 1;
        }
    }
    platform::ERRNO.set(ENOMSG);
    s.cast_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/catclose.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catclose(catd: nl_catd) -> c_int {
    if catd.is_null() || catd == NLERR {
        platform::ERRNO.set(EBADF);
        return -1;
    }
    let catalog = unsafe { Box::from_raw(catd) };
    drop(unsafe {
        Box::from_raw(ptr::slice_from_raw_parts_mut(
            catalog.data.cast::<u8>(),
            catalog.size as usize,
        ))
    });
    0
}
