//! When a failed runtime core is started again, and when the instance gives up.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

/// The failures that count are those no older than this.
pub const WINDOW: Duration = Duration::from_secs(10 * 60);

/// The wait before the core restarts after its first, second, and third failure in
/// the window. A fourth failure in the window stops the instance.
pub const DELAYS: [Duration; 3] = [
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

/// What a failure leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    RestartAfter(Duration),
    Stop,
}

/// The core's recent failures, in a window that slides with the clock.
#[derive(Debug, Default)]
pub struct RestartPolicy {
    failures: VecDeque<SystemTime>,
}

impl RestartPolicy {
    /// Records a failure at `now` and says what follows. A failure older than the
    /// window, and so no longer counted, is forgotten first. One exactly as old as
    /// the window still counts.
    pub fn record_failure(&mut self, now: SystemTime) -> Verdict {
        self.failures.retain(|failed_at| {
            now.duration_since(*failed_at)
                .map_or(true, |age| age <= WINDOW)
        });
        self.failures.push_back(now);
        match DELAYS.get(self.failures.len().saturating_sub(1)) {
            Some(delay) => Verdict::RestartAfter(*delay),
            None => Verdict::Stop,
        }
    }

    /// Forgets every failure, as a resume does.
    pub fn clear(&mut self) {
        self.failures.clear();
    }

    /// How many failures are held, counted or not yet forgotten.
    #[must_use]
    pub fn held(&self) -> usize {
        self.failures.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
    }

    #[test]
    fn restarts_follow_after_10_30_and_60_seconds_and_a_fourth_failure_stops() {
        let mut policy = RestartPolicy::default();
        let verdicts: Vec<Verdict> = [0, 40, 100, 200]
            .iter()
            .map(|seconds| policy.record_failure(at(*seconds)))
            .collect();
        assert_eq!(
            verdicts,
            [
                Verdict::RestartAfter(Duration::from_secs(10)),
                Verdict::RestartAfter(Duration::from_secs(30)),
                Verdict::RestartAfter(Duration::from_secs(60)),
                Verdict::Stop,
            ]
        );
    }

    #[test]
    fn a_failure_older_than_ten_minutes_no_longer_counts() {
        let mut policy = RestartPolicy::default();
        for seconds in [0, 10, 20] {
            policy.record_failure(at(seconds));
        }
        assert_eq!(
            policy.record_failure(at(20 + 601)),
            Verdict::RestartAfter(DELAYS[0]),
            "the three earlier failures are all older than the window"
        );
        assert_eq!(policy.held(), 1);
    }

    #[test]
    fn the_window_slides_so_only_the_failures_inside_it_count() {
        let mut policy = RestartPolicy::default();
        policy.record_failure(at(0));
        policy.record_failure(at(300));
        policy.record_failure(at(500));
        assert_eq!(
            policy.record_failure(at(601)),
            Verdict::RestartAfter(DELAYS[2]),
            "the failure at 0 s dropped out, leaving 300, 500 and 601"
        );
    }

    #[test]
    fn a_failure_exactly_ten_minutes_old_still_counts() {
        let mut policy = RestartPolicy::default();
        for seconds in [0, 1, 2] {
            policy.record_failure(at(seconds));
        }
        assert_eq!(policy.record_failure(at(600)), Verdict::Stop);
    }

    #[test]
    fn clearing_forgets_every_failure() {
        let mut policy = RestartPolicy::default();
        for seconds in [0, 1, 2] {
            policy.record_failure(at(seconds));
        }
        policy.clear();
        assert_eq!(
            policy.record_failure(at(3)),
            Verdict::RestartAfter(DELAYS[0])
        );
    }

    #[test]
    fn a_clock_that_moved_backwards_does_not_forget_failures() {
        let mut policy = RestartPolicy::default();
        policy.record_failure(at(1000));
        policy.record_failure(at(10));
        assert_eq!(policy.held(), 2);
    }
}
