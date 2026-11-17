//! The time left of a timer, as `timer_gettime` and `getitimer` show it.
//! Kept free of the rest of the library so that it can be tested on the host
//! (`rustc --test src/header/time/timer_left.rs`).

/// The time left (ns) of a timer at `at`, on its clock. 0 means disarmed, so
/// an armed timer never reads 0. When the expiry is past and the helper has
/// not yet counted it (a stall of the host or of the process, the helper
/// waiting for the processor), a periodic timer shows the time left in the
/// current interval, the one the helper will set when it runs; a one-shot
/// timer shows 1 ns, since its notice is on the way (Linux does the same
/// for an interval timer and for a timer with a signal), except a one-shot
/// timer with no notice (`quiet`), which reads 0 as soon as it has expired.
pub(crate) fn time_left(armed: bool, next: u64, interval: u64, at: u64, quiet: bool) -> u64 {
    if !armed {
        return 0;
    }
    if at < next {
        return next - at;
    }
    if interval > 0 {
        interval - (at - next) % interval
    } else if quiet {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::time_left;

    #[test]
    fn disarmed_reads_zero() {
        assert_eq!(time_left(false, 100, 10, 50, false), 0);
        assert_eq!(time_left(false, 100, 10, 500, false), 0);
    }

    #[test]
    fn future_deadline_is_the_time_to_it() {
        assert_eq!(time_left(true, 100, 10, 60, false), 40);
        assert_eq!(time_left(true, 100, 0, 99, false), 1);
    }

    #[test]
    fn periodic_with_a_passed_deadline_shows_the_rest_of_the_interval() {
        // 30 ms interval, the deadline passed 5 ms ago, then 65 ms ago.
        assert_eq!(time_left(true, 1_000, 30, 1_005, false), 25);
        assert_eq!(time_left(true, 1_000, 30, 1_065, false), 25);
        // On an expiry boundary a whole interval is left.
        assert_eq!(time_left(true, 1_000, 30, 1_000, false), 30);
        assert_eq!(time_left(true, 1_000, 30, 1_060, false), 30);
    }

    #[test]
    fn periodic_never_reads_zero() {
        for at in 0..500u64 {
            assert!(time_left(true, 100, 7, at, false) > 0);
            assert!(time_left(true, 100, 7, at, true) > 0);
        }
    }

    #[test]
    fn one_shot_with_a_passed_deadline_is_not_zero_while_armed() {
        assert_eq!(time_left(true, 100, 0, 100, false), 1);
        assert_eq!(time_left(true, 100, 0, 9_999, false), 1);
    }

    #[test]
    fn one_shot_without_notice_reads_zero_once_expired() {
        assert_eq!(time_left(true, 100, 0, 100, true), 0);
        assert_eq!(time_left(true, 100, 0, 50, true), 50);
    }
}
