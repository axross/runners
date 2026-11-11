//! the time as the machine tells it.

use std::time::{SystemTime, UNIX_EPOCH};

use marshal_view::Clock;

use crate::seam::TimeSource;

/// the system clock, with the local offset from UTC read by `utc_offset_minutes` each
/// time, so a change of daylight saving is followed.
pub struct WallClock {
    utc_offset_minutes: fn() -> i32,
}

impl WallClock {
    #[must_use]
    pub fn new(utc_offset_minutes: fn() -> i32) -> Self {
        Self { utc_offset_minutes }
    }
}

impl TimeSource for WallClock {
    fn clock(&self) -> Clock {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        Clock::new(now_ms, (self.utc_offset_minutes)())
    }
}
