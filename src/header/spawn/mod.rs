//! `spawn.h` implementation
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/spawn.h.html>.

mod file_actions;
mod spawn_attr;

pub use file_actions::{Action, posix_spawn_file_actions_t};
pub use spawn_attr::{Flags, posix_spawnattr_t};

use crate::{
    c_str::CStr,
    header::{errno, stdlib::getenv},
    iter::NulTerminated,
    platform::{
        self, Pal,
        types::{c_char, c_int, pid_t},
    },
};

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_spawn.html>.
///
/// Creates a new process (child process) from the specified process image. The
/// `path` argument is a pathname that identifies the new process image file to
/// execute.
///
/// Returns the process ID of the child process to the parent process, in the
/// variable pointed to by a non-null `pid` argument, and shall return `0` as
/// the function return value. Upon error, an error number shall be returned
/// as the function return value.
///
/// # Panics
/// `argv` must **not** be `NULL` and must contain atleast the program name.
/// `path` must also **not** be `NULL`. Failure to ensure any of this will
/// result in a panic.
///
/// # Safety
/// `file_actions` and `attrp` must either be `NULL` or be pointers to properly
/// initialised objects. Doing otherwise is undefined behaviour.
///
/// `path` and the elements in `argv` must be a pointers to valid
/// null-terminated character arrays. Failure to ensure any of this will result
/// in undefined behaviour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawn(
    pid: *mut pid_t,
    path: *const c_char,
    file_actions: *const posix_spawn_file_actions_t,
    attrp: *const posix_spawnattr_t,
    argv: *const *mut c_char,
    envp: *const *mut c_char,
) -> c_int {
    let argv = {
        if argv.is_null() || unsafe { (*argv).is_null() } {
            return errno::EINVAL;
        }

        unsafe { NulTerminated::new(argv).unwrap() }
    };
    let envp = unsafe { NulTerminated::new(envp) };
    let program = unsafe { CStr::from_ptr(path) };

    match unsafe {
        platform::Sys::spawn(program, file_actions.as_ref(), attrp.as_ref(), argv, envp)
    } {
        Ok(v) => {
            if let Some(pid) = unsafe { pid.as_mut() } {
                *pid = v;
            }
            0
        }
        Err(e) => e.0,
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/posix_spawnp.html>.
///
/// Creates a new process (child process) from the specified process image. The
/// `file` argument is used to construct a pathname that identifies the new
/// process image file.
///
/// Returns the process ID of the child process to the parent process, in the
/// variable pointed to by a non-null `pid` argument, and shall return `0` as
/// the function return value. Upon error, an error number shall be returned
/// as the function return value.
///
/// # Panics
/// `argv` must **not** be `NULL` and must contain atleast the program name.
/// `file` must also **not** be `NULL`. Failure to ensure any of this will
/// result in a panic.
///
/// # Safety
/// `file_actions` and `attrp` must either be `NULL` or be pointers to properly
/// initialised objects. Doing otherwise is undefined behaviour.
///
/// `file` and the elements in `argv` must be a pointers to valid
/// null-terminated character arrays. Failure to ensure any of this will result
/// in undefined behaviour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn posix_spawnp(
    pid: *mut pid_t,
    file: *const c_char,
    file_actions: *const posix_spawn_file_actions_t,
    attrp: *const posix_spawnattr_t,
    argv: *const *mut c_char,
    envp: *const *mut c_char,
) -> c_int {
    let program = unsafe { CStr::from_ptr(file) };
    if program.contains(b'/') {
        return unsafe { posix_spawn(pid, file, file_actions, attrp, argv, envp) };
    }
    let argv = {
        if argv.is_null() || unsafe { (*argv).is_null() } {
            return errno::EINVAL;
        }

        unsafe { NulTerminated::new(argv).unwrap() }
    };
    let envp = unsafe { NulTerminated::new(envp) };
    let path_env = unsafe { getenv(c"PATH".as_ptr()) };
    let path_env = (!path_env.is_null()).then(|| unsafe { CStr::from_ptr(path_env) });

    match unsafe {
        platform::Sys::spawnp(
            program,
            path_env,
            file_actions.as_ref(),
            attrp.as_ref(),
            argv,
            envp,
        )
    } {
        Ok(v) => {
            if let Some(pid) = unsafe { pid.as_mut() } {
                *pid = v;
            }
            0
        }
        Err(e) => e.0,
    }
}
