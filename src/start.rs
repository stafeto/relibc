//! Startup code.

use alloc::{boxed::Box, vec::Vec};
use core::{intrinsics, marker::PhantomData, ptr};

use crate::{
    c_str::CStr,
    header::{libgen, stdlib},
    ld_so::{self},
    platform::{
        self, Pal, Sys, get_auxvs,
        types::{c_char, c_int},
    },
};

#[repr(C)]
pub struct Stack<'a> {
    argc: isize,
    argv0: *const c_char,
    _marker: PhantomData<&'a [u8]>,
}

impl<'a> Stack<'a> {
    pub fn argc(&self) -> isize {
        self.argc
    }
    pub unsafe fn set_argc(&mut self, new: isize) {
        self.argc = new;
    }
    pub fn argv_raw(&self) -> *const *const c_char {
        ptr::from_ref(&self.argv0)
    }
    pub fn argv_with_last_null(&self) -> &'a [Option<CStr<'a>>] {
        // SAFETY: safe by construction of this struct
        unsafe {
            let raw_cstrs = core::slice::from_raw_parts(
                self.argv_raw(),
                usize::try_from(self.argc).unwrap() + 1,
            );
            CStr::opt_strs_from_raw(raw_cstrs)
        }
    }

    pub fn envp_raw(&self) -> *const *const c_char {
        unsafe { self.argv_raw().offset(self.argc + 1) }
    }
    pub fn envp_with_last_null(&self) -> &'a [Option<CStr<'a>>] {
        let mut count = 0;
        // TODO: use NullTerminated iterator
        let mut base = self.envp_raw();
        while unsafe { !base.read().is_null() } {
            base = unsafe { base.add(1) };
            count += 1;
        }

        // SAFETY: safe by construction of this struct
        unsafe {
            let raw_cstrs = core::slice::from_raw_parts(self.envp_raw(), count + 1);
            CStr::opt_strs_from_raw(raw_cstrs)
        }
    }

    pub fn auxv(&self) -> *const (usize, usize) {
        unsafe {
            let mut envp = self.envp_raw();
            while !(*envp).is_null() {
                envp = envp.add(1);
            }
            envp.add(1).cast::<(usize, usize)>()
        }
    }
}

fn copy_string_array(array_with_nul: &[Option<CStr>]) -> Vec<*mut c_char> {
    let array_without_nul = &array_with_nul[..array_with_nul.len() - 1];
    let mut vec = Vec::with_capacity(array_with_nul.len());
    let mut lengths = Vec::with_capacity(array_without_nul.len());
    let mut size = 0;

    for item in array_without_nul {
        let this_len = item.expect("non-trailing NULL").len() + 1;
        lengths.push(this_len);
        size += this_len;
    }

    // Programs unfortunately rely on the strings being contiguous in memory. For example:
    // https://github.com/libuv/libuv/blob/12d0dd48e3c6baf1e2f0d9f85f11f0ef58285d6f/src/unix/proctitle.c#L87
    let mut offset = 0;
    let buf = Box::leak(vec![0_u8; size].into_boxed_slice());

    for (len, item_opt) in lengths.into_iter().zip(array_without_nul) {
        let item = item_opt.expect("non-trailing NULL");

        let dst = &mut buf[offset..][..len];
        dst.copy_from_slice(item.to_bytes_with_nul());

        vec.push(dst.as_mut_ptr().cast());
        offset += len;
    }
    vec.push(ptr::null_mut());
    vec
}

// Since Redox and Linux are so similar, it is easy to accidentally run a binary from one on the
// other. This will test that the current system is compatible with the current binary
#[unsafe(no_mangle)]
pub unsafe fn relibc_verify_host() {
    if !Sys::verify() {
        intrinsics::abort();
    }
}
#[unsafe(link_section = ".init_array")]
#[used]
static INIT_ARRAY: [extern "C" fn(); 1] = [init_array];

static mut INIT_COMPLETE: bool = false;

#[used]
#[unsafe(no_mangle)]
static mut __relibc_init_environ: *mut *mut c_char = ptr::null_mut();

