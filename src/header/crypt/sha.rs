use alloc::string::{String, ToString};

use sha_crypt::{
    ROUNDS_DEFAULT, ROUNDS_MIN, Sha256Params, Sha512Params, sha256_crypt_b64,
    sha512_crypt_b64,
};

use crate::platform::types::c_ulong;

// key limit is not part of the original design, added for DoS protection.
// rounds limit has been lowered (versus the reference/spec), also for DoS
// protection: above 9,999,999 as in musl the setting is refused. runtime is O(klen^2 + klen*rounds)
const KEY_MAX: usize = 256;
const ROUNDS_LIMIT: c_ulong = 9_999_999;
const SALT_MAX: usize = 16;
const RSTRING: &str = "rounds=";

#[derive(Clone, Copy)]
pub enum ShaType {
    Sha256,
    Sha512,
}

/// Performs SHA hashing on a given password with a specific setting.
///
/// # Parameters
/// * `passw`: The password to be hashed. It must be a string slice (`&str`).
/// * `setting`: The settings for the SHA hashing. It must be a string slice (`&str`)
///   and should start with "$5$" for SHA256 or "$6$" for SHA512. The rest of the string should represent the salt
///   to be used for the SHA hashing.
/// * `cipher`: The type of SHA algorithm to use. It should be either `ShaType::Sha256` or `ShaType::Sha512`.
///
/// # Returns
/// * `Option<String>`: Returns `Some(String)` if the SHA operation was successful, where the
///   returned string is the result of the SHA operation formatted according to the Modular
///   Crypt Format (MCF). If the SHA operation failed, it returns `None`.
///
/// # Errors
/// * If the `passw` length exceeds `KEY_MAX`.
/// * If the `setting` does not start with "$5$" or "$6$".
/// * If the `setting` does not contain a '$' character.
/// * If the `setting` contains another '$' character after the first one.
/// * If the `setting` contains invalid characters.
/// * If the `setting` contains an invalid number of rounds.
/// * If the `sha256_crypt_b64` or `sha512_crypt_b64` function fails to hash the password.
///
/// # Example
/// ```
/// let password = "my_password";
/// let setting = "$5$rounds=1400$anotherlongsaltstringg";
/// let result = crypt_sha(password, setting, ShaType::Sha256);
/// assert!(result.is_some());
/// ```
///
/// # Note
/// The `crypt_sha` function uses the SHA256 or SHA512 hashing algorithm for hashing.
/// The output of the SHA operation is base64-encoded using the BCrypt variant of base64.
pub fn crypt_sha(passw: &[u8], setting: &str, cipher: ShaType) -> Option<String> {
    let mut cursor = 3;
    let rounds;

    /* reject large keys */
    if passw.len() > KEY_MAX {
        return None;
    }

    // SHA256
    // setting: $5$rounds=n$setting$ (rounds=n$ and closing $ are optional)
    // SHA512
    // setting: $6$rounds=n$setting$ (rounds=n$ and closing $ are optional)
    let param = match cipher {
        ShaType::Sha256 => "$5$",
        ShaType::Sha512 => "$6$",
    };

    let setting = setting.as_bytes();

    if setting.get(0..3) != Some(param.as_bytes()) {
        return None;
    }

    let has_round;
    // 7 is len("rounds=")
    if setting.get(cursor..cursor + 7) == Some(RSTRING.as_bytes()) {
        cursor += 7;
        has_round = true;
        {
            let c_end = setting[cursor..].iter().position(|&r| r == b'$')?;
            let digits = core::str::from_utf8(&setting[cursor..cursor + c_end]).ok()?;
            if !digits.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                return None;
            }
            if let Ok(u) = digits.parse::<c_ulong>() {
                if u > ROUNDS_LIMIT {
                    return None;
                }
                cursor += c_end + 1;
                rounds = u.max(ROUNDS_MIN as c_ulong);
            } else {
                return None;
            }
        }
    } else {
        has_round = false;
        rounds = ROUNDS_DEFAULT as c_ulong;
    }

    let mut slen = cursor;

    for i in 0..SALT_MAX.min(setting.len() - cursor) {
        let idx = cursor + i;

        if setting[idx] == b'$' {
            break;
        }

        // reject characters that interfere with /etc/shadow parsing, and
        // bytes outside ASCII
        if setting[idx] == b'\n' || setting[idx] == b':' || !setting[idx].is_ascii() {
            return None;
        }
        slen += 1;
    }

    let setting = &setting[cursor..slen];

    if let Ok(enc) = match cipher {
        ShaType::Sha256 => {
            let params = Sha256Params::new(usize::try_from(rounds).expect("rounds below u32::MAX"))
                .unwrap_or(Sha256Params::new(ROUNDS_DEFAULT).unwrap());
            sha256_crypt_b64(passw, setting, &params)
        }
        ShaType::Sha512 => {
            let params = Sha512Params::new(usize::try_from(rounds).expect("rounds below u32::MAX"))
                .unwrap_or(Sha512Params::new(ROUNDS_DEFAULT).unwrap());
            sha512_crypt_b64(passw, setting, &params)
        }
    } {
        let (r_slice, rn_slice) = if has_round {
            (RSTRING, rounds.to_string() + "$")
        } else {
            ("", String::new())
        };

        // The salt is ASCII: the loop above refuses every other byte.
        let salt = core::str::from_utf8(setting).ok()?;
        Some(format!(
            "{}{}{}{}${}",
            param, r_slice, rn_slice, salt, enc
        ))
    } else {
        None
    }
}
