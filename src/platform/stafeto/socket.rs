// SPDX-License-Identifier: MIT
#![allow(unused_variables, unused_imports, unused_mut)]
use super::{Sys, stafeto_exit};
use crate::{error::Errno, header::errno::ENOSYS};
use crate::{
    error::Result,
    header::sys_socket::{msghdr, socklen_t},
    out::Out,
    platform::{PalSocket, types::c_int},
};

impl PalSocket for Sys {
    fn accept(socket: c_int, address_dst: Option<&mut [u8]>) -> Result<(c_int, socklen_t)> {
        Err(Errno(ENOSYS))
    }

    fn bind(socket: c_int, address_raw: &[u8]) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn connect(socket: c_int, address_raw: &[u8]) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn getpeername(socket: c_int, address_dst: &mut [u8]) -> Result<socklen_t> {
        Err(Errno(ENOSYS))
    }

    fn getsockname(socket: c_int, address_dst: &mut [u8]) -> Result<socklen_t> {
        Err(Errno(ENOSYS))
    }

    fn getsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: &mut [u8],
    ) -> Result<socklen_t> {
        Err(Errno(ENOSYS))
    }

    fn listen(socket: c_int, backlog: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn recvfrom(
        socket: c_int,
        buf: Out<[u8]>,
        flags: c_int,
        address_raw: Option<&mut [u8]>,
    ) -> Result<(usize, socklen_t)> {
        Err(Errno(ENOSYS))
    }

    unsafe fn recvmsg(socket: c_int, msg: *mut msghdr, flags: c_int) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    unsafe fn sendmsg(socket: c_int, msg: *const msghdr, flags: c_int) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn sendto(socket: c_int, buf: &[u8], flags: c_int, dest: Option<&[u8]>) -> Result<usize> {
        Err(Errno(ENOSYS))
    }

    fn setsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: &[u8],
    ) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn shutdown(socket: c_int, how: c_int) -> Result<()> {
        Err(Errno(ENOSYS))
    }

    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> Result<c_int> {
        Err(Errno(ENOSYS))
    }

    fn socketpair(domain: c_int, kind: c_int, protocol: c_int, sv: &mut [c_int; 2]) -> Result<()> {
        Err(Errno(ENOSYS))
    }
}
