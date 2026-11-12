//! sleep detection: how far the wall clock moved against how far the monotonic
//! clock moved between two samples. On macOS `Instant` is `CLOCK_UPTIME_RAW`, which
//! does not advance while the Mac sleeps, and `SystemTime` does.

use std::time::{Duration, Instant, SystemTime};

/// how often the instance samples the clocks.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// the gap between the two clocks, in either direction, above which the Mac is
/// judged to have slept; a gap of exactly this is not a sleep.
pub const SLEEP_GAP: Duration = Duration::from_secs(3);

/// both clocks at one moment.
#[derive(Debug, Clone, Copy)]
pub struct ClockSample {
    pub wall: SystemTime,
    pub monotonic: Instant,
}

impl ClockSample {
    #[must_use]
    pub fn now() -> Self {
        Self {
            wall: SystemTime::now(),
            monotonic: Instant::now(),
        }
    }
}

fn nanos(duration: Duration) -> i128 {
    i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX)
}

fn wall_movement(previous: SystemTime, current: SystemTime) -> i128 {
    match current.duration_since(previous) {
        Ok(forward) => nanos(forward),
        Err(backward) => -nanos(backward.duration()),
    }
}

/// whether the Mac slept, or its wall clock was stepped, between two samples.
#[must_use]
pub fn slept_between(previous: &ClockSample, current: &ClockSample) -> bool {
    let wall = wall_movement(previous.wall, current.wall);
    let monotonic = nanos(
        current
            .monotonic
            .saturating_duration_since(previous.monotonic),
    );
    (wall - monotonic).unsigned_abs() > SLEEP_GAP.as_nanos()
}

/// the sampler of one run of the instance: it judges each sample against the one
/// before.
#[derive(Debug, Default)]
pub struct SleepDetector {
    previous: Option<ClockSample>,
}

impl SleepDetector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// records `sample` and returns whether the Mac slept since the one before; the
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