extern "C" fn init_array() {
    // The thing is that we cannot guarantee if
    // init_array runs first or if relibc_start runs first

    unsafe {
        if INIT_COMPLETE {
            return;
        }
    }

    unsafe {
        if platform::environ.is_null() {
            platform::environ = __relibc_init_environ;
        }
    }

    unsafe {
        crate::pthread::init();
        INIT_COMPLETE = true
    }
}

/// stafeto: the constructor arrays of a dynamically linked program, found
/// through its `PT_DYNAMIC` (the loader names the program's headers with
/// `AT_PHDR`). `(preinit, init)`, each a start and a count of functions.
#[cfg(stafeto)]
struct ProgramArrays {
    preinit: (*const extern "C" fn(), usize),
    init: (*const extern "C" fn(), usize),
    fini: (*const extern "C" fn(), usize),
}

/// The `DT_FINI_ARRAY` of a dynamically linked program: start and count, for
/// `exit` (count 0 for a static program).
#[cfg(stafeto)]
pub static PROGRAM_FINI: (core::sync::atomic::AtomicUsize, core::sync::atomic::AtomicUsize) = (
    core::sync::atomic::AtomicUsize::new(0),
    core::sync::atomic::AtomicUsize::new(0),
);

/// None for a program that is linked statically (no `AT_STAFETO_TLS`), whose
/// arrays the linker bounded with `__init_array_start` and its kin.
#[cfg(stafeto)]
unsafe fn program_arrays(sp: &Stack) -> Option<ProgramArrays> {
    use crate::platform::auxv_defs::{AT_STAFETO_TLS, AT_NULL, AT_PHDR, AT_PHENT, AT_PHNUM};
    const PT_PHDR: u32 = 6;
    const PT_DYNAMIC: u32 = 2;
    const DT_NULL: u64 = 0;
    const DT_INIT_ARRAY: u64 = 25;
    const DT_INIT_ARRAYSZ: u64 = 27;
    const DT_FINI_ARRAY: u64 = 26;
    const DT_FINI_ARRAYSZ: u64 = 28;
    const DT_PREINIT_ARRAY: u64 = 32;
    const DT_PREINIT_ARRAYSZ: u64 = 33;
    // Elf64_Phdr: p_type u32 at 0, p_vaddr u64 at 16.
    const PHDR64: usize = 56;
    let (mut dynamic, mut phdr, mut phnum, mut phent) = (false, 0, 0, 0);
    let mut auxv = sp.auxv();
    loop {
        let (kind, value) = unsafe { *auxv };
        match kind {
            AT_NULL => break,
            AT_STAFETO_TLS => dynamic = true,
            AT_PHDR => phdr = value,
            AT_PHNUM => phnum = value,
            AT_PHENT => phent = value,
            _ => (),
        }
        auxv = unsafe { auxv.add(1) };
    }
    if !dynamic {
        return None;
    }
    // The loader wrote the vector: a header size it cannot read is a fault
    // of the loader, and the process ends with the code of a refused start.
    if phent != PHDR64 {
        Sys::exit(125);
    }
    let mut bias = 0usize;
    let mut dynamic_at = None;
    for i in 0..phnum {
        let header = (phdr + PHDR64 * i) as *const u8;
        let kind = unsafe { header.cast::<u32>().read_unaligned() };
        let vaddr = unsafe { header.add(16).cast::<u64>().read_unaligned() } as usize;
        match kind {
            // The shift of the program: zero for ET_EXEC, whose headers lie
            // at their own addresses.
            PT_PHDR => bias = phdr.wrapping_sub(vaddr),
            PT_DYNAMIC => dynamic_at = Some(vaddr),
            _ => (),
        }
    }
    let mut entry = dynamic_at?.wrapping_add(bias) as *const u64;
    let (mut preinit, mut preinit_size, mut init, mut init_size) = (0u64, 0u64, 0u64, 0u64);
    let (mut fini, mut fini_size) = (0u64, 0u64);
    loop {
        let tag = unsafe { entry.read() };
        let value = unsafe { entry.add(1).read() };
        match tag {
            DT_NULL => break,
            DT_PREINIT_ARRAY => preinit = value,
            DT_PREINIT_ARRAYSZ => preinit_size = value,
            DT_INIT_ARRAY => init = value,
            DT_INIT_ARRAYSZ => init_size = value,
            DT_FINI_ARRAY => fini = value,
            DT_FINI_ARRAYSZ => fini_size = value,
            _ => (),
        }
        entry = unsafe { entry.add(2) };
    }
    let word = core::mem::size_of::<usize>();
    let array = |at: u64, size: u64| {
        (
            (at as usize).wrapping_add(bias) as *const extern "C" fn(),
            if at == 0 { 0 } else { size as usize / word },
        )
    };
    Some(ProgramArrays {
        preinit: array(preinit, preinit_size),
        init: array(init, init_size),
        fini: array(fini, fini_size),
    })
}

