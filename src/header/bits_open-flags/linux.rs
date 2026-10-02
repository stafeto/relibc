use crate::platform::types::c_int;

/// Open for reading only.
pub const O_RDONLY: c_int = 0x0000;
/// Open for writing only.
pub const O_WRONLY: c_int = 0x0001;
/// Open for reading and writing.
pub const O_RDWR: c_int = 0x0002;
/// Mask for file access modes.
pub const O_ACCMODE: c_int = 0x0003;
/// Create file if it does not exist.
pub const O_CREAT: c_int = 0x0040;
/// Exclusive use flag.
pub const O_EXCL: c_int = 0x0080;
/// Do not assign controlling terminal.
pub const O_NOCTTY: c_int = 0x0100;
/// Truncate flag.
pub const O_TRUNC: c_int = 0x0200;
/// Set append mode.
pub const O_APPEND: c_int = 0x0400;
/// Non-blocking mode.
pub const O_NONBLOCK: c_int = 0x0800;
/// Fail if file is a non-directory file.
#[cfg(not(target_arch = "aarch64"))]
pub const O_DIRECTORY: c_int = 0x1_0000;
/// Do not follow symbolic links.
#[cfg(not(target_arch = "aarch64"))]
pub const O_NOFOLLOW: c_int = 0x2_0000;
/// Fail if file is a non-directory file (AArch64 Linux, asm/fcntl.h).
#[cfg(target_arch = "aarch64")]
pub const O_DIRECTORY: c_int = 0o4_0000;
/// Do not follow symbolic links (AArch64 Linux, asm/fcntl.h).
#[cfg(target_arch = "aarch64")]
pub const O_NOFOLLOW: c_int = 0o10_0000;
/// Atomically set the `FD_CLOEXEC` flag on the new file desciptor.
pub const O_CLOEXEC: c_int = 0x8_0000;
/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man2/open.2.html>.
///
/// Get a file descriptor to indicate a location in the filesystem tree and
/// to perform operations that act purely at the file descriptor level.
pub const O_PATH: c_int = 0x20_0000;

/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man2/open.2.html>.
///
/// Alternative name for `O_NONBLOCK`.
pub const O_NDELAY: c_int = O_NONBLOCK;

// The values the kernel takes: those of the libc crate for this target.
#[cfg(feature = "check_against_libc_crate")]
const _: () = {
    use __libc_only_for_layout_checks as libc;
    assert!(O_ACCMODE == libc::O_ACCMODE);
    assert!(O_CREAT == libc::O_CREAT);
    assert!(O_EXCL == libc::O_EXCL);
    assert!(O_NOCTTY == libc::O_NOCTTY);
    assert!(O_TRUNC == libc::O_TRUNC);
    assert!(O_APPEND == libc::O_APPEND);
    assert!(O_NONBLOCK == libc::O_NONBLOCK);
    assert!(O_DIRECTORY == libc::O_DIRECTORY);
    assert!(O_NOFOLLOW == libc::O_NOFOLLOW);
    assert!(O_CLOEXEC == libc::O_CLOEXEC);
    assert!(O_PATH == libc::O_PATH);
};
