//! `ftw.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/ftw.h.html>.

use alloc::{boxed::Box, ffi::CString, string::String, vec::Vec};
use core::mem;

use crate::{
    c_str::CStr,
    header::{
        dirent::{closedir, opendir, readdir},
        sys_stat::{S_IFDIR, S_IFLNK, S_IFMT, lstat, stat},
    },
    platform::types::{c_char, c_int},
};

// Types of the objects `ftw` and `nftw` visit.
pub const FTW_F: c_int = 0;
pub const FTW_D: c_int = 1;
pub const FTW_DNR: c_int = 2;
pub const FTW_DP: c_int = 3;
pub const FTW_NS: c_int = 4;
pub const FTW_SL: c_int = 5;
pub const FTW_SLN: c_int = 6;

// Flags of `nftw`.
pub const FTW_PHYS: c_int = 1;
pub const FTW_MOUNT: c_int = 2;
pub const FTW_CHDIR: c_int = 4;
pub const FTW_DEPTH: c_int = 8;
pub const FTW_ACTIONRETVAL: c_int = 16;

// What a function may answer to `nftw` with FTW_ACTIONRETVAL.
pub const FTW_CONTINUE: c_int = 0;
pub const FTW_STOP: c_int = 1;
pub const FTW_SKIP_SUBTREE: c_int = 2;
pub const FTW_SKIP_SIBLINGS: c_int = 3;

/// What `nftw` tells its function about the position of the object.
#[repr(C)]
pub struct FTW {
    pub base: c_int,
    pub level: c_int,
}

type NftwFn = extern "C" fn(*const c_char, *const stat, c_int, *mut FTW) -> c_int;

struct Walk {
    function: NftwFn,
    flags: c_int,
}

/// Visits `path` and what is below it. The answer is 0 to go on, FTW_STOP to
/// end the walk at once with `stopped` set to the value to return, or
/// FTW_SKIP_SIBLINGS.
fn visit(walk: &Walk, path: &str, base: usize, level: c_int, stopped: &mut Option<c_int>) -> c_int {
    let Ok(c_path) = CString::new(path) else {
        return FTW_CONTINUE;
    };
    let mut info: stat = unsafe { mem::zeroed() };
    let physical = walk.flags & FTW_PHYS != 0;
    let status = if physical {
        unsafe { lstat(c_path.as_ptr(), &mut info) }
    } else {
        unsafe { stat(c_path.as_ptr(), &mut info) }
    };
    let mut kind = if status != 0 {
        // A link whose target is missing is still a link.
        if !physical && unsafe { lstat(c_path.as_ptr(), &mut info) } == 0 {
            if info.st_mode & S_IFMT == S_IFLNK {
                FTW_SLN
            } else {
                FTW_NS
            }
        } else {
            FTW_NS
        }
    } else if info.st_mode & S_IFMT == S_IFDIR {
        FTW_D
    } else if info.st_mode & S_IFMT == S_IFLNK {
        FTW_SL
    } else {
        FTW_F
    };
    let mut position = FTW {
        base: base as c_int,
        level,
    };
    let call = |kind: c_int, position: &mut FTW, info: &stat| {
        (walk.function)(c_path.as_ptr(), info, kind, position)
    };
    let answer = |value: c_int, stopped: &mut Option<c_int>| -> c_int {
        if walk.flags & FTW_ACTIONRETVAL != 0 {
            match value {
                FTW_CONTINUE => FTW_CONTINUE,
                FTW_SKIP_SUBTREE | FTW_SKIP_SIBLINGS => value,
                _ => {
                    *stopped = Some(value);
                    FTW_STOP
                }
            }
        } else if value != 0 {
            *stopped = Some(value);
            FTW_STOP
        } else {
            FTW_CONTINUE
        }
    };

    if kind != FTW_D {
        let value = call(kind, &mut position, &info);
        return answer(value, stopped);
    }

    let depth_first = walk.flags & FTW_DEPTH != 0;
    if !depth_first {
        let value = call(FTW_D, &mut position, &info);
        let reply = answer(value, stopped);
        if reply != FTW_CONTINUE {
            return if reply == FTW_SKIP_SUBTREE {
                FTW_CONTINUE
            } else {
                reply
            };
        }
    }

    let directory = unsafe { opendir(c_path.as_ptr()) };
    if directory.is_null() {
        kind = FTW_DNR;
        let value = call(kind, &mut position, &info);
        return answer(value, stopped);
    }
    let mut names: Vec<String> = Vec::new();
    loop {
        let entry = unsafe { readdir(&mut *directory) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        if name != "." && name != ".." {
            names.push(name);
        }
    }
    unsafe { closedir(Box::from_raw(directory)) };

    let mut reply = FTW_CONTINUE;
    for name in names {
        let mut child = String::from(path);
        if !child.ends_with('/') {
            child.push('/');
        }
        let child_base = child.len();
        child.push_str(&name);
        reply = visit(walk, &child, child_base, level + 1, stopped);
        if reply == FTW_STOP {
            return FTW_STOP;
        }
        if reply == FTW_SKIP_SIBLINGS {
            break;
        }
    }

    if depth_first {
        let value = call(FTW_DP, &mut position, &info);
        let after = answer(value, stopped);
        if after == FTW_STOP {
            return FTW_STOP;
        }
    }
    if reply == FTW_SKIP_SIBLINGS {
        FTW_SKIP_SIBLINGS
    } else {
        FTW_CONTINUE
    }
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/nftw.html>.
///
/// The descriptor limit is not needed (the walk keeps one directory open at a
/// time); FTW_CHDIR and FTW_MOUNT are not acted on.
///
/// # Safety
/// `path` ends with a null; `fn_` is a function of the type given.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nftw(
    path: *const c_char,
    fn_: Option<extern "C" fn(*const c_char, *const stat, c_int, *mut FTW) -> c_int>,
    _fd_limit: c_int,
    flags: c_int,
) -> c_int {
    let Some(function) = fn_ else {
        return -1;
    };
    let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
        return -1;
    };
    let base = path.rfind('/').map_or(0, |at| at + 1);
    let walk = Walk { function, flags };
    let mut stopped = None;
    visit(&walk, path, base, 0, &mut stopped);
    stopped.unwrap_or(0)
}

type FtwFn = extern "C" fn(*const c_char, *const stat, c_int) -> c_int;

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/ftw.html>.
///
/// Kept for old programs: `nftw` is the interface to use.
///
/// # Safety
/// As for `nftw`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ftw(
    path: *const c_char,
    fn_: Option<extern "C" fn(*const c_char, *const stat, c_int) -> c_int>,
    _ndirs: c_int,
) -> c_int {
    // `ftw` has no position argument; its function is wrapped by a small
    // trampoline kept in a thread-local cell.
    use core::cell::Cell;
    #[thread_local]
    static CURRENT: Cell<Option<FtwFn>> = Cell::new(None);
    extern "C" fn bridge(p: *const c_char, s: *const stat, kind: c_int, _f: *mut FTW) -> c_int {
        match CURRENT.get() {
            Some(function) => function(p, s, kind),
            None => -1,
        }
    }
    let Some(function) = fn_ else {
        return -1;
    };
    let before = CURRENT.replace(Some(function));
    let result = unsafe { nftw(path, Some(bridge), 1, FTW_PHYS) };
    CURRENT.set(before);
    result
}
