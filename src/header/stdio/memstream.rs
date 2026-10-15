//! Streams over memory: `fmemopen`, `open_memstream` and `open_wmemstream`.
//!
//! The content of such a stream lives in an unlinked temporary file, so the
//! stream is an ordinary one. The caller's buffer follows the file at each
//! `fflush`, `fseek` and `fclose`, which are the points where the specification
//! requires it to be current.

use alloc::{boxed::Box, vec::Vec};

use super::{
    FILE, Pending, SEEK_CUR, SEEK_END, SEEK_SET,
    constants::{F_APP, F_NOWR},
    helpers,
};
use crate::{
    c_str::CStr,
    error::{Errno, ResultExtPtrMut},
    header::{
        errno::{EINVAL, ENOMEM},
        fcntl::O_CLOEXEC,
        stdlib,
    },
    platform::{
        self, Pal, Sys,
        types::{c_char, c_int, c_void, size_t, wchar_t},
    },
};

/// Where the buffer of a memory stream is, and what it is.
#[derive(Clone, Copy)]
pub(crate) enum MemKind {
    /// `fmemopen`: a buffer of the caller (null for an internal one).
    Fixed {
        buf: *mut u8,
        size: usize,
        binary: bool,
    },
    /// `open_memstream`: the buffer grows with `realloc`.
    Dynamic {
        bufp: *mut *mut c_char,
        sizep: *mut size_t,
    },
    /// `open_wmemstream`: as above, in wide characters.
    Wide {
        bufp: *mut *mut wchar_t,
        sizep: *mut size_t,
    },
}

/// An unlinked file in `/tmp`, open for reading and writing.
fn anonymous_file() -> Result<c_int, Errno> {
    let mut name = *b"/tmp/memstrXXXXXX\0";
    let name = name.as_mut_ptr().cast::<c_char>();
    let fd = unsafe { stdlib::mkostemp(name, O_CLOEXEC) };
    if fd < 0 {
        return Err(Errno(platform::ERRNO.get()));
    }
    let path = unsafe { CStr::from_ptr(name) };
    let _ = Sys::unlink(path);
    Ok(fd)
}

fn read_all(fd: c_int, len: usize) -> Option<Vec<u8>> {
    let mut content = Vec::new();
    content.try_reserve_exact(len).ok()?;
    content.resize(len, 0);
    let mut done = 0;
    while done < len {
        match Sys::pread(fd, &mut content[done..], done as _) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(_) => return None,
        }
    }
    content.truncate(done);
    Some(content)
}

fn write_all(fd: c_int, content: &[u8]) -> bool {
    let mut done = 0;
    while done < content.len() {
        match Sys::pwrite(fd, &content[done..], done as _) {
            Ok(0) | Err(_) => return false,
            Ok(n) => done += n,
        }
    }
    true
}

/// The characters of UTF-8 text; a cut-off sequence at the end is left out.
fn wide_chars(bytes: &[u8]) -> Vec<wchar_t> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let lead = bytes[at];
        let (len, init) = match lead {
            0x00..=0x7f => (1, u32::from(lead)),
            0xc0..=0xdf => (2, u32::from(lead & 0x1f)),
            0xe0..=0xef => (3, u32::from(lead & 0x0f)),
            0xf0..=0xf7 => (4, u32::from(lead & 0x07)),
            _ => {
                at += 1;
                continue;
            }
        };
        if at + len > bytes.len() {
            break;
        }
        let mut c = init;
        for b in &bytes[at + 1..at + len] {
            c = (c << 6) | u32::from(b & 0x3f);
        }
        out.push(c as wchar_t);
        at += len;
    }
    out
}

impl FILE {
    /// How many of `want` bytes the buffer of an `fmemopen` stream still has
    /// room for at the place where a write lands (a stream of any other kind
    /// has room for all). The bytes in the stream's own buffer count.
    pub(crate) fn memory_room(&self, want: usize) -> usize {
        let Some(MemKind::Fixed { size, .. }) = self.mem else {
            return want;
        };
        let fd = *self.file;
        let Ok(cur) = Sys::lseek(fd, 0, SEEK_CUR) else {
            return want;
        };
        let at = if self.flags & F_APP != 0 {
            let end = Sys::lseek(fd, 0, SEEK_END);
            let _ = Sys::lseek(fd, cur, SEEK_SET);
            end.unwrap_or(cur) as usize
        } else {
            cur as usize
        } + self.writer.pending() as usize;
        want.min(size.saturating_sub(at))
    }

    /// Checks the place a seek of a stream over a fixed buffer landed on:
    /// past the buffer is `EINVAL`, and the stream goes back to `from`.
    pub(crate) fn memory_seek_ok(&self, from: i64) -> bool {
        let Some(MemKind::Fixed { size, .. }) = self.mem else {
            return true;
        };
        let fd = *self.file;
        match Sys::lseek(fd, 0, SEEK_CUR) {
            Ok(to) if to as usize <= size => true,
            Ok(_) => {
                let _ = Sys::lseek(fd, from, SEEK_SET);
                platform::ERRNO.set(EINVAL);
                false
            }
            Err(_) => true,
        }
    }

