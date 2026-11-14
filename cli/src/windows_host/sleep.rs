//! sleep detection on Windows. `Instant` reads `QueryPerformanceCounter`, which counts
//! through standby and hibernation, and `QueryUnbiasedInterruptTime` does not, so the
//! two drift apart across a sleep. Modern standby pauses desktop programs instead,
//! which shows as a long gap between samples.

use std::time::{Duration, Instant};

pub use crate::macos::sleep::{SAMPLE_INTERVAL, SLEEP_GAP};

/// how far the monotonic clock may move between two samples taken a second apart
/// before the agent counts itself paused; a stall this long is counted too.
pub const PAUSE: Duration = Duration::from_secs(30);

/// both clocks at one moment.
#[derive(Debug, Clone, Copy)]
pub struct ClockSample {
    pub monotonic: Instant,
    /// `QueryUnbiasedInterruptTime`, which stops while Windows sleeps.
    pub unbiased: Duration,
}

impl ClockSample {
    /// both clocks now, or none when Windows refuses the unbiased one.
    #[cfg(windows)]
    #[must_use]
    pub fn now() -> Option<Self> {
        let ticks = winsafe::QueryUnbiasedInterruptTime().ok()?;
        Some(Self {
            monotonic: Instant::now(),
            unbiased: Duration::from_nanos(ticks.saturating_mul(100)),
        })
    }
}

/// whether Windows slept, or the agent was paused, between two samples.
#[must_use]
pub fn slept_between(previous: &ClockSample, current: &ClockSample) -> bool {
    let monotonic = current
        .monotonic
        .saturating_duration_since(previous.monotonic);
    let unbiased = current.unbiased.saturating_sub(previous.unbiased);
    monotonic.saturating_sub(unbiased) > SLEEP_GAP || monotonic > PAUSE
}

/// the sampler of one run: it judges each sample against the one before.
#[derive(Debug, Default)]
pub struct SleepDetector {
    previous: Option<ClockSample>,
}

impl SleepDetector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// records `sample` and returns whether Windows slept since the one before; the
    /// first sample only sets the baseline.
    pub fn observe(&mut self, sample: ClockSample) -> bool {
        let slept = self
            .previous
            .as_ref()
            .is_some_and(|previous| slept_between(previous, &sample));
        self.previous = Some(sample);
        slept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(base: &ClockSample, monotonic: u64, unbiased_millis: u64) -> ClockSample {
        ClockSample {
            monotonic: base.monotonic + Duration::from_millis(monotonic),
            unbiased: base.unbiased + Duration::from_millis(unbiased_millis),
        }
    }

    fn base() -> ClockSample {
        ClockSample {
            monotonic: Instant::now(),
            unbiased: Duration::from_secs(1_000),
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_answers_both_clocks_and_the_unbiased_one_moves_forward() {
        let first = ClockSample::now();
        std::thread::sleep(Duration::from_millis(50));
        let second = ClockSample::now();
        assert!(
            first
                .zip(second)
                .is_some_and(|(first, second)| second.unbiased > first.unbiased
                    && !slept_between(&first, &second)),
            "Windows refused the unbiased clock, or it did not move forward"
        );
    }

    #[test]
    fn the_unbiased_clock_falling_behind_by_more_than_three_seconds_is_a_sleep() {
        let base = base();
        assert!(!slept_between(&base, &after(&base, 4_000, 1_000)));
        assert!(slept_between(&base, &after(&base, 4_001, 1_000)));
        assert!(!slept_between(&base, &after(&base, 1_000, 1_000)));
        assert!(!slept_between(&base, &after(&base, 1_000, 9_000)));
    }

    #[test]
    fn a_pause_of_more_than_thirty_seconds_is_a_sleep_even_with_both_clocks_moving() {
        let base = base();
        assert!(!slept_between(&base, &after(&base, 30_000, 30_000)));
        assert!(slept_between(&base, &after(&base, 30_001, 30_001)));
    }

    #[test]
    fn the_first_sample_only_sets_the_baseline() {
        let base = base();
        let mut detector = SleepDetector::new();
        assert!(!detector.observe(after(&base, 0, 0)));
        assert!(detector.observe(after(&base, 10_000, 1_000)));
        assert!(!detector.observe(after(&base, 11_000, 2_000)));
    }
}
