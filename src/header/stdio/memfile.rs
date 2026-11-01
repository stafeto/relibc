//! The content of a stream over memory (`fmemopen`, `open_memstream`,
//! `open_wmemstream`): a buffer of the process with a position, behind a
//! descriptor number that no file has. The numbers are below -1 (-2 for the
//! first), so `fileno` of such a stream is -1, and the descriptor table of the
//! program never holds one. `fs::File` sends the reads, writes, seeks and the
//! close of such a number here; no file service is asked and no space of a
//! file system is used.

use alloc::vec::Vec;

use crate::{
    error::Errno,
    header::{
        errno::{EBADF, EINVAL, ENOMEM},
        unistd::{SEEK_CUR, SEEK_END, SEEK_SET},
    },
    platform::types::{c_int, off_t},
    sync::Mutex,
};

struct MemFile {
    data: Vec<u8>,
    pos: usize,
    /// Every write goes to the end (mode "a").
    append: bool,
}

static TABLE: Mutex<Vec<Option<MemFile>>> = Mutex::new(Vec::new());

/// Whether `fd` is the number of a stream's content over memory.
#[inline]
pub fn is_memory(fd: c_int) -> bool {
    fd <= -2
}

fn place(fd: c_int) -> usize {
    (-2 - fd) as usize
}

/// Makes the content of a stream: `data` with the position `pos`. The number
/// is below -1.
pub fn create(data: Vec<u8>, pos: usize, append: bool) -> Result<c_int, Errno> {
    let mut table = TABLE.lock();
    let file = MemFile { data, pos, append };
    let at = match table.iter().position(Option::is_none) {
        Some(at) => at,
        None => {
            table.try_reserve(1).map_err(|_| Errno(ENOMEM))?;
            table.push(None);
            table.len() - 1
        }
    };
    table[at] = Some(file);
    Ok(-2 - at as c_int)
}

/// Runs `f` on the content and the position of `fd`.
pub fn with<R>(fd: c_int, f: impl FnOnce(&mut Vec<u8>, &mut usize) -> R) -> Result<R, Errno> {
    let mut table = TABLE.lock();
    match table.get_mut(place(fd)).and_then(Option::as_mut) {
        Some(file) if is_memory(fd) => Ok(f(&mut file.data, &mut file.pos)),
        _ => Err(Errno(EBADF)),
    }
}

pub fn read(fd: c_int, buf: &mut [u8]) -> Result<usize, Errno> {
    with(fd, |data, pos| {
        let from = (*pos).min(data.len());
        let n = buf.len().min(data.len() - from);
        buf[..n].copy_from_slice(&data[from..from + n]);
        *pos = from + n;
        n
    })
}

pub fn write(fd: c_int, buf: &[u8]) -> Result<usize, Errno> {
    let mut table = TABLE.lock();
    let Some(Some(file)) = table.get_mut(place(fd)).filter(|_| is_memory(fd)) else {
        return Err(Errno(EBADF));
    };
    if file.append {
        file.pos = file.data.len();
    }
    let end = file.pos.checked_add(buf.len()).ok_or(Errno(EINVAL))?;
    if end > file.data.len() {
        // A write past the end leaves zeroes in the gap, as a file does.
        file.data.try_reserve(end - file.data.len()).map_err(|_| Errno(ENOMEM))?;
        file.data.resize(end, 0);
    }
    file.data[file.pos..end].copy_from_slice(buf);
    file.pos = end;
    Ok(buf.len())
}

pub fn seek(fd: c_int, off: off_t, whence: c_int) -> Result<off_t, Errno> {
    with(fd, |data, pos| {
        let base = match whence {
            SEEK_SET => 0,
            SEEK_CUR => *pos as i64,
            SEEK_END => data.len() as i64,
            _ => return Err(Errno(EINVAL)),
        };
        match base.checked_add(off as i64) {
            Some(to) if to >= 0 => {
                *pos = to as usize;
                Ok(to as off_t)
            }
            _ => Err(Errno(EINVAL)),
        }
    })?
}

pub fn close(fd: c_int) {
    let mut table = TABLE.lock();
    if let Some(slot) = table.get_mut(place(fd)).filter(|_| is_memory(fd)) {
        *slot = None;
    }
}
