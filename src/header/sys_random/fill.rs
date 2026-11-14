//! The loop that fills a buffer from a read that may come back short. It
//! uses nothing but `core`, so that its tests run on the host:
//! `rustc --edition 2024 --test src/header/sys_random/fill.rs && ./fill`.

/// Fills `buf` from `read`, which may give fewer bytes than asked: the loop
/// asks again for the rest. An error that `retry` accepts (a read that a
/// signal handler ended) is asked again; any other error ends the fill with
/// that error, and so do several reads in a row that give nothing, with
/// `empty`.
pub(crate) fn fill_with<E>(
    buf: &mut [u8],
    mut read: impl FnMut(&mut [u8]) -> Result<usize, E>,
    retry: impl Fn(&E) -> bool,
    empty: E,
) -> Result<(), E> {
    let mut filled = 0;
    let mut empty_reads = 0;
    while filled < buf.len() {
        match read(&mut buf[filled..]) {
            Ok(0) => {
                empty_reads += 1;
                if empty_reads == 4 {
                    return Err(empty);
                }
            }
            Ok(n) => {
                empty_reads = 0;
                filled += n.min(buf.len() - filled);
            }
            Err(e) if retry(&e) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::fill_with;

    const EINTR: i32 = 4;
    const ENOSYS: i32 = 38;
    const EIO: i32 = 5;

    #[test]
    fn short_reads_are_continued() {
        // Three bytes at a time, with a signal in the middle.
        let mut calls = 0;
        let mut seq = 0u8;
        let mut buf = [0u8; 8];
        let r = fill_with(
            &mut buf,
            |rest| {
                calls += 1;
                if calls == 2 {
                    return Err(EINTR);
                }
                let n = rest.len().min(3);
                for b in rest[..n].iter_mut() {
                    seq += 1;
                    *b = seq;
                }
                Ok(n)
            },
            |e| *e == EINTR,
            EIO,
        );
        assert_eq!(r, Ok(()));
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(calls, 4);
    }

    #[test]
    fn real_error_after_a_short_read_fails() {
        let mut buf = [0u8; 8];
        let mut calls = 0;
        let r = fill_with(
            &mut buf,
            |rest| {
                calls += 1;
                if calls == 1 {
                    rest[..2].fill(9);
                    Ok(2)
                } else {
                    Err(ENOSYS)
                }
            },
            |e| *e == EINTR,
            EIO,
        );
        assert_eq!(r, Err(ENOSYS));
    }

    #[test]
    fn reads_of_nothing_fail() {
        let mut buf = [0u8; 4];
        assert_eq!(
            fill_with(&mut buf, |_| Ok(0), |e: &i32| *e == EINTR, EIO),
            Err(EIO)
        );
    }
}
