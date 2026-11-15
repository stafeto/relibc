//! `pthread_attr_t` from `sys/types.h` implementation.
//!
//! See <https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/sys_types.h.html>.

use crate::{
    platform::types::{c_uchar, size_t},
    pthread_assert_equal_size,
};

/// Used to identify a thread attribute object.
#[repr(C)]
pub union pthread_attr_t {
    // 96 bytes: the scheduling parameters (struct sched_param, 48 bytes) are
    // held whole, with the sporadic server members, and 16 bytes are
    // reserved for later members.
    __relibc_internal_size: [c_uchar; 96],
    __relibc_internal_align: size_t,
}

pthread_assert_equal_size!(pthread_attr_t, RlctAttr);

#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<pthread_attr_t>() == 96);
