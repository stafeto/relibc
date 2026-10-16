//! `crypt.h` implementation.
//!
//! Non-POSIX, see <https://www.man7.org/linux/man-pages/man3/crypt.3.html>.

use ::scrypt::password_hash::{Salt, SaltString};
use alloc::string::{String, ToString};
use core::ptr;
use rand::{Rng, SeedableRng, rngs::SmallRng};

use crate::{
    c_str::CStr,
    header::{errno::EINVAL, stdlib::rand},
    platform::{
        self,
        types::{c_char, c_int},
    },
};

mod argon2;
mod blowfish;
mod md5;
mod pbkdf2;
mod scrypt;
mod sha;

use self::{
    argon2::crypt_argon2,
    blowfish::crypt_blowfish,
    md5::crypt_md5,
    pbkdf2::crypt_pbkdf2,
    scrypt::crypt_scrypt,
    sha::{
        ShaType::{Sha256, Sha512},
        crypt_sha,
    },
};

unsafe extern "C" {
    /// Traditional DES crypt of musl (src/c/crypt_des.c); "*" or "x" on a bad setting.
    fn __crypt_des(key: *const c_char, setting: *const c_char, output: *mut c_char) -> *mut c_char;
}

/// See <https://www.man7.org/linux/man-pages/man3/crypt.3.html>.
#[repr(C)]
pub struct crypt_data {
    initialized: c_int,
    buff: [c_char; 256],
}

impl crypt_data {
    #[expect(clippy::new_without_default)]
    pub const fn new() -> Self {
        crypt_data {
            initialized: 1,
            buff: [0; 256],
        }
    }
}

fn gen_salt() -> Option<String> {
    let mut rng = SmallRng::seed_from_u64(unsafe { u64::from(rand().cast_unsigned()) });
    let mut bytes = [0u8; Salt::RECOMMENDED_LENGTH];
    rng.fill_bytes(&mut bytes);
    Some(SaltString::encode_b64(&bytes).ok()?.as_str().to_string())
}

/// See <https://www.man7.org/linux/man-pages/man3/crypt.3.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crypt_r(
    key: *const c_char,
    setting: *const c_char,
    data: *mut crypt_data,
) -> *mut c_char {
    if unsafe { (*data).initialized } == 0 {
        unsafe { *data = crypt_data::new() };
    }

    let key_ptr = key;
    let setting_ptr = setting;
    let key = unsafe { CStr::from_ptr(key) }.to_bytes();
    let Ok(setting) = unsafe { CStr::from_ptr(setting) }.to_str() else {
        platform::ERRNO.set(EINVAL);
        return ptr::null_mut();
    };

    let encoded = if setting.starts_with('$') {
        if setting.starts_with("$1$") {
            crypt_md5(key, setting)
        } else if setting.starts_with("$2") && setting.as_bytes().get(3) == Some(&b'$') {
            crypt_blowfish(key, setting)
        } else if setting.starts_with("$5$") {
            crypt_sha(key, setting, Sha256)
        } else if setting.starts_with("$6$") {
            crypt_sha(key, setting, Sha512)
        } else if setting.starts_with("$7$") {
            crypt_scrypt(key, setting)
        } else if setting.starts_with("$8$") {
            crypt_pbkdf2(key, setting)
        } else if setting.starts_with("$argon2") {
            crypt_argon2(key, setting)
        } else {
            platform::ERRNO.set(EINVAL);
            return ptr::null_mut();
        }
    } else {
        // The Traditional DES of src/c/crypt_des.c: a setting of two characters
        // of the alphabet ./0-9A-Za-z, or an extended one that starts with '_'.
        let mut out = [0 as c_char; 21];
        let hashed = unsafe { __crypt_des(key_ptr, setting_ptr, out.as_mut_ptr()) };
        let text = unsafe { CStr::from_ptr(hashed) }.to_bytes();
        if text.first() == Some(&b'*') || text == b"x" {
            // The failure token of musl: a setting no table takes.
            platform::ERRNO.set(EINVAL);
            return ptr::null_mut();
        }
        core::str::from_utf8(text).ok().map(ToString::to_string)
    };

    if let Some(inner) = encoded {
        let bytes = inner.as_bytes();
        // The result and its NUL fit the buffer of the data.
        if bytes.len() + 1 > unsafe { (*data).buff.len() } {
            platform::ERRNO.set(EINVAL);
            return ptr::null_mut();
        }
        unsafe {
            let dst = (*data).buff.as_mut_ptr();
            ptr::copy_nonoverlapping(bytes.as_ptr().cast(), dst, bytes.len());
            *dst.add(bytes.len()) = 0;
            dst
        }
    } else {
        ptr::null_mut()
    }
}
