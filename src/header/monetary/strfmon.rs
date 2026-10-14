use crate::{
    header::errno::{E2BIG, EINVAL},
    platform::{
        self,
        types::{c_char, size_t, ssize_t},
    },
};

use alloc::{string::String, vec::Vec};
use core::{ffi::CStr, slice};

/// The conversion specification of one `%` in the format of `strfmon`.
struct Spec {
    fill: char,
    grouping: bool,
    force_sign: bool,
    parens: bool,
    suppress_symbol: bool,
    left_justify: bool,
    width: usize,
    left_precision: Option<usize>,
    right_precision: Option<usize>,
}

fn number(chars: &mut core::iter::Peekable<core::str::Chars<'_>>) -> Option<usize> {
    let mut value: Option<usize> = None;
    while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
        value = Some(
            value
                .unwrap_or(0)
                .checked_mul(10)?
                .checked_add(d as usize)?,
        );
        chars.next();
    }
    value
}

/// One amount in the POSIX locale: the currency symbol and the radix
/// character are empty, no grouping is done, the sign is "-" and the number of
/// fractional digits is 2 unless the specification gives one.
fn amount(spec: &Spec, value: f64) -> Option<String> {
    let negative = value.is_sign_negative() && value != 0.0;
    let digits = spec.right_precision.unwrap_or(2);
    if digits > 18 || !value.is_finite() {
        return None;
    }
    let scale = libm::pow(10.0, digits as f64);
    let scaled = libm::round(libm::fabs(value) * scale);
    if scaled >= 1.0e30 {
        return None;
    }
    let all = alloc::format!("{:0>width$}", scaled as u128, width = digits + 1);
    let (whole, fraction) = all.split_at(all.len() - digits);
    let mut body = String::new();
    if let Some(left) = spec.left_precision {
        for _ in whole.len()..left {
            body.push(spec.fill);
        }
    }
    body.push_str(whole);
    body.push_str(fraction);
    // `grouping`, `suppress_symbol` and `force_sign` have nothing to show here:
    // the separator, the symbol and the positive sign of the locale are empty.
    let _ = (spec.grouping, spec.suppress_symbol, spec.force_sign);
    Some(if negative {
        if spec.parens {
            alloc::format!("({body})")
        } else {
            alloc::format!("-{body}")
        }
    } else {
        body
    })
}

unsafe fn strfmon_va(
    s: *mut c_char,
    maxsize: size_t,
    format: *const c_char,
    args: &mut core::ffi::VaList<'_>,
) -> ssize_t {
    let Ok(format) = unsafe { CStr::from_ptr(format) }.to_str() else {
        platform::ERRNO.set(EINVAL);
        return -1;
    };
    let mut out: Vec<u8> = Vec::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.push(b'%');
            continue;
        }
        let mut spec = Spec {
            fill: ' ',
            grouping: true,
            force_sign: false,
            parens: false,
            suppress_symbol: false,
            left_justify: false,
            width: 0,
            left_precision: None,
            right_precision: None,
        };
        while let Some(&flag) = chars.peek() {
            match flag {
                '=' => {
                    chars.next();
                    let Some(fill) = chars.peek().copied() else {
                        platform::ERRNO.set(EINVAL);
                        return -1;
                    };
                    spec.fill = fill;
                }
                '^' => spec.grouping = false,
                '+' => spec.force_sign = true,
                '(' => spec.parens = true,
                '!' => spec.suppress_symbol = true,
                '-' => spec.left_justify = true,
                _ => break,
            }
            chars.next();
        }
        spec.width = number(&mut chars).unwrap_or(0);
        if chars.peek() == Some(&'#') {
            chars.next();
            spec.left_precision = number(&mut chars);
        }
        if chars.peek() == Some(&'.') {
            chars.next();
            spec.right_precision = number(&mut chars);
        }
        match chars.next() {
            Some('i' | 'n') => {
                let value = unsafe { args.next_arg::<f64>() };
                let Some(mut text) = amount(&spec, value) else {
                    platform::ERRNO.set(E2BIG);
                    return -1;
                };
                while text.len() < spec.width {
                    if spec.left_justify {
                        text.push(' ');
                    } else {
                        text.insert(0, ' ');
                    }
                }
                out.extend_from_slice(text.as_bytes());
            }
            _ => {
                platform::ERRNO.set(EINVAL);
                return -1;
            }
        }
    }
    if out.len() >= maxsize {
        platform::ERRNO.set(E2BIG);
        return -1;
    }
    let buffer = unsafe { slice::from_raw_parts_mut(s.cast::<u8>(), maxsize) };
    buffer[..out.len()].copy_from_slice(&out);
    buffer[out.len()] = 0;
    ssize_t::try_from(out.len()).unwrap_or(-1)
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strfmon.html>.
///
/// # Safety
/// `s` holds `maxsize` bytes; `format` ends with a null; the arguments are doubles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strfmon(
    s: *mut c_char,
    maxsize: size_t,
    format: *const c_char,
    mut args: ...
) -> ssize_t {
    unsafe { strfmon_va(s, maxsize, format, &mut args) }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/strfmon_l.html>.
///
/// # Safety
/// As for `strfmon`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strfmon_l(
    s: *mut c_char,
    maxsize: size_t,
    _locale: crate::header::bits_locale_t::locale_t,
    format: *const c_char,
    mut args: ...
) -> ssize_t {
    unsafe { strfmon_va(s, maxsize, format, &mut args) }
}
