//! Allocation-free native TCB construction for the static stafeto loader.

use super::tcb::{Master, OsSpecific, Tcb};
use crate::header::errno::{EAGAIN, EINVAL, ENOMEM};
use core::{
    mem, ptr,
    sync::atomic::{AtomicBool, Ordering},
};

const _: () = {
    assert!(mem::size_of::<Master>() == 32);
    assert!(mem::align_of::<Master>() == 8);
    assert!(mem::size_of::<usize>() == 8);
};

static READY: AtomicBool = AtomicBool::new(false);
static TLS_ALIGN: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(1);

pub(super) fn record_align(align: usize) {
    TLS_ALIGN.store(align.max(1), Ordering::Relaxed);
}

#[path = "native_tcb/layout.rs"]
mod layout;
pub use layout::Descriptor;
use layout::Layout;

static mut DESCRIPTOR: Descriptor = Descriptor {
    version: 1,
    image: ptr::null(),
    image_len: 0,
    tls_len: 0,
    tls_align: 1,
    tcb_len: mem::size_of::<Tcb>(),
    tcb_align: mem::align_of::<Tcb>(),
};

/// Publish the immutable static image once, before stafeto_init.
/// # Safety
/// Startup is single threaded; the image remains mapped until ProcessEnd.
pub(crate) unsafe fn publish() {
    if READY.load(Ordering::Acquire) {
        return;
    }
    let Some(tcb) = (unsafe { Tcb::current() }) else {
        return;
    };
    if !tcb.linker_ptr.is_null() {
        return;
    }
    let master = unsafe { &*ptr::addr_of!(super::STATIC_TCB_MASTER) };
    // static_init has already validated and copied this executable image.
    unsafe {
        DESCRIPTOR.image = master.ptr;
        DESCRIPTOR.image_len = master.image_size;
        DESCRIPTOR.tls_len = master.segment_size;
        DESCRIPTOR.tls_align = TLS_ALIGN.load(Ordering::Relaxed);
    }
    READY.store(true, Ordering::Release);
}

/// The static master has process lifetime, also for pthread children of native callers.
pub(crate) fn static_master() -> Option<*mut Master> {
    READY
        .load(Ordering::Acquire)
        .then_some(ptr::addr_of_mut!(super::STATIC_TCB_MASTER))
}

