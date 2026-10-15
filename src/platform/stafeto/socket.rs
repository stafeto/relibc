// SPDX-License-Identifier: MIT
//! Sockets through the layer (posix_abi::sockets): AF_UNIX and the loopback
//! of AF_INET. The addresses go as relibc has them; send, sendto, recv and
//! recvfrom are sendmsg and recvmsg with one part.
use super::{Sys, ret};
use crate::{
    error::Result,
    header::sys_socket::{msghdr, socklen_t},
    out::Out,
    platform::{PalSocket, types::c_int},
};
use core::ptr;

unsafe extern "C" {
    fn stafeto_socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn stafeto_socketpair(domain: c_int, kind: c_int, protocol: c_int, sv: *mut c_int) -> c_int;
    fn stafeto_bind(fd: c_int, address: *const u8, len: usize) -> c_int;
    fn stafeto_listen(fd: c_int, backlog: c_int) -> c_int;
    fn stafeto_connect(fd: c_int, address: *const u8, len: usize) -> c_int;
    fn stafeto_accept(fd: c_int, address: *mut u8, len: *mut u32, flags: c_int) -> c_int;
    fn stafeto_getsockname(fd: c_int, address: *mut u8, len: *mut u32) -> c_int;
    fn stafeto_getpeername(fd: c_int, address: *mut u8, len: *mut u32) -> c_int;
    fn stafeto_getsockopt(
        fd: c_int,
        level: c_int,
        name: c_int,
        option: *mut u8,
        len: *mut u32,
    ) -> c_int;
    fn stafeto_setsockopt(
        fd: c_int,
        level: c_int,
        name: c_int,
        option: *const u8,
        len: usize,
    ) -> c_int;
    fn stafeto_shutdown(fd: c_int, how: c_int) -> c_int;
    fn stafeto_sockatmark(fd: c_int) -> c_int;
    fn stafeto_sendmsg(fd: c_int, message: *const msghdr, flags: c_int) -> isize;
    fn stafeto_recvmsg(fd: c_int, message: *mut msghdr, flags: c_int) -> isize;
}

impl Sys {
    /// accept4: accept with SOCK_NONBLOCK and SOCK_CLOEXEC.
    pub fn accept4(
        socket: c_int,
        address_dst: Option<&mut [u8]>,
        flags: c_int,
    ) -> Result<(c_int, socklen_t)> {
        let mut len = address_dst.as_ref().map_or(0, |a| a.len()) as u32;
        let (pointer, want) = match address_dst {
            Some(a) => (a.as_mut_ptr(), true),
            None => (ptr::null_mut(), false),
        };
        let fd = ret(unsafe {
            stafeto_accept(
                socket,
                pointer,
                if want { &raw mut len } else { ptr::null_mut() },
                flags,
            )
        } as isize)? as c_int;
        Ok((fd, len))
    }

    /// sockatmark: 1 at the mark; no socket has one.
    pub fn sockatmark(socket: c_int) -> Result<c_int> {
        Ok(ret(unsafe { stafeto_sockatmark(socket) } as isize)? as c_int)
    }
}

impl PalSocket for Sys {
    fn accept(socket: c_int, address_dst: Option<&mut [u8]>) -> Result<(c_int, socklen_t)> {
        Sys::accept4(socket, address_dst, 0)
    }

    fn bind(socket: c_int, address_raw: &[u8]) -> Result<()> {
        ret(unsafe { stafeto_bind(socket, address_raw.as_ptr(), address_raw.len()) } as isize)
            .map(|_| ())
    }

    fn connect(socket: c_int, address_raw: &[u8]) -> Result<c_int> {
        ret(unsafe { stafeto_connect(socket, address_raw.as_ptr(), address_raw.len()) } as isize)
            .map(|n| n as c_int)
    }

    fn getpeername(socket: c_int, address_dst: &mut [u8]) -> Result<socklen_t> {
        let mut len = address_dst.len() as u32;
        ret(unsafe { stafeto_getpeername(socket, address_dst.as_mut_ptr(), &raw mut len) }
            as isize)?;
        Ok(len)
    }

