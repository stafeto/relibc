//! `wordexp.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/wordexp.h.html>.
//!
//! The words are expanded by `/bin/sh`: it is given a loop over them that
//! prints each result followed by a mark that no word contains.

use alloc::vec::Vec;
use core::ptr;

use crate::{
    header::{
        stdio::{fread, pclose, popen},
        stdlib::{free, malloc, realloc},
    },
    platform::types::{c_char, c_int, c_void, size_t},
};

pub const WRDE_DOOFFS: c_int = 1 << 0;
pub const WRDE_APPEND: c_int = 1 << 1;
pub const WRDE_NOCMD: c_int = 1 << 2;
pub const WRDE_REUSE: c_int = 1 << 3;
pub const WRDE_SHOWERR: c_int = 1 << 4;
pub const WRDE_UNDEF: c_int = 1 << 5;

pub const WRDE_NOSPACE: c_int = 1;
pub const WRDE_BADCHAR: c_int = 2;
pub const WRDE_BADVAL: c_int = 3;
pub const WRDE_CMDSUB: c_int = 4;
pub const WRDE_SYNTAX: c_int = 5;

#[repr(C)]
pub struct wordexp_t {
    pub we_wordc: size_t,
    pub we_wordv: *mut *mut c_char,
    pub we_offs: size_t,
}

/// The byte that ends a word in the output of the shell.
const MARK: u8 = 0x01;

/// Looks at the text for the characters that are not allowed outside quotes
/// (and for command substitution, which `WRDE_NOCMD` forbids).
fn check(text: &[u8], no_command: bool) -> Result<(), c_int> {
    let mut i = 0;
    let mut quote = 0u8;
    // Depth of `$(`, `${` and backquote constructs.
    let mut inside = 0usize;
    let mut backquote = false;
    while i < text.len() {
        let c = text[i];
        if quote == b'\'' {
            if c == b'\'' {
                quote = 0;
            }
            i += 1;
            continue;
        }
        match c {
            b'\\' => i += 1,
            b'`' => {
                if no_command {
                    return Err(WRDE_CMDSUB);
                }
                backquote = !backquote;
            }
            b'$' if text.get(i + 1) == Some(&b'(') => {
                if no_command {
                    return Err(WRDE_CMDSUB);
                }
                inside += 1;
                i += 1;
            }
            b'$' if text.get(i + 1) == Some(&b'{') => {
                inside += 1;
                i += 1;
            }
            b'\'' if quote == 0 => quote = b'\'',
            b'"' => quote = if quote == b'"' { 0 } else { b'"' },
            b')' | b'}' if inside > 0 => inside -= 1,
            b'\n' | b'|' | b'&' | b';' | b'<' | b'>' | b'(' | b')' | b'{' | b'}'
                if quote == 0 && inside == 0 && !backquote =>
            {
                return Err(WRDE_BADCHAR);
            }
            _ => {}
        }
        i += 1;
    }
    if quote != 0 || inside != 0 || backquote {
        return Err(WRDE_SYNTAX);
    }
    Ok(())
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/wordexp.html>.
///
/// # Safety
/// `words` ends with a null; `pwordexp` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wordexp(
    words: *const c_char,
    pwordexp: *mut wordexp_t,
    flags: c_int,
) -> c_int {
    let text = unsafe { crate::c_str::CStr::from_ptr(words) }.to_bytes();
    if let Err(code) = check(text, flags & WRDE_NOCMD != 0) {
        return code;
    }
    let we = unsafe { &mut *pwordexp };
    let mut script: Vec<u8> = Vec::new();
    script.extend_from_slice(b"{ ");
    if flags & WRDE_UNDEF != 0 {
        script.extend_from_slice(b"set -u; ");
    }
    script.extend_from_slice(b"for w in ");
    script.extend_from_slice(text);
    script.extend_from_slice(b"; do printf '%s\\n' \"$w\"");
    script.push(MARK);
    script.extend_from_slice(b"; done; }");
    if flags & WRDE_SHOWERR == 0 {
        script.extend_from_slice(b" 2>/dev/null");
    }
    script.push(0);
    let stream = unsafe { popen(script.as_ptr().cast(), c"r".as_ptr()) };
    if stream.is_null() {
        return WRDE_NOSPACE;
    }
    let mut output: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = unsafe { fread(chunk.as_mut_ptr().cast::<c_void>(), 1, chunk.len(), stream) };
        if n == 0 {
            break;
        }
        output.extend_from_slice(&chunk[..n]);
    }
    let status = unsafe { pclose(stream) };
    if status != 0 {
        return if flags & WRDE_UNDEF != 0 {
            WRDE_BADVAL
        } else {
            WRDE_SYNTAX
        };
    }

    // Each word is followed by the mark and a newline.
    let mut found: Vec<&[u8]> = Vec::new();
    let mut rest = output.as_slice();
    while let Some(at) = rest.iter().position(|&b| b == MARK) {
        found.push(&rest[..at]);
        rest = &rest[(at + 2).min(rest.len())..];
    }

    // With WRDE_APPEND the words go after those the vector holds; otherwise
    // the vector starts again, after `we_offs` empty places if asked.
    let append = flags & WRDE_APPEND != 0 && !we.we_wordv.is_null();
    let (old_count, offs) = if append {
        (we.we_wordc, we.we_offs)
    } else if flags & WRDE_DOOFFS != 0 {
        (0, we.we_offs)
    } else {
        (0, 0)
    };
    let base = offs;
    let total = base + old_count + found.len() + 1;
    let vector = if append {
        unsafe {
            realloc(
                we.we_wordv.cast(),
                total * core::mem::size_of::<*mut c_char>(),
            )
        }
    } else {
        unsafe { malloc(total * core::mem::size_of::<*mut c_char>()) }
    }
    .cast::<*mut c_char>();
    if vector.is_null() {
        return WRDE_NOSPACE;
    }
    if !append {
        for i in 0..base {
            unsafe { *vector.add(i) = ptr::null_mut() };
        }
    }
    for (i, word) in found.iter().enumerate() {
        let copy = unsafe { malloc(word.len() + 1) }.cast::<c_char>();
        if copy.is_null() {
            for j in 0..i {
                unsafe { free((*vector.add(base + old_count + j)).cast()) };
            }
            unsafe { free(vector.cast()) };
            we.we_wordv = ptr::null_mut();
            we.we_wordc = 0;
            return WRDE_NOSPACE;
        }
        unsafe {
            copy.copy_from_nonoverlapping(word.as_ptr().cast(), word.len());
            *copy.add(word.len()) = 0;
            *vector.add(base + old_count + i) = copy;
        }
    }
    unsafe { *vector.add(base + old_count + found.len()) = ptr::null_mut() };
    we.we_wordv = vector;
    we.we_wordc = old_count + found.len();
    we.we_offs = offs;
    0
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/wordfree.html>.
///
/// # Safety
/// `pwordexp` was filled by `wordexp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wordfree(pwordexp: *mut wordexp_t) {
    let we = unsafe { &mut *pwordexp };
    if we.we_wordv.is_null() {
        return;
    }
    for i in 0..we.we_wordc {
        unsafe { free((*we.we_wordv.add(we.we_offs + i)).cast()) };
    }
    unsafe { free(we.we_wordv.cast()) };
    we.we_wordv = ptr::null_mut();
    we.we_wordc = 0;
}
