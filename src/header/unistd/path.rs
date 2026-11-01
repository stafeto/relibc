use core::slice::Split;

use alloc::vec::Vec;

use arrayvec::ArrayVec;

use crate::{
    c_str::CStr,
    header::limits::PATH_MAX,
    platform::types::c_char,
};

pub struct PathSearchIter<'a> {
    file_bytes: &'a [u8],
    path_splits: Split<'a, u8, fn(&u8) -> bool>,
}

const PATH_SEPARATOR: u8 = b':';

impl<'a> PathSearchIter<'a> {
    /// Construct a new PATH parser.
    /// Safety: file must have no slashes
    pub fn new(file_bytes: &'a [u8], path_env: &'a CStr) -> Self {
        Self {
            file_bytes,
            path_splits: path_env.to_bytes().split(|&b| b == PATH_SEPARATOR),
        }
    }
}

impl<'a> Iterator for PathSearchIter<'a> {
    type Item = ArrayVec<u8, PATH_MAX>;

    fn next(&mut self) -> Option<Self::Item> {
        for path in &mut self.path_splits {
            let len = path.len() + self.file_bytes.len() + 2;
            if len > PATH_MAX {
                continue;
            }
            let mut program: ArrayVec<u8, PATH_MAX> = ArrayVec::new();
            program.try_extend_from_slice(path).unwrap();
            program.push(b'/');
            program.try_extend_from_slice(self.file_bytes).unwrap();
            program.push(b'\0');
            return Some(program);
        }

        None
    }
}

/// The argument list of the command interpreter that runs a file the system
/// does not know as a program (ENOEXEC from execvp or posix_spawnp): `sh`,
/// the file, and the arguments after the first of `argv`.
///
/// # Safety
/// `script` is a NUL-terminated path; `argv` is null or a list of C strings
/// ended by a null pointer.
pub(crate) unsafe fn shell_argv(
    script: *const c_char,
    argv: *const *mut c_char,
) -> Vec<*mut c_char> {
    let mut list: Vec<*mut c_char> = Vec::new();
    list.push(c"sh".as_ptr().cast_mut());
    list.push(script.cast_mut());
    if !argv.is_null() && !unsafe { *argv }.is_null() {
        let mut at = 1;
        while !unsafe { *argv.add(at) }.is_null() {
            list.push(unsafe { *argv.add(at) });
            at += 1;
        }
    }
    list.push(core::ptr::null_mut());
    list
}

/// The shell that runs such a file.
pub(crate) const SHELL: &core::ffi::CStr = c"/bin/sh";