    fn getsockname(socket: c_int, address_dst: &mut [u8]) -> Result<socklen_t> {
        let mut len = address_dst.len() as u32;
        ret(unsafe { stafeto_getsockname(socket, address_dst.as_mut_ptr(), &raw mut len) }
            as isize)?;
        Ok(len)
    }

    fn getsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: &mut [u8],
    ) -> Result<socklen_t> {
        let mut len = option_value.len() as u32;
        ret(unsafe {
            stafeto_getsockopt(
                socket,
                level,
                option_name,
                option_value.as_mut_ptr(),
                &raw mut len,
            )
        } as isize)?;
        Ok(len)
    }

    fn listen(socket: c_int, backlog: c_int) -> Result<()> {
        ret(unsafe { stafeto_listen(socket, backlog) } as isize).map(|_| ())
    }

    fn recvfrom(
        socket: c_int,
        mut buf: Out<[u8]>,
        flags: c_int,
        address_raw: Option<&mut [u8]>,
    ) -> Result<(usize, socklen_t)> {
        let mut iov = crate::header::sys_uio::iovec {
            iov_base: buf.as_mut_ptr().as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        let (name, namelen) = match address_raw {
            Some(a) => (a.as_mut_ptr().cast(), a.len() as socklen_t),
            None => (ptr::null_mut(), 0),
        };
        let mut message = msghdr {
            msg_name: name,
            msg_namelen: namelen,
            msg_iov: &raw mut iov,
            msg_iovlen: 1,
            msg_control: ptr::null_mut(),
            msg_controllen: 0,
            msg_flags: 0,
        };
        let count = ret(unsafe { stafeto_recvmsg(socket, &raw mut message, flags) })?;
        Ok((count as usize, message.msg_namelen))
    }

    unsafe fn recvmsg(socket: c_int, msg: *mut msghdr, flags: c_int) -> Result<usize> {
        Ok(ret(unsafe { stafeto_recvmsg(socket, msg, flags) })? as usize)
    }

    unsafe fn sendmsg(socket: c_int, msg: *const msghdr, flags: c_int) -> Result<usize> {
        Ok(ret(unsafe { stafeto_sendmsg(socket, msg, flags) })? as usize)
    }

    fn sendto(socket: c_int, buf: &[u8], flags: c_int, dest: Option<&[u8]>) -> Result<usize> {
        let mut iov = crate::header::sys_uio::iovec {
            iov_base: buf.as_ptr().cast_mut().cast(),
            iov_len: buf.len(),
        };
        let (name, namelen) = match dest {
            Some(a) => (a.as_ptr().cast_mut().cast(), a.len() as socklen_t),
            None => (ptr::null_mut(), 0),
        };
        let message = msghdr {
            msg_name: name,
            msg_namelen: namelen,
            msg_iov: &raw mut iov,
            msg_iovlen: 1,
            msg_control: ptr::null_mut(),
            msg_controllen: 0,
            msg_flags: 0,
        };
        Ok(ret(unsafe { stafeto_sendmsg(socket, &raw const message, flags) })? as usize)
    }

    fn setsockopt(
        socket: c_int,
        level: c_int,
        option_name: c_int,
        option_value: &[u8],
    ) -> Result<()> {
        ret(unsafe {
            stafeto_setsockopt(
                socket,
                level,
                option_name,
                option_value.as_ptr(),
                option_value.len(),
            )
        } as isize)
        .map(|_| ())
    }

    fn shutdown(socket: c_int, how: c_int) -> Result<()> {
        ret(unsafe { stafeto_shutdown(socket, how) } as isize).map(|_| ())
    }

    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> Result<c_int> {
        Ok(ret(unsafe { stafeto_socket(domain, kind, protocol) } as isize)? as c_int)
    }

    fn socketpair(domain: c_int, kind: c_int, protocol: c_int, sv: &mut [c_int; 2]) -> Result<()> {
        ret(unsafe { stafeto_socketpair(domain, kind, protocol, sv.as_mut_ptr()) } as isize)
            .map(|_| ())
    }
}