    /// Brings the buffer of a memory stream up to the content of its file.
    /// The stream is locked and flushed. False when the buffer could not be
    /// made.
    pub(crate) fn sync_memory(&mut self) -> bool {
        let Some(kind) = self.mem else {
            return true;
        };
        let fd = *self.file;
        let Ok(pos) = Sys::lseek(fd, 0, SEEK_CUR) else {
            return false;
        };
        let Ok(end) = Sys::lseek(fd, 0, SEEK_END) else {
            return false;
        };
        let _ = Sys::lseek(fd, pos, SEEK_SET);
        let (pos, end) = (pos as usize, end as usize);
        match kind {
            MemKind::Fixed { buf, size, binary } => {
                if buf.is_null() || self.flags & F_NOWR != 0 {
                    return true;
                }
                let Some(content) = read_all(fd, end.min(size)) else {
                    return false;
                };
                unsafe { buf.copy_from_nonoverlapping(content.as_ptr(), content.len()) };
                if !binary && content.len() < size {
                    unsafe { *buf.add(content.len()) = 0 };
                }
                true
            }
            MemKind::Dynamic { bufp, sizep } => {
                let len = pos.min(end);
                let Some(content) = read_all(fd, len) else {
                    return false;
                };
                let new = unsafe { stdlib::realloc((*bufp).cast(), content.len() + 1) };
                if new.is_null() {
                    return false;
                }
                let new = new.cast::<u8>();
                unsafe {
                    new.copy_from_nonoverlapping(content.as_ptr(), content.len());
                    *new.add(content.len()) = 0;
                    *bufp = new.cast();
                    *sizep = content.len();
                }
                true
            }
            MemKind::Wide { bufp, sizep } => {
                let len = pos.min(end);
                let Some(content) = read_all(fd, len) else {
                    return false;
                };
                let chars = wide_chars(&content);
                let bytes = (chars.len() + 1) * core::mem::size_of::<wchar_t>();
                let new = unsafe { stdlib::realloc((*bufp).cast(), bytes) };
                if new.is_null() {
                    return false;
                }
                let new = new.cast::<wchar_t>();
                unsafe {
                    new.copy_from_nonoverlapping(chars.as_ptr(), chars.len());
                    *new.add(chars.len()) = 0;
                    *bufp = new;
                    *sizep = chars.len();
                }
                true
            }
        }
    }
}

fn open_dynamic(kind: MemKind, wide: bool) -> *mut FILE {
    let result = (|| {
        let fd = anonymous_file()?;
        let mut stream = match helpers::_fdopen(fd, CStr::from_bytes_with_nul(b"w+\0").unwrap()) {
            Ok(stream) => stream,
            Err(e) => {
                let _ = Sys::close(fd);
                return Err(e);
            }
        };
        stream.mem = Some(kind);
        if wide {
            stream.orientation = 1;
        }
        if !stream.sync_memory() {
            let _ = Sys::close(fd);
            stream.file.reference = true;
            return Err(Errno(ENOMEM));
        }
        Ok(stream)
    })();
    result.map(Box::into_raw).or_errno_null_mut()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/open_memstream.html>.
///
/// # Safety
/// `bufp` and `sizep` point to places that stay valid while the stream is open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn open_memstream(bufp: *mut *mut c_char, sizep: *mut size_t) -> *mut FILE {
    if bufp.is_null() || sizep.is_null() {
        platform::ERRNO.set(EINVAL);
        return core::ptr::null_mut();
    }
    unsafe {
        *bufp = core::ptr::null_mut();
        *sizep = 0;
    }
    open_dynamic(MemKind::Dynamic { bufp, sizep }, false)
}

/// `open_wmemstream`, which wchar.h declares.
///
/// # Safety
/// `bufp` and `sizep` point to places that stay valid while the stream is open.
pub(crate) unsafe fn open_wide(bufp: *mut *mut wchar_t, sizep: *mut size_t) -> *mut FILE {
    if bufp.is_null() || sizep.is_null() {
        platform::ERRNO.set(EINVAL);
        return core::ptr::null_mut();
    }
    unsafe {
        *bufp = core::ptr::null_mut();
        *sizep = 0;
    }
    open_dynamic(MemKind::Wide { bufp, sizep }, true)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/fmemopen.html>.
///
/// # Safety
/// `buf` is null or points to `size` bytes that stay valid while the stream is
/// open; `mode` points to a string that ends with a nul.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fmemopen(
    buf: *mut c_void,
    size: size_t,
    mode: *const c_char,
) -> *mut FILE {
    let mode = unsafe { CStr::from_ptr(mode) };
    let first = mode.first();
    let update = mode.contains(b'+');
    if size == 0 || !matches!(first, b'r' | b'w' | b'a') || (buf.is_null() && !update) {
        platform::ERRNO.set(EINVAL);
        return core::ptr::null_mut();
    }
    let buf = buf.cast::<u8>();
    let result = (|| {
        let fd = anonymous_file()?;
        // What the stream holds when it is opened.
        let content: &[u8] = if buf.is_null() || first == b'w' {
            &[]
        } else {
            let all = unsafe { core::slice::from_raw_parts(buf, size) };
            if first == b'a' {
                let used = all.iter().position(|&b| b == 0).unwrap_or(size);
                &all[..used]
            } else {
                all
            }
        };
        if !write_all(fd, content) {
            let errno = Errno(platform::ERRNO.get());
            let _ = Sys::close(fd);
            return Err(errno);
        }
        if first == b'w' && !buf.is_null() && !mode.contains(b'b') {
            unsafe { *buf = 0 };
        }
        let mut stream = match helpers::_fdopen(fd, mode) {
            Ok(stream) => stream,
            Err(e) => {
                let _ = Sys::close(fd);
                return Err(e);
            }
        };
        let _ = Sys::lseek(
            fd,
            if first == b'a' { content.len() as _ } else { 0 },
            SEEK_SET,
        );
        stream.mem = Some(MemKind::Fixed {
            buf,
            size,
            binary: mode.contains(b'b'),
        });
        Ok(stream)
    })();
    result.map(Box::into_raw).or_errno_null_mut()
}