/// Static children borrow the immutable process master through their own lifetime.
pub(crate) fn child_masters(current: &Tcb) -> (*mut Master, usize) {
    if current.linker_ptr.is_null()
        && let Some(master) = static_master()
    {
        (master, mem::size_of::<Master>())
    } else {
        (current.masters_ptr, current.masters_len)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn relibc_stafeto_native_tls_v1() -> *const Descriptor {
    if READY.load(Ordering::Acquire) {
        ptr::addr_of!(DESCRIPTOR)
    } else {
        ptr::null()
    }
}

fn layout(d: &Descriptor, length: usize) -> Option<Layout> {
    if d.tcb_len != mem::size_of::<Tcb>() || d.tcb_align != mem::align_of::<Tcb>() {
        return None;
    }
    layout::checked(d, length)
}

/// Validate geometry before the caller reserves any resources.
/// # Safety
/// out points to four writable usize words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn relibc_stafeto_native_layout_v1(out: *mut usize) -> i32 {
    if !READY.load(Ordering::Acquire) {
        return EAGAIN;
    }
    if out.is_null() {
        return EINVAL;
    }
    let d = unsafe { &*ptr::addr_of!(DESCRIPTOR) };
    let Some(l) = layout(d, 4096) else {
        return ENOMEM;
    };
    unsafe {
        out.write(l.tcb);
        out.add(1).write(
            l.tcb
                + mem::offset_of!(Tcb, generic)
                + mem::offset_of!(generic_rt::GenericTcb<OsSpecific>, os_specific),
        );
        out.add(2).write(l.end);
        out.add(3).write(l.tp);
    }
    0
}

/// Build a complete TCB, its fresh static TLS and eager DTV inside one prepaid page.
/// Returns zero on success or a positive errno. No field is written on error.
/// # Safety
/// page is an exclusively owned writable page; out points to two writable usize words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn relibc_stafeto_native_tcb_v1(
    page: *mut u8,
    length: usize,
    id: usize,
    out: *mut usize,
) -> i32 {
    if !READY.load(Ordering::Acquire) {
        return EAGAIN;
    }
    if !(1..=64).contains(&id)
        || page.is_null()
        || page as usize & 4095 != 0
        || length != 4096
        || out.is_null()
    {
        return EINVAL;
    }
    let d = unsafe { &*ptr::addr_of!(DESCRIPTOR) };
    let Some(l) = layout(d, length) else {
        return ENOMEM;
    };
    // All ranges and arithmetic were checked before this first write.
    unsafe {
        ptr::write_bytes(page, 0, length);
        let tls = page.add(l.tls);
        if d.image_len != 0 {
            ptr::copy_nonoverlapping(d.image, tls, d.image_len);
        }
        let tcb = page.add(l.tcb).cast::<Tcb>();
        Tcb::initialize(tcb, page.add(l.tcb), l.tls_len, d.tcb_len);
        (*tcb)
            .pthread
            .os_tid
            .get()
            .write(crate::pthread::OsTid { thread_id: id });
        let master = page.add(l.master).cast::<Master>();
        ptr::write(
            master,
            Master {
                ptr: d.image,
                image_size: d.image_len,
                segment_size: d.tls_len,
                offset: d.tls_len,
            },
        );
        (*tcb).masters_ptr = master;
        (*tcb).masters_len = mem::size_of::<Master>();
        (*tcb).num_copied_masters = 1;
        let dtv = page.add(l.dtv).cast::<*mut u8>();
        ptr::write(dtv, tls);
        (*tcb).dtv_ptr = dtv;
        (*tcb).dtv_len = 1;
        ptr::write(page.add(l.tp).cast::<usize>(), tcb as usize);
        ptr::write(out, page.add(l.tp) as usize);
        ptr::write(
            out.add(1),
            ptr::addr_of_mut!((*tcb).generic.os_specific).cast::<OsSpecific>() as usize,
        );
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[repr(C, align(4096))]
    struct Page([u8; 4096]);

    #[test]
    fn builder_retains_page_local_metadata_and_children_use_process_image() {
        let image = [0x35u8; 32];
        // This test runs before any simulated resource admission and owns publication.
        unsafe {
            DESCRIPTOR = Descriptor {
                version: 1,
                image: image.as_ptr(),
                image_len: image.len(),
                tls_len: 84,
                tls_align: 8,
                tcb_len: mem::size_of::<Tcb>(),
                tcb_align: mem::align_of::<Tcb>(),
            };
            super::super::STATIC_TCB_MASTER = Master {
                ptr: image.as_ptr(),
                image_size: 32,
                segment_size: 84,
                offset: 88,
            };
        }
        READY.store(true, Ordering::Release);
        let mut page = Page([0xa5; 4096]);
        let base = page.0.as_mut_ptr();
        let mut out = [usize::MAX; 2];
        assert_eq!(
            unsafe { relibc_stafeto_native_tcb_v1(base, 4096, 65, out.as_mut_ptr()) },
            EINVAL
        );
        assert!(page.0.iter().all(|byte| *byte == 0xa5));
        assert_eq!(out, [usize::MAX; 2]);
        assert_eq!(
            unsafe { relibc_stafeto_native_tcb_v1(base, 4096, 7, out.as_mut_ptr()) },
            0
        );
        let tcb = unsafe { &*(out[0] as *const *const Tcb).read() };
        assert_eq!(out[1], ptr::addr_of!(tcb.generic.os_specific) as usize);
        assert_eq!(unsafe { tcb.pthread.os_tid.get().read() }.thread_id, 7);
        assert_eq!(&page.0[16..48], &image);
        assert!(page.0[48..100].iter().all(|byte| *byte == 0));
        let range = base as usize..base as usize + 4096;
        assert!(range.contains(&(tcb.masters_ptr as usize)));
        assert!(range.contains(&(tcb.dtv_ptr as usize)));
        assert_eq!(tcb.dtv_len, 1);
        assert_eq!(tcb.num_copied_masters, 1);
        assert_eq!(unsafe { tcb.dtv_ptr.read() }, unsafe { base.add(16) });
        let (child, child_len) = child_masters(tcb);
        assert_ne!(child, tcb.masters_ptr);
        assert_eq!(child_len, mem::size_of::<Master>());
        // End reclaims the native page. The child still sees the process image.
        page.0.fill(0xa5);
        assert_eq!(unsafe { (*child).data() }, &image);
        READY.store(false, Ordering::Release);
    }
}
