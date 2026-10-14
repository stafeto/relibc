//! `iconv.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/iconv.h.html>.
//!
//! The code sets are UTF-8, UTF-16 and UTF-32 (little endian, big endian and
//! with a byte order mark), US-ASCII and ISO-8859-1. A name may carry the
//! suffixes `//TRANSLIT` and `//IGNORE`; they are accepted and have no
//! effect.

use alloc::boxed::Box;

use crate::{
    c_str::CStr,
    header::errno::{E2BIG, EILSEQ, EINVAL},
    platform::{
        self,
        types::{c_char, c_int, c_void, size_t},
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Code {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// UTF-16 with a byte order mark; written little endian after a mark.
    Utf16,
    Utf32Le,
    Utf32Be,
    Utf32,
    Ascii,
    Latin1,
}

struct Conversion {
    from: Code,
    to: Code,
    /// For `Utf16` and `Utf32` as the source: the order found in the mark.
    big_endian: Option<bool>,
    /// For `Utf16` and `Utf32` as the target: the mark is written.
    mark_written: bool,
}

fn code_of(name: &[u8]) -> Option<Code> {
    let name = core::str::from_utf8(name).ok()?;
    let name = name.split("//").next().unwrap_or("");
    let upper: alloc::string::String = name.chars().map(|c| c.to_ascii_uppercase()).collect();
    Some(match upper.as_str() {
        "UTF-8" | "UTF8" => Code::Utf8,
        "UTF-16LE" | "UTF16LE" | "UCS-2LE" => Code::Utf16Le,
        "UTF-16BE" | "UTF16BE" | "UCS-2BE" => Code::Utf16Be,
        "UTF-16" | "UTF16" | "UCS-2" => Code::Utf16,
        "UTF-32LE" | "UTF32LE" | "UCS-4LE" => Code::Utf32Le,
        "UTF-32BE" | "UTF32BE" | "UCS-4BE" => Code::Utf32Be,
        "UTF-32" | "UTF32" | "UCS-4" | "WCHAR_T" => Code::Utf32,
        "ASCII" | "US-ASCII" | "ANSI_X3.4-1968" | "646" => Code::Ascii,
        "ISO-8859-1" | "ISO8859-1" | "ISO_8859-1" | "LATIN1" | "L1" => Code::Latin1,
        _ => return None,
    })
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/iconv_open.html>.
///
/// # Safety
/// The names end with a null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iconv_open(tocode: *const c_char, fromcode: *const c_char) -> *mut c_void {
    let to = code_of(unsafe { CStr::from_ptr(tocode) }.to_bytes());
    let from = code_of(unsafe { CStr::from_ptr(fromcode) }.to_bytes());
    let (Some(to), Some(from)) = (to, from) else {
        platform::ERRNO.set(EINVAL);
        return usize::MAX as *mut c_void;
    };
    Box::into_raw(Box::new(Conversion {
        from,
        to,
        big_endian: None,
        mark_written: false,
    }))
    .cast()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/iconv_close.html>.
///
/// # Safety
/// `cd` comes from `iconv_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iconv_close(cd: *mut c_void) -> c_int {
    if cd.is_null() || cd as usize == usize::MAX {
        platform::ERRNO.set(crate::header::errno::EBADF);
        return -1;
    }
    drop(unsafe { Box::from_raw(cd.cast::<Conversion>()) });
    0
}

enum Read {
    Char(u32, usize),
    Incomplete,
    Invalid,
}

fn unit16(bytes: &[u8], big: bool) -> u16 {
    if big {
        u16::from_be_bytes([bytes[0], bytes[1]])
    } else {
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
}

fn unit32(bytes: &[u8], big: bool) -> u32 {
    if big {
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    } else {
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

fn decode(conv: &mut Conversion, input: &[u8]) -> Read {
    match conv.from {
        Code::Utf8 => {
            let Some(&lead) = input.first() else {
                return Read::Incomplete;
            };
            let len = match lead {
                0x00..=0x7f => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Read::Invalid,
            };
            if input.len() < len {
                // A prefix that is wrong at once is no cut-off sequence.
                return if core::str::from_utf8(input)
                    .err()
                    .is_some_and(|e| e.error_len().is_some())
                {
                    Read::Invalid
                } else {
                    Read::Incomplete
                };
            }
            match core::str::from_utf8(&input[..len]) {
                Ok(text) => Read::Char(text.chars().next().map_or(0, |c| c as u32), len),
                Err(_) => Read::Invalid,
            }
        }
        Code::Utf16Le | Code::Utf16Be | Code::Utf16 => {
            let mut skip = 0;
            let big = match conv.from {
                Code::Utf16Be => true,
                Code::Utf16Le => false,
                _ => {
                    if conv.big_endian.is_none() {
                        if input.len() < 2 {
                            return Read::Incomplete;
                        }
                        match [input[0], input[1]] {
                            [0xfe, 0xff] => {
                                conv.big_endian = Some(true);
                                skip = 2;
                            }
                            [0xff, 0xfe] => {
                                conv.big_endian = Some(false);
                                skip = 2;
                            }
                            _ => conv.big_endian = Some(false),
                        }
                    }
                    conv.big_endian.unwrap_or(false)
                }
            };
            let rest = &input[skip..];
            if rest.len() < 2 {
                return Read::Incomplete;
            }
            let first = unit16(rest, big);
            match first {
                0xd800..=0xdbff => {
                    if rest.len() < 4 {
                        return Read::Incomplete;
                    }
                    let second = unit16(&rest[2..], big);
                    if !(0xdc00..0xe000).contains(&second) {
                        return Read::Invalid;
                    }
                    let c = 0x1_0000
                        + ((u32::from(first) - 0xd800) << 10)
                        + (u32::from(second) - 0xdc00);
                    Read::Char(c, skip + 4)
                }
                0xdc00..=0xdfff => Read::Invalid,
                _ => Read::Char(u32::from(first), skip + 2),
            }
        }
        Code::Utf32Le | Code::Utf32Be | Code::Utf32 => {
            let mut skip = 0;
            let big = match conv.from {
                Code::Utf32Be => true,
                Code::Utf32Le => false,
                _ => {
                    if conv.big_endian.is_none() {
                        if input.len() < 4 {
                            return Read::Incomplete;
                        }
                        match [input[0], input[1], input[2], input[3]] {
                            [0, 0, 0xfe, 0xff] => {
                                conv.big_endian = Some(true);
                                skip = 4;
                            }
                            [0xff, 0xfe, 0, 0] => {
                                conv.big_endian = Some(false);
                                skip = 4;
                            }
                            _ => conv.big_endian = Some(false),
                        }
                    }
                    conv.big_endian.unwrap_or(false)
                }
            };
            let rest = &input[skip..];
            if rest.len() < 4 {
                return Read::Incomplete;
            }
            let c = unit32(rest, big);
            if char::from_u32(c).is_none() {
                Read::Invalid
            } else {
                Read::Char(c, skip + 4)
            }
        }
        Code::Ascii => match input.first() {
            None => Read::Incomplete,
            Some(&b) if b < 0x80 => Read::Char(u32::from(b), 1),
            Some(_) => Read::Invalid,
        },
        Code::Latin1 => match input.first() {
            None => Read::Incomplete,
            Some(&b) => Read::Char(u32::from(b), 1),
        },
    }
}

/// The bytes of `c` in the target code set, or None when it has none.
fn encode(conv: &mut Conversion, c: u32, out: &mut [u8; 8]) -> Option<usize> {
    let mut put = |bytes: &[u8], at: usize| {
        out[at..at + bytes.len()].copy_from_slice(bytes);
        at + bytes.len()
    };
    match conv.to {
        Code::Utf8 => {
            let ch = char::from_u32(c)?;
            let mut buf = [0u8; 4];
            let text = ch.encode_utf8(&mut buf);
            Some(put(text.as_bytes(), 0))
        }
        Code::Utf16Le | Code::Utf16Be | Code::Utf16 => {
            let big = conv.to == Code::Utf16Be;
            let mut at = 0;
            if conv.to == Code::Utf16 && !conv.mark_written {
                at = put(&[0xff, 0xfe], 0);
                conv.mark_written = true;
            }
            let ch = char::from_u32(c)?;
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units).iter() {
                let bytes = if big {
                    unit.to_be_bytes()
                } else {
                    unit.to_le_bytes()
                };
                at = put(&bytes, at);
            }
            Some(at)
        }
        Code::Utf32Le | Code::Utf32Be | Code::Utf32 => {
            char::from_u32(c)?;
            let big = conv.to == Code::Utf32Be;
            let bytes = if big {
                c.to_be_bytes()
            } else {
                c.to_le_bytes()
            };
            Some(put(&bytes, 0))
        }
        Code::Ascii => (c < 0x80).then(|| put(&[c as u8], 0)),
        Code::Latin1 => (c < 0x100).then(|| put(&[c as u8], 0)),
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/iconv.html>.
///
/// # Safety
/// The pointers are those of the function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn iconv(
    cd: *mut c_void,
    inbuf: *mut *mut c_char,
    inbytesleft: *mut size_t,
    outbuf: *mut *mut c_char,
    outbytesleft: *mut size_t,
) -> size_t {
    if cd.is_null() || cd as usize == usize::MAX {
        platform::ERRNO.set(crate::header::errno::EBADF);
        return usize::MAX;
    }
    let conv = unsafe { &mut *cd.cast::<Conversion>() };
    // A call without input puts the state back to the initial one.
    if inbuf.is_null() || unsafe { *inbuf }.is_null() {
        conv.big_endian = None;
        conv.mark_written = false;
        return 0;
    }
    loop {
        let left = unsafe { *inbytesleft };
        if left == 0 {
            return 0;
        }
        let input = unsafe { core::slice::from_raw_parts((*inbuf).cast::<u8>(), left) };
        let (c, used) = match decode(conv, input) {
            Read::Char(c, used) => (c, used),
            Read::Incomplete => {
                platform::ERRNO.set(EINVAL);
                return usize::MAX;
            }
            Read::Invalid => {
                platform::ERRNO.set(EILSEQ);
                return usize::MAX;
            }
        };
        let was_marked = conv.mark_written;
        let mut bytes = [0u8; 8];
        let Some(len) = encode(conv, c, &mut bytes) else {
            platform::ERRNO.set(EILSEQ);
            return usize::MAX;
        };
        if len > unsafe { *outbytesleft } {
            conv.mark_written = was_marked;
            platform::ERRNO.set(E2BIG);
            return usize::MAX;
        }
        unsafe {
            (*outbuf)
                .cast::<u8>()
                .copy_from_nonoverlapping(bytes.as_ptr(), len);
            *outbuf = (*outbuf).add(len);
            *outbytesleft -= len;
            *inbuf = (*inbuf).add(used);
            *inbytesleft -= used;
        }
    }
}
