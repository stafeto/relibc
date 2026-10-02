use super::{sigset_t, stack_t};
#[allow(unused_imports)]
use crate::platform::types::{c_longlong, c_uchar, c_uint, c_ulong, c_ulonglong, c_ushort};
use core::arch::global_asm;

// Needs to be defined in assembly because it can't have a function prologue
// rax is register, 15 is RT_SIGRETURN
#[cfg(target_arch = "x86_64")]
global_asm!(
    "
    .global __restore_rt
    __restore_rt:
        mov rax, 15
        syscall
"
);
// x8 is register, 139 is RT_SIGRETURN
#[cfg(target_arch = "aarch64")]
global_asm!(
    "
    .global __restore_rt
    __restore_rt:
        mov x8, #139
        svc 0
"
);

#[cfg(target_arch = "riscv64")]
global_asm!(
    "
    .global __restore_rt
    __restore_rt:
        li a7, 139
        ecall
"
);

/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man7/signal.7.html>.
///
/// IOT trap. A synonym for `SIGABRT`.
pub const SIGIOT: usize = super::constants::SIGABRT;
// TODO mark #[deprecated]?
/// Obsolete in issue 7, removed in issue 8.
///
/// Pollable event.
/// Default action: T
pub const SIGPOLL: usize = super::constants::SIGIO;
/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man7/signal.7.html>.
///
/// Synonymous with `SIGSYS`.
pub const SIGUNUSED: usize = super::constants::SIGSYS;

// Below SA_* constants cannot share the same values as Redox for implementation reasons.
/// Do not generate `SIGCHLD` when children stop or stopped children continue.
pub const SA_NOCLDSTOP: usize = 1;
/// Causes extra information to be passed to signal handlers at the time of
/// receipt of a signal.
pub const SA_SIGINFO: usize = 4;
/// Process is executing on an alternate signal stack.
pub const SA_ONSTACK: usize = 0x0800_0000;
/// Causes certain functions to become restartable.
pub const SA_RESTART: usize = 0x1000_0000;
/// Causes signal not to be automatically blocked on entry to signal handler.
pub const SA_NODEFER: usize = 0x4000_0000;
/// Causes signal dispositions to be set to `SIG_DFL` on entry to signal
/// handlers.
pub const SA_RESETHAND: usize = 0x8000_0000;
/// Non-POSIX, see <https://www.man7.org/linux/man-pages/man2/sigaction.2.html>.
///
/// Not intended for application use. Used by C libraries to indicate that the
/// `sa_restorer` field contains the address of a "signal trampoline".
pub const SA_RESTORER: usize = 0x0400_0000;

// Mirrors the ucontext_t struct from the libc crate on Linux.

pub(crate) type ucontext_t = ucontext;
/// A machine-specific representation of the saved context.
pub(crate) type mcontext_t = mcontext;

#[cfg(not(target_arch = "aarch64"))]
#[repr(C)]
pub struct ucontext {
    pub uc_flags: c_ulong,
    /// Pointer to the context that is resumed when this context returns.
    pub uc_link: *mut ucontext_t,
    /// The stack used by this context.
    pub uc_stack: stack_t,
    /// A machine-specific representation of the saved context.
    pub uc_mcontext: mcontext_t,
    /// The set of signals that are blocked when this context is active.
    pub uc_sigmask: sigset_t,
    __private: [c_uchar; 512],
}

/// AArch64 Linux (asm/ucontext.h): the mask, room for a wider one, then
/// the machine context aligned to 16.
#[cfg(target_arch = "aarch64")]
#[repr(C)]
pub struct ucontext {
    pub uc_flags: c_ulong,
    /// Pointer to the context that is resumed when this context returns.
    pub uc_link: *mut ucontext_t,
    /// The stack used by this context.
    pub uc_stack: stack_t,
    /// The set of signals that are blocked when this context is active.
    pub uc_sigmask: sigset_t,
    __unused: [c_uchar; 120],
    /// A machine-specific representation of the saved context.
    pub uc_mcontext: mcontext_t,
}

#[repr(C)]
pub struct _libc_fpstate {
    pub cwd: c_ushort,
    pub swd: c_ushort,
    pub ftw: c_ushort,
    pub fop: c_ushort,
    pub rip: c_ulonglong,
    pub rdp: c_ulonglong,
    pub mxcsr: c_uint,
    pub mxcr_mask: c_uint,
    pub _st: [_libc_fpxreg; 8],
    pub _xmm: [_libc_xmmreg; 16],
    __private: [c_ulonglong; 12],
}
#[repr(C)]
pub struct _libc_fpxreg {
    pub significand: [c_ushort; 4],
    pub exponent: c_ushort,
    __private: [c_ushort; 3],
}

#[repr(C)]
pub struct _libc_xmmreg {
    pub element: [c_uint; 4],
}
#[cfg(not(target_arch = "aarch64"))]
#[repr(C)]
pub struct mcontext {
    pub gregs: [c_longlong; 23], // TODO: greg_t?
    pub fpregs: *mut _libc_fpstate,
    __private: [c_ulonglong; 8],
}

/// AArch64 Linux (asm/sigcontext.h, struct sigcontext): x0 to x30, sp, pc
/// and pstate, then records of further state (the FP and SIMD registers
/// first) in `__reserved`.
#[cfg(target_arch = "aarch64")]
#[repr(C, align(16))]
pub struct mcontext {
    pub fault_address: c_ulonglong,
    pub regs: [c_ulonglong; 31],
    pub sp: c_ulonglong,
    pub pc: c_ulonglong,
    pub pstate: c_ulonglong,
    pub __reserved: [c_uchar; 4096],
}

// The layouts the kernel uses: those of the libc crate for this target.
#[cfg(all(target_arch = "aarch64", feature = "check_against_libc_crate"))]
const _: () = {
    use __libc_only_for_layout_checks as libc;
    use core::mem::{align_of, offset_of, size_of};
    assert!(size_of::<ucontext>() == size_of::<libc::ucontext_t>());
    assert!(offset_of!(ucontext, uc_sigmask) == offset_of!(libc::ucontext_t, uc_sigmask));
    assert!(offset_of!(ucontext, uc_mcontext) == offset_of!(libc::ucontext_t, uc_mcontext));
    assert!(size_of::<mcontext>() == size_of::<libc::mcontext_t>());
    assert!(align_of::<mcontext>() == align_of::<libc::mcontext_t>());
    assert!(offset_of!(mcontext, regs) == offset_of!(libc::mcontext_t, regs));
    assert!(offset_of!(mcontext, sp) == offset_of!(libc::mcontext_t, sp));
    assert!(offset_of!(mcontext, pc) == offset_of!(libc::mcontext_t, pc));
    assert!(offset_of!(mcontext, pstate) == offset_of!(libc::mcontext_t, pstate));
};
