//! the real macOS signal source: the wall clock against `Instant` for sleeps,
//! `route` and `arp` for the network, and `caffeinate` for the hold.

use std::cell::RefCell;
use std::future::Future;

use super::keep_awake::{HoldEvent, KeepAwake, KeepAwakeConfig};
use super::network::{NetworkReading, NetworkTools};
use super::signals::SignalSource;
use super::sleep::{ClockSample, SleepDetector};

pub struct MacSignals {
    detector: RefCell<SleepDetector>,
    network: NetworkTools,
    keep_awake: KeepAwake,
}

impl MacSignals {
    #[must_use]
    pub fn new(network: NetworkTools, keep_awake: KeepAwakeConfig) -> Self {
        Self {
            detector: RefCell::new(SleepDetector::new()),
            network,
            keep_awake: KeepAwake::new(keep_awake),
        }
    }

    /// the source with the `MARSHAL_TEST_ROUTE`, `MARSHAL_TEST_ARP` and
    /// `MARSHAL_TEST_CAFFEINATE` programs in a debug build, else the system's.
    #[must_use]
    pub fn from_environment() -> Self {
        Self::new(
            NetworkTools::from_environment(),
            KeepAwakeConfig::from_environment(),
        )
    }
}

impl SignalSource for MacSignals {
    fn sleep_detected(&self) -> bool {
        self.detector.borrow_mut().observe(ClockSample::now())
    }

    fn read_network(&self) -> impl Future<Output = NetworkReading> {
        self.network.read()
    }

    fn hold_keep_awake(&self) -> HoldEvent {
        self.keep_awake.hold()
    }

    fn poll_keep_awake(&self) -> HoldEvent {
        self.keep_awake.poll()
    }

    fn release_keep_awake(&self) {
        self.keep_awake.release();
    }

    fn keep_awake_warning(&self) -> Option<String> {
        self.keep_awake.warning()
    }
}
