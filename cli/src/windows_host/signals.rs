//! the real Windows signal source behind M1's [`SignalSource`] seam: the two clocks for
//! sleeps, `netdev` for the primary network, and the keep-awake request the executable
//! supplies.

use std::cell::RefCell;

use crate::macos::network::NetworkReading;
use crate::macos::signals::SignalSource;

use super::keep_awake::{HoldEvent, KeepAwake};
use super::network::Bounded;
use super::sleep::{ClockSample, SleepDetector};

pub struct WindowsSignals {
    detector: RefCell<SleepDetector>,
    sample: fn() -> Option<ClockSample>,
    network: Bounded,
    /// none when the executable supplied no request, so nothing is held.
    keep_awake: Option<KeepAwake>,
}

impl WindowsSignals {
    #[must_use]
    pub fn new(
        sample: fn() -> Option<ClockSample>,
        network: Bounded,
        keep_awake: Option<KeepAwake>,
    ) -> Self {
        Self {
            detector: RefCell::new(SleepDetector::new()),
            sample,
            network,
            keep_awake,
        }
    }

    /// the source that reads Windows itself.
    #[cfg(windows)]
    #[must_use]
    pub fn real(keep_awake: Option<KeepAwake>) -> Self {
        Self::new(
            ClockSample::now,
            Bounded::new(
                std::sync::Arc::new(super::network::read),
                super::network::READ_LIMIT,
            ),
            keep_awake,
        )
    }
}

impl SignalSource for WindowsSignals {
    /// a sample Windows refuses counts as no sleep and leaves the baseline as it was.
    fn sleep_detected(&self) -> bool {
        (self.sample)().is_some_and(|sample| self.detector.borrow_mut().observe(sample))
    }

    fn rearm_sleep(&self) {
        *self.detector.borrow_mut() = SleepDetector::new();
    }

    async fn read_network(&self) -> NetworkReading {
        self.network.read().await
    }

    fn hold_keep_awake(&self) -> HoldEvent {
        self.keep_awake
            .as_ref()
            .map_or(HoldEvent::Unchanged, KeepAwake::hold)
    }

    fn poll_keep_awake(&self) -> HoldEvent {
        self.keep_awake
            .as_ref()
            .map_or(HoldEvent::Unchanged, KeepAwake::poll)
    }

    fn release_keep_awake(&self) {
        if let Some(keep_awake) = &self.keep_awake {
            keep_awake.release();
        }
    }

    fn keep_awake_warning(&self) -> Option<String> {
        self.keep_awake.as_ref().and_then(KeepAwake::warning)
    }
}
