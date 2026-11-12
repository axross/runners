//! an in-process signal source for tests: the sleep, the network readings and the
//! failure of the hold are set by the test; a debug or test build only.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;

use super::keep_awake::HoldEvent;
use super::network::NetworkReading;
use super::signals::SignalSource;

#[derive(Default)]
pub struct FakeSignals {
    slept: Cell<bool>,
    readings: RefCell<VecDeque<NetworkReading>>,
    reads: Cell<u32>,
    wanted: Cell<bool>,
    failure: RefCell<Option<String>>,
    warned: Cell<bool>,
    holds: Cell<u32>,
    releases: Cell<u32>,
    rearms: Cell<u32>,
}

impl FakeSignals {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// makes the next [`SignalSource::sleep_detected`] answer true.
    pub fn sleep(&self) {
        self.slept.set(true);
    }

    /// queues readings, one per read; the last one repeats once the rest are read.
    /// With none queued a read is `NoDefaultRoute`.
    pub fn script_network(&self, readings: impl IntoIterator<Item = NetworkReading>) {
        *self.readings.borrow_mut() = readings.into_iter().collect();
    }

    /// makes every read from now on answer `reading`.
    pub fn set_network(&self, reading: NetworkReading) {
        self.script_network([reading]);
    }

    /// makes the hold fail with `warning`, or work again with `None`.
    pub fn fail_keep_awake(&self, warning: Option<&str>) {
        *self.failure.borrow_mut() = warning.map(str::to_string);
    }

    /// whether the hold is wanted and held.
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.wanted.get() && self.failure.borrow().is_none()
    }

    #[must_use]
    pub fn hold_calls(&self) -> u32 {
        self.holds.get()
    }

    #[must_use]
    pub fn release_calls(&self) -> u32 {
        self.releases.get()
    }

    /// how many times the sleep baseline was re-armed.
    #[must_use]
    pub fn rearm_calls(&self) -> u32 {
        self.rearms.get()
    }

    #[must_use]
    pub fn network_reads(&self) -> u32 {
        self.reads.get()
    }

    fn judge_hold(&self) -> HoldEvent {
        if !self.wanted.get() {
            return HoldEvent::Unchanged;
        }
        match (self.failure.borrow().clone(), self.warned.get()) {
            (Some(text), false) => {
                self.warned.set(true);
                HoldEvent::Warned(text)
            }
            (None, true) => {
                self.warned.set(false);
                HoldEvent::Recovered
            }
            _ => HoldEvent::Unchanged,
        }
    }
}

impl SignalSource for FakeSignals {
    fn sleep_detected(&self) -> bool {
        self.slept.replace(false)
    }

    fn rearm_sleep(&self) {
        self.slept.set(false);
        self.rearms.set(self.rearms.get() + 1);
    }

    fn read_network(&self) -> impl Future<Output = NetworkReading> {
        self.reads.set(self.reads.get() + 1);
        let mut readings = self.readings.borrow_mut();
        let reading = if readings.len() > 1 {
            readings.pop_front()
        } else {
            readings.front().cloned()
        };
        std::future::ready(reading.unwrap_or(NetworkReading::NoDefaultRoute))
    }

    fn hold_keep_awake(&self) -> HoldEvent {
        self.holds.set(self.holds.get() + 1);
        if self.wanted.replace(true) {
            return HoldEvent::Unchanged;
        }
        self.judge_hold()
    }

    fn poll_keep_awake(&self) -> HoldEvent {
        self.judge_hold()
    }

    fn release_keep_awake(&self) {
        self.releases.set(self.releases.get() + 1);
        self.wanted.set(false);
        self.warned.set(false);
    }

    fn keep_awake_warning(&self) -> Option<String> {
        if self.wanted.get() {
            self.failure.borrow().clone()
        } else {
            None
        }
    }
}