#[cfg(stafeto)]
unsafe fn run_array((start, count): (*const extern "C" fn(), usize)) {
    for i in 0..count {
        (unsafe { *start.add(i) })();
    }
}

/// Sets `__stack_chk_guard` from the 16 bytes AT_RANDOM points to; without
/// them (or with all zero bytes) from a blocking `getrandom`. A process that
/// has neither ends with `abort`: the canary never takes a value that is the
/// same in every process. The low byte of the canary is zero, so that a string
/// copy stops at it.
///
/// The functions this start calls before it returns are Rust, which has no
/// stack protector, and `relibc_start_v1` does not return, so the canary
/// changes under no frame that checks it.
#[inline(never)]
unsafe fn init_stack_guard(sp: &Stack) {
    use crate::platform::auxv_defs::AT_RANDOM;
    const WORD: usize = core::mem::size_of::<usize>();

    let mut bytes = [0u8; WORD];
    let at = unsafe { crate::platform::get_auxv_raw(sp.auxv().cast(), AT_RANDOM) };
    if let Some(at) = at.filter(|at| *at != 0) {
        // SAFETY: the loader names 16 readable bytes.
        bytes.copy_from_slice(unsafe { core::slice::from_raw_parts(at as *const u8, WORD) });
    }
    // The first byte in memory is the low byte on the little-endian targets;
    // clear the low byte of the value on any of them.
    let mut value = usize::from_ne_bytes(bytes) & !0xff;
    let mut draws = 0;
    while value == 0 {
        // A wait for the first seed that a signal handler ended goes on.
        match Sys::getrandom(&mut bytes, 0) {
            Ok(WORD) => {
                value = usize::from_ne_bytes(bytes) & !0xff;
                draws += 1;
                // A generator that gives only zero is no generator.
                if value == 0 && draws == 8 {
                    unsafe { stdlib::abort() };
                }
            }
            Err(crate::error::Errno(crate::header::errno::EINTR)) => {}
            _ => unsafe { stdlib::abort() },
        }
    }
    unsafe { stdlib::set_stack_guard(value) };
    // The bytes of the stack do not outlive the call.
    for b in bytes.iter_mut() {
        unsafe { core::ptr::write_volatile(b, 0) };
    }
}

