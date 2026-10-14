//! `uchar.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/uchar.h.html>.
//!
//! The multibyte encoding is UTF-8; `char16_t` is UTF-16 and `char32_t` is
//! UTF-32.

use crate::{
    header::{errno::EILSEQ, wchar::mbstate_t},
    platform::{
        self,
        types::{c_char, size_t},
    },
};

const ERROR: size_t = usize::MAX;
const INCOMPLETE: size_t = usize::MAX - 1;
const PREVIOUS: size_t = usize::MAX - 2;

fn state_of<'a>(ps: *mut mbstate_t) -> &'a mut mbstate_t {
    static mut INTERNAL: mbstate_t = mbstate_t::new();
    if ps.is_null() {
        unsafe { &mut *(&raw mut INTERNAL) }
    } else {
        unsafe { &mut *ps }
    }
}

/// The length of a UTF-8 sequence from its first byte.
fn sequence_length(lead: u8) -> Option<usize> {
    match lead {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

/// Decodes a code point from the bytes of `s` after those the state holds.
/// The result is the code point and the number of bytes of `s` used, or
/// `Err(INCOMPLETE)` and `Err(ERROR)`.
unsafe fn decode(
    state: &mut mbstate_t,
    s: *const c_char,
    n: size_t,
) -> Result<(u32, size_t), size_t> {
    let mut bytes = [0u8; 4];
    let held = (state.__count_and_surrogate & 0xff) as usize;
    for (i, byte) in bytes.iter_mut().enumerate().take(held) {
        *byte = (state.__bytes >> (8 * i)) as u8;
    }
    let mut have = held;
    let mut used = 0;
    loop {
        if have == 0 || have < sequence_length(bytes[0]).unwrap_or(1) {
            if used == n {
                // The bytes so far wait in the state.
                let mut packed = 0u32;
                for (i, byte) in bytes.iter().enumerate().take(have) {
                    packed |= u32::from(*byte) << (8 * i);
                }
                state.__bytes = packed;
                state.__count_and_surrogate =
                    (state.__count_and_surrogate & 0xffff_0000) | have as u32;
                return Err(INCOMPLETE);
            }
            bytes[have] = unsafe { *s.add(used) } as u8;
            have += 1;
            used += 1;
            if have > 1 && bytes[have - 1] & 0xc0 != 0x80 {
                platform::ERRNO.set(EILSEQ);
                state.__bytes = 0;
                state.__count_and_surrogate &= 0xffff_0000;
                return Err(ERROR);
            }
            continue;
        }
        break;
    }
    state.__bytes = 0;
    state.__count_and_surrogate &= 0xffff_0000;
    let Some(len) = sequence_length(bytes[0]) else {
        platform::ERRNO.set(EILSEQ);
        return Err(ERROR);
    };
    match core::str::from_utf8(&bytes[..len]) {
        Ok(text) => Ok((text.chars().next().map_or(0, |c| c as u32), used)),
        Err(_) => {
            platform::ERRNO.set(EILSEQ);
            Err(ERROR)
        }
    }
}

fn encode(c: u32, out: *mut c_char) -> size_t {
    let Some(ch) = char::from_u32(c) else {
        platform::ERRNO.set(EILSEQ);
        return ERROR;
    };
    let mut buf = [0u8; 4];
    let text = ch.encode_utf8(&mut buf);
    unsafe { out.copy_from_nonoverlapping(text.as_ptr().cast(), text.len()) };
    text.len()
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mbrtoc32.html>.
///
/// # Safety
/// The pointers are those of the function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbrtoc32(
    pc32: *mut u32,
    s: *const c_char,
    n: size_t,
    ps: *mut mbstate_t,
) -> size_t {
    let state = state_of(ps);
    let (s, n) = if s.is_null() {
        *state = mbstate_t::new();
        (c"".as_ptr(), 1)
    } else {
        (s, n)
    };
    match unsafe { decode(state, s, n) } {
        Ok((c, used)) => {
            if !pc32.is_null() {
                unsafe { *pc32 = c };
            }
            if c == 0 { 0 } else { used }
        }
        Err(e) => e,
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/c32rtomb.html>.
///
/// # Safety
/// `s` is null or holds `MB_CUR_MAX` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c32rtomb(s: *mut c_char, c32: u32, ps: *mut mbstate_t) -> size_t {
    let state = state_of(ps);
    if s.is_null() {
        *state = mbstate_t::new();
        return 1;
    }
    encode(c32, s)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/mbrtoc16.html>.
///
/// # Safety
/// The pointers are those of the function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbrtoc16(
    pc16: *mut u16,
    s: *const c_char,
    n: size_t,
    ps: *mut mbstate_t,
) -> size_t {
    let state = state_of(ps);
    let pending = (state.__count_and_surrogate >> 16) as u16;
    if pending != 0 {
        state.__count_and_surrogate &= 0xffff;
        if !pc16.is_null() {
            unsafe { *pc16 = pending };
        }
        return PREVIOUS;
    }
    let (s, n) = if s.is_null() {
        *state = mbstate_t::new();
        (c"".as_ptr(), 1)
    } else {
        (s, n)
    };
    match unsafe { decode(state, s, n) } {
        Ok((c, used)) => {
            if c > 0xffff {
                let c = c - 0x1_0000;
                let high = 0xd800 + (c >> 10) as u16;
                let low = 0xdc00 + (c & 0x3ff) as u16;
                state.__count_and_surrogate =
                    (state.__count_and_surrogate & 0xffff) | (u32::from(low) << 16);
                if !pc16.is_null() {
                    unsafe { *pc16 = high };
                }
            } else if !pc16.is_null() {
                unsafe { *pc16 = c as u16 };
            }
            if c == 0 { 0 } else { used }
        }
        Err(e) => e,
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/c16rtomb.html>.
///
/// # Safety
/// `s` is null or holds `MB_CUR_MAX` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn c16rtomb(s: *mut c_char, c16: u16, ps: *mut mbstate_t) -> size_t {
    let state = state_of(ps);
    if s.is_null() {
        *state = mbstate_t::new();
        return 1;
    }
    let high = (state.__count_and_surrogate >> 16) as u16;
    if high != 0 {
        state.__count_and_surrogate &= 0xffff;
        if !(0xdc00..0xe000).contains(&c16) {
            platform::ERRNO.set(EILSEQ);
            return ERROR;
        }
        let c = 0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(c16) - 0xdc00);
        return encode(c, s);
    }
    if (0xd800..0xdc00).contains(&c16) {
        state.__count_and_surrogate =
            (state.__count_and_surrogate & 0xffff) | (u32::from(c16) << 16);
        return 0;
    }
    encode(u32::from(c16), s)
}
