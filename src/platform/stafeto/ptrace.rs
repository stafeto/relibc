// SPDX-License-Identifier: MIT
#![allow(unused_variables, unused_imports, unused_mut)]
use super::{
    super::{
        PalPtrace,
        types::{c_int, c_void, pid_t},
    },
    Sys, stafeto_exit,
};
use crate::error::Result;
use crate::{error::Errno, header::errno::ENOSYS};

impl PalPtrace for Sys {
    unsafe fn ptrace(
        request: c_int,
        pid: pid_t,
        addr: *mut c_void,
        data: *mut c_void,
    ) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }
}