#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn relibc_start_v1(
    sp: &'static Stack,
    main: unsafe extern "C" fn(
        argc: isize,
        argv: *mut *mut c_char,
        envp: *mut *mut c_char,
    ) -> c_int,
) -> ! {
    unsafe extern "C" {
        static __preinit_array_start: extern "C" fn();
        static __preinit_array_end: extern "C" fn();
        static __init_array_start: extern "C" fn();
        static __init_array_end: extern "C" fn();
    }

    // Ensure correct host system before executing more system calls
    unsafe { relibc_verify_host() };

    // The canary of the stack protector, before anything that uses it runs.
    unsafe { init_stack_guard(sp) };

    #[cfg(target_os = "redox")]
    let thr_fd = redox_rt::proc::FdGuard::new(
        unsafe {
            crate::platform::get_auxv_raw(sp.auxv().cast(), redox_rt::auxv_defs::AT_REDOX_THR_FD)
        }
        .expect("no thread fd present"),
    )
    .to_upper()
    .expect("failed to move thread fd to upper table");

    #[cfg(target_os = "redox")]
    {
        if redox_rt::current_filetable().fd().is_none() {
            let filetable_fd = unsafe {
                crate::platform::get_auxv_raw(
                    sp.auxv().cast(),
                    redox_rt::auxv_defs::AT_REDOX_FILETABLE_FD,
                )
            }
            .expect("no filetable fd present");
            let filetable_guard = redox_rt::proc::FdGuard::new(filetable_fd)
                .to_upper()
                .expect("failed to move filetable fd to upper table");
            *redox_rt::current_filetable() = redox_rt::sys::FdTbl::from_binary_fd(filetable_guard)
                .expect("failed to initialize FILETABLE");
        }
    }

    // Initialize TLS, if necessary
    unsafe {
        ld_so::init(
            sp,
            #[cfg(target_os = "redox")]
            thr_fd,
        )
    };

    #[cfg(target_os = "redox")]
    {
        redox_rt::TLS_ACTIVATED.store(true, core::sync::atomic::Ordering::Relaxed);
    }

    let is_dynamically_linked = if let Some(tcb) = unsafe { ld_so::tcb::Tcb::current() } {
        #[cfg(target_os = "redox")]
        redox_rt::signal::setup_sighandler(&tcb.os_specific, true);

        !tcb.linker_ptr.is_null()
    } else {
        false
    };

    // Set up argc and argv
    let argc = sp.argc();
    let argv = sp.argv_with_last_null();
    unsafe { platform::inner_argv.unsafe_set(copy_string_array(argv)) };
    unsafe { platform::argv = platform::inner_argv.unsafe_mut().as_mut_ptr() };
    // Special code for program_invocation_name and program_invocation_short_name
    if let Some(arg) = unsafe { platform::inner_argv.unsafe_ref() }.first() {
        unsafe { platform::program_invocation_name = *arg };
        unsafe { platform::program_invocation_short_name = libgen::basename(*arg) };
    }
    // We check for NULL here since ld.so might already have initialized it for us, and we don't
    // want to overwrite it if constructors in .init_array of dependency libraries have called
    // setenv.
    let envp = sp.envp_with_last_null();
    if unsafe { platform::environ }.is_null() {
        unsafe { platform::OUR_ENVIRON.unsafe_set(copy_string_array(envp)) };
        unsafe { platform::environ = platform::OUR_ENVIRON.unsafe_mut().as_mut_ptr() };
    }

    let auxvs = unsafe { get_auxvs(sp.auxv().cast()) };
    if !is_dynamically_linked {
        unsafe { crate::platform::init(auxvs) };
        init_array();
        if unsafe { crate::platform::logger::init().is_err() } {
            log::error!("Logger has already been initialised");
        }
    }

    // (It would technically have been equally valid to just allow the option to be set to "1".)
    if let Some(opt) = envp
        .iter()
        .filter_map(|x| *x)
        .find_map(|var| var.strip_prefix(b"RELIBC_DEBUG_COMMIT_HASH="))
        && matches!(opt.to_bytes(), b"1" | b"true")
    {
        let commit_hash =
            option_env!("RELIBC_COMMIT_HASH").unwrap_or("(unknown; not set by build.rs)");
        log::info!("Relibc commit hash: {commit_hash}");
    }

    // A dynamically linked program: its arrays go in the order of the gABI,
    // its preinit array, the C library's arrays (the loop below), its init
    // array.
    #[cfg(stafeto)]
    let program = unsafe { program_arrays(sp) };
    #[cfg(stafeto)]
    if let Some(program) = &program {
        PROGRAM_FINI.0.store(program.fini.0 as usize, core::sync::atomic::Ordering::Relaxed);
        PROGRAM_FINI.1.store(program.fini.1, core::sync::atomic::Ordering::Relaxed);
        unsafe { run_array(program.preinit) };
    }

    // Run preinit array
    {
        let mut f = core::ptr::from_ref(unsafe { &__preinit_array_start });
        while f < &raw const __preinit_array_end {
            (unsafe { *f })();
            f = unsafe { f.add(1) };
        }
    }

    // Run init array
    {
        let mut f = core::ptr::from_ref(unsafe { &__init_array_start });
        while f < &raw const __init_array_end {
            (unsafe { *f })();
            f = unsafe { f.add(1) };
        }
    }

    #[cfg(stafeto)]
    if let Some(program) = &program {
        unsafe { run_array(program.init) };
    }

    // not argv or envp, because programs like bash try to modify this *const* pointer :|
    unsafe { stdlib::exit(main(argc, platform::argv, platform::environ)) };

    unreachable!();
}
