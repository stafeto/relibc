#[cfg(not(stafeto))]
use crate::header::{
    sys_socket::{
        connect,
        constants::{AF_UNIX, SOCK_CLOEXEC, SOCK_DGRAM},
        sockaddr, socket,
    },
    sys_un::sockaddr_un,
    unistd::close,
};
use crate::{
    error::{Errno, Result},
    fs::File,
    io::BufWriter,
    platform::ERRNO,
};

use super::logger::LogSink;

/// Unix Domain Socket connected to /dev/log.
pub struct LogFile(BufWriter<File>);

impl LogSink for LogFile {
    type Sink = BufWriter<File>;

    // No log daemon runs on stafeto: the journal is a file the messages are
    // appended to, /var/log/messages. Where the file cannot be opened the
    // messages go to the standard error, as on a system whose log is the
    // terminal.
    #[cfg(stafeto)]
    fn open() -> Result<Self>
    where
        Self: Sized,
    {
        use crate::header::fcntl::{O_APPEND, O_CLOEXEC, O_WRONLY};
        // The refusals on the way to the file that opens are no error.
        let saved = ERRNO.get();
        let file = File::create(c"/var/log/messages".into(), O_WRONLY | O_APPEND | O_CLOEXEC, 0o644)
            .or_else(|_| {
                let fd = crate::header::unistd::dup(2);
                if fd < 0 {
                    Err(Errno(ERRNO.get()))
                } else {
                    Ok(File::new(fd))
                }
            })?;
        ERRNO.set(saved);
        Ok(Self(BufWriter::new(file)))
    }

    #[cfg(not(stafeto))]
    fn open() -> Result<Self>
    where
        Self: Sized,
    {
        let log_addr = {
            let path = c"/dev/log";
            let mut sockaddr: sockaddr_un = unsafe { core::mem::zeroed() };
            sockaddr.sun_family = u16::try_from(AF_UNIX).expect("constant value within bounds");
            unsafe {
                core::ptr::copy_nonoverlapping(
                    path.as_ptr(),
                    sockaddr.sun_path.as_mut_ptr(),
                    path.count_bytes(),
                )
            };
            path
        };
        let log_fd = {
            let result = unsafe { socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0) };
            if result > 0 {
                Ok(result)
            } else {
                Err(Errno(ERRNO.get()))
            }
        }?;

        // SAFETY:
        // * connect handles invalid descriptors.
        // * log_addr is a sockaddr_un so the size is correct.
        if unsafe {
            connect(
                log_fd,
                (&raw const log_addr).cast::<sockaddr>(),
                size_of::<sockaddr_un>() as u32,
            ) < 0
        } {
            // In case close sets ERRNO.
            let e = ERRNO.get();
            close(log_fd);
            return Err(Errno(e));
        }

        Ok(Self(BufWriter::new(File::new(log_fd))))
    }

    #[inline(always)]
    fn writer(&mut self) -> &mut Self::Sink {
        &mut self.0
    }
}
