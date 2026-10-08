//! Checked AArch64 layout for the allocation-free native TCB builder.

/// Immutable executable TLS image, published before the layer attaches main.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Descriptor {
    pub version: usize,
    pub image: *const u8,
    pub image_len: usize,
    pub tls_len: usize,
    pub tls_align: usize,
    pub tcb_len: usize,
    pub tcb_align: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) tp: usize,
    pub(super) tls: usize,
    pub(super) tls_len: usize,
    pub(super) tcb: usize,
    pub(super) master: usize,
    pub(super) dtv: usize,
    pub(super) end: usize,
}

fn align(value: usize, alignment: usize) -> Option<usize> {
    if !alignment.is_power_of_two() {
        return None;
    }
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
}

pub(super) fn checked(d: &Descriptor, length: usize) -> Option<Layout> {
    if d.version != 1 || d.image_len > d.tls_len || (d.image_len != 0 && d.image.is_null()) {
        return None;
    }
    // AArch64 TLS starts TP+16; alignment padding belongs to the same page.
    if !d.tls_align.is_power_of_two() {
        return None;
    }
    let tls = align(16, d.tls_align.max(16))?;
    let tcb = align(tls.checked_add(d.tls_len)?, d.tcb_align)?;
    let master = align(tcb.checked_add(d.tcb_len)?, 8)?;
    let dtv = align(master.checked_add(32)?, 8)?;
    let end = dtv.checked_add(8)?;
    (end <= length).then_some(Layout {
        tp: tls - 16,
        tls,
        tls_len: tcb - tls,
        tcb,
        master,
        dtv,
        end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor() -> Descriptor {
        Descriptor {
            version: 1,
            image: 1usize as *const u8,
            image_len: 32,
            tls_len: 84,
            tls_align: 8,
            tcb_len: 384,
            tcb_align: 16,
        }
    }
    #[test]
    fn errno_and_resident_regions_are_disjoint() {
        let d = descriptor();
        let l = checked(&d, 4096).unwrap();
        assert_eq!(l.tcb, 112);
        assert!(96 < 16 + d.tls_len);
        assert!(l.master >= l.tcb + d.tcb_len);
        assert!(l.dtv >= l.master + 32);
        assert!(l.end <= 4096);
    }
    #[test]
    fn oversized_image_alignment_and_overflow_are_rejected() {
        let mut d = descriptor();
        d.image_len = 85;
        assert!(checked(&d, 4096).is_none());
        d = descriptor();
        d.tls_len = usize::MAX;
        assert!(checked(&d, 4096).is_none());
        d = descriptor();
        d.tcb_len = usize::MAX;
        assert!(checked(&d, 4096).is_none());
        d = descriptor();
        d.tls_len = 4096;
        assert!(checked(&d, 4096).is_none());
        d = descriptor();
        d.tls_align = 4096;
        assert!(checked(&d, 4096).is_none());
        d = descriptor();
        d.tcb_align = 3;
        assert!(checked(&d, 4096).is_none());
    }
    #[test]
    fn larger_alignment_keeps_tp_tls_spacing() {
        let mut d = descriptor();
        d.tls_align = 32;
        let l = checked(&d, 4096).unwrap();
        assert_eq!(l.tp, 16);
        assert_eq!(l.tls, 32);
        assert_eq!(l.tcb, 128);
    }
    #[test]
    fn exact_page_boundary_and_zero_image() {
        let mut d = descriptor();
        let l = checked(&d, 4096).unwrap();
        assert!(checked(&d, l.end).is_some());
        assert!(checked(&d, l.end - 1).is_none());
        d.image_len = 0;
        d.image = core::ptr::null();
        assert!(checked(&d, 4096).is_some());
    }
}
