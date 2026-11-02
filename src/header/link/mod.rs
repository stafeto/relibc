//! `link.h`: `dl_iterate_phdr`.
//!
//! Non-POSIX, see <https://www.man7.org/linux/man-pages/man3/dl_iterate_phdr.3.html>.
//!
//! stafeto: a process has the program and, when it is linked dynamically,
//! libc.so, and the loader gives the program headers of both in the
//! auxiliary vector (`AT_PHDR`, `AT_PHNUM`; `AT_BASE` is libc.so's ELF
//! header). The callback sees them in that order.

use crate::{
    header::elf::Elf64_Phdr,
    platform::types::{c_char, c_int, c_ulonglong, c_void, size_t},
};

/// What the callback of `dl_iterate_phdr` is told of one object.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct dl_phdr_info {
    /// The difference between the addresses of the file and those in memory
    /// (0 for both modules: libc.so and the program are placed where they say).
    dlpi_addr: u64,
    /// The name of the object: "" for the program.
    dlpi_name: *const c_char,
    dlpi_phdr: *const Elf64_Phdr,
    dlpi_phnum: u16,
    /// Objects added and removed so far.
    dlpi_adds: c_ulonglong,
    dlpi_subs: c_ulonglong,
    /// The module of thread-local storage (0 for none).
    dlpi_tls_modid: size_t,
    dlpi_tls_data: *mut c_void,
}

#[cfg(stafeto)]
const AT_PHDR: usize = crate::platform::auxv_defs::AT_PHDR;
#[cfg(stafeto)]
const AT_PHNUM: usize = crate::platform::auxv_defs::AT_PHNUM;
#[cfg(stafeto)]
const AT_BASE: usize = crate::platform::auxv_defs::AT_BASE;

/// See <https://www.man7.org/linux/man-pages/man3/dl_iterate_phdr.3.html>.
///
/// stafeto: the program, then libc.so for a dynamically linked program;
/// the walk ends when the callback answers nonzero, and that answer is the
/// result.
#[cfg(stafeto)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dl_iterate_phdr(
    callback: Option<unsafe extern "C" fn(*mut dl_phdr_info, size_t, *mut c_void) -> c_int>,
    data: *mut c_void,
) -> c_int {
    use crate::header::sys_auxv::getauxval;
    let Some(callback) = callback else {
        return 0;
    };
    let value = |key: usize| getauxval(key as _) as usize;
    let (phdr, phnum, base) = (value(AT_PHDR), value(AT_PHNUM), value(AT_BASE));
    let library_headers = if base == 0 {
        None
    } else {
        // SAFETY: AT_BASE is the address of libc.so's ELF header, which the
        // loader mapped; e_phoff is at 32 and e_phnum at 56.
        unsafe {
            let header = base as *const u8;
            let off = header.add(32).cast::<u64>().read_unaligned() as usize;
            let num = header.add(56).cast::<u16>().read_unaligned();
            Some((base + off, num))
        }
    };
    let adds = 1 + c_ulonglong::from(library_headers.is_some());
    let mut info = dl_phdr_info {
        dlpi_addr: 0,
        dlpi_name: c"".as_ptr(),
        dlpi_phdr: phdr as *const Elf64_Phdr,
        dlpi_phnum: phnum as u16,
        dlpi_adds: adds,
        dlpi_subs: 0,
        dlpi_tls_modid: 0,
        dlpi_tls_data: core::ptr::null_mut(),
    };
    let size = core::mem::size_of::<dl_phdr_info>();
    // SAFETY: the callback is the caller's; `info` outlives the call.
    let stop = unsafe { callback(&mut info, size, data) };
    if stop != 0 {
        return stop;
    }
    let Some((headers, num)) = library_headers else {
        return 0;
    };
    info.dlpi_name = c"/lib/libc.so".as_ptr();
    info.dlpi_phdr = headers as *const Elf64_Phdr;
    info.dlpi_phnum = num;
    info.dlpi_tls_modid = 1;
    // SAFETY: as above.
    unsafe { callback(&mut info, size, data) }
}
