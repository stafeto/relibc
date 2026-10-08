//! `setjmp.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/setjmp.h.html>.

use core::arch::global_asm;

use crate::platform::types::{c_int, c_ulonglong};

macro_rules! platform_specific {
    ($($rust_arch:expr,$c_arch:expr,$ext:expr;)+) => {
        $(
            #[cfg(target_arch = $rust_arch)]
            global_asm!(include_str!(concat!("impl/", $c_arch, "/setjmp.", $ext)));
            #[cfg(target_arch = $rust_arch)]
            global_asm!(include_str!(concat!("impl/", $c_arch, "/sigsetjmp.", $ext)));
        )+
    }
}

macro_rules! longjmp_specific {
    ($($rust_arch:expr,$c_arch:expr,$ext:expr;)+) => {
        $(
            #[cfg(all(target_arch = $rust_arch, not(all(stafeto, target_arch = "aarch64"))))]
            global_asm!(include_str!(concat!("impl/", $c_arch, "/longjmp.", $ext)));
        )+
    }
}

platform_specific! {
    "aarch64","aarch64", "s";
    "x86","i386","s";
    "x86_64","x86_64","s";
    "riscv64", "riscv64", "S";
}

longjmp_specific! {
    "aarch64","aarch64", "s";
    "x86","i386","s";
    "x86_64","x86_64","s";
    "riscv64", "riscv64", "S";
}

/// The entry record of a thread on stafeto (the kernel's message buffer
/// of the thread, whose address TPIDRRO_EL0 holds): its offset and the
/// offset of the word `outer` in it. The long jump clears that word when it
/// leaves a live resident call of the entry distributor of rt. The numbers
/// are those of abi::msgbuf in the stafeto repository; the layer asks for them
/// at the start of a process (`relibc_stafeto_entries_layout_v1`) and ends
/// the process when they differ.
#[cfg(all(stafeto, target_arch = "aarch64"))]
mod entries {
    pub const BASE: usize = 1936;
    pub const OUTER: usize = BASE + 24;
}

#[cfg(all(stafeto, target_arch = "aarch64"))]
global_asm!(
    include_str!("impl/aarch64/longjmp_stafeto.s"),
    outer = const entries::OUTER,
);

/// Writes the offset of the entry record and the offset of its word
/// `outer` to `out[0]` and `out[1]`; the layer compares them with its own.
/// # Safety
/// `out` points to two writable usize words.
#[cfg(all(stafeto, target_arch = "aarch64"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn relibc_stafeto_entries_layout_v1(out: *mut usize) {
    unsafe {
        out.write(entries::BASE);
        out.add(1).write(entries::OUTER);
    }
}

//Each platform has different sizes for sigjmp_buf, currently only x86_64 is supported
unsafe extern "C" {
    /// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/setjmp.html>.
    pub unsafe fn setjmp(env: *mut c_ulonglong) -> c_int;
    /// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/sigsetjmp.html>.
    pub unsafe fn sigsetjmp(env: *mut c_ulonglong, savemask: c_int) -> c_int;
    /// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/longjmp.html>.
    pub unsafe fn longjmp(env: *mut c_ulonglong, val: c_int);
}

/// See <https://pubs.opengroup.org/onlinepubs/9799919799/functions/siglongjmp.html>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siglongjmp(env: *mut c_ulonglong, val: c_int) {
    unsafe { longjmp(env, val) };
}
