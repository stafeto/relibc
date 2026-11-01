//! `spawn.h` implementation
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/spawn.h.html>.

mod file_actions;
mod spawn_attr;

pub use file_actions::{Action, posix_spawn_file_actions_t};
pub use spawn_attr::{Flags, posix_spawnattr_t};

use crate::{
    c_str::CStr,
    header::{
        errno,
        stdlib::getenv,
        unistd::path::{PathSearchIter, SHELL, shell_argv},
    },
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
        let status = unsafe { posix_spawn(pid, file, file_actions, attrp, argv, envp) };
        if cfg!(stafeto) && status == errno::ENOEXEC && !argv.is_null() {
            // A file that is no program runs with the shell, as for a name
            // without a slash.
            let list = unsafe { shell_argv(file, argv) };
            let shell = unsafe { NulTerminated::new(list.as_ptr()) }.unwrap();
            let environment = unsafe { NulTerminated::new(envp) };
            return match unsafe {
                platform::Sys::spawn(
                    CStr::from_ptr(SHELL.as_ptr()),
                    file_actions.as_ref(),
                    attrp.as_ref(),
                    shell,
                    environment,
                )
            } {
                Ok(v) => {
                    if let Some(pid) = unsafe { pid.as_mut() } {
                        *pid = v;
                    }
                    0
                }
                Err(second) if second.0 == errno::ENOENT => status,
                Err(second) => second.0,
            };
        }
        return status;
    }
    let (argv_ptr, envp_ptr) = (argv, envp);
    let argv = {
        if argv.is_null() || unsafe { (*argv).is_null() } {
            return errno::EINVAL;
        }

        unsafe { NulTerminated::new(argv).unwrap() }
    };
    let envp = unsafe { NulTerminated::new(envp) };
    let path_env = unsafe { getenv(c"PATH".as_ptr()) };
    // An environment with no PATH searches the path confstr(_CS_PATH) gives.
    let path_env = if path_env.is_null() && cfg!(stafeto) {
        c"/bin:/usr/bin".as_ptr().cast_mut()
    } else {
        path_env
    };
    let path_env = (!path_env.is_null()).then(|| unsafe { CStr::from_ptr(path_env) });

    let argv_list = argv_ptr;
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
        Err(e) if cfg!(stafeto) && e.0 == errno::ENOEXEC => {
            // The file found on the path is no program: the shell runs it
            // (XSH posix_spawnp, "as if" execvp).
            let Some(path_env) = path_env else { return e.0 };
            for candidate in PathSearchIter::new(program.to_bytes(), &path_env) {
                // SAFETY: the iterator ended the buffer with a NUL.
                let found = unsafe { CStr::from_bytes_with_nul_unchecked(candidate.as_slice()) };
                // The first file of the path that exists as a regular file with an
                // execute bit: the one the layer stopped at (it looks with the
                // effective IDs, as `stat` does; `access` would use the real ones).
                let mut info = crate::header::sys_stat::stat::default();
                if platform::Sys::stat(found, crate::out::Out::from_mut(&mut info)).is_err()
                    || info.st_mode & crate::header::sys_stat::S_IFMT
                        != crate::header::sys_stat::S_IFREG
                    || info.st_mode & 0o111 == 0
                {
                    continue;
                }
                let list = unsafe { shell_argv(found.as_ptr(), argv_list) };
                let shell = unsafe { NulTerminated::new(list.as_ptr()).unwrap() };
                let envp = unsafe { NulTerminated::new(envp_ptr) };
                return match unsafe {
                    platform::Sys::spawn(
                        CStr::from_ptr(SHELL.as_ptr()),
                        file_actions.as_ref(),
                        attrp.as_ref(),
                        shell,
                        envp,
                    )
                } {
                    Ok(v) => {
                        if let Some(pid) = unsafe { pid.as_mut() } {
                            *pid = v;
                        }
                        0
                    }
                    // No shell in the image: the answer stays ENOEXEC.
                    Err(second) if second.0 == errno::ENOENT => e.0,
                    Err(second) => second.0,
                };
            }
            e.0
        }
        Err(e) => e.0,
    }
}
