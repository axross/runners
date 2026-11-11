//! the instance's signals on axmc: the sleep and network watch that stops every runner,
//! and the keep-awake hold, driven by a [`SignalSource`] on a cadence. The Tart backend
//! waits at the [`NetworkGate`] this monitor keeps until a primary network is recorded.

use std::cell::Cell;
use std::convert::Infallible;
use std::time::Duration;

use crate::instance::lifecycle::{Monitor, StopReason};
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;

use super::gate::NetworkGate;
use super::keep_awake::HoldEvent;
use super::network::NetworkReading;
use super::signals::SignalSource;
use super::sleep::SAMPLE_INTERVAL;
use super::watch::{CHECK_INTERVAL, CHECKS_IN_A_ROW, NetworkWatch, WatchOutcome};

/// the `status` field that says the instance waits for a primary network.
pub const NETWORK_FIELD: &str = "network";

/// what `status` shows while the instance waits for a primary network.
pub const WAITING_FOR_A_NETWORK: &str = "waiting for a network (no IPv4 default route)";

/// how often the monitor samples the clocks, and how many samples make one network
/// check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    pub sample: Duration,
    pub samples_per_check: u32,
}

impl Default for Cadence {
    fn default() -> Self {
        let per_check = CHECK_INTERVAL.as_millis() / SAMPLE_INTERVAL.as_millis();
        Self {
            sample: SAMPLE_INTERVAL,
            samples_per_check: u32::try_from(per_check).unwrap_or(1),
        }
    }
}

impl Cadence {
    /// the cadence for a signal source: a scripted one, which only a debug build has,
    /// keeps the real ratio of samples to checks in a fiftieth of the time.
    #[must_use]
    pub fn for_source(scripted: bool) -> Self {
        let real = Self::default();
        if scripted {
            Self {
                sample: real.sample / 50,
                ..real
            }
        } else {
            real
        }
    }
}

/// clears the flag when the watch ends, however it ends.
struct Waiting<'a>(&'a Cell<bool>);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

pub struct SignalMonitor<'a, S, K> {
    signals: S,
    clock: &'a K,
    log: &'a dyn Log,
    gate: NetworkGate,
    cadence: Cadence,
    waiting: Cell<bool>,
}

impl<'a, S: SignalSource, K: Clock> SignalMonitor<'a, S, K> {
    #[must_use]
    pub fn new(
        signals: S,
        clock: &'a K,
        log: &'a dyn Log,
        gate: NetworkGate,
        cadence: Cadence,
    ) -> Self {
        Self {
            signals,
            clock,
            log,
            gate,
            cadence,
            waiting: Cell::new(false),
        }
    }

    fn note(&self, event: HoldEvent) {
        match event {
            HoldEvent::Unchanged => {}
            HoldEvent::Warned(text) => self.log.warn(&text),
            HoldEvent::Recovered => self.log.info("Keep-awake is held again."),
        }
    }

    /// one tick of the cadence; it always yields, so a clock that does not wait cannot
    /// starve the instance's other work.
    async fn tick(&self) {
        self.clock.sleep(self.cadence.sample).await;
        tokio::task::yield_now().await;
    }

    /// reads the network the instance has just begun to run on: a primary network is
    /// recorded at once and opens the gate, and none leaves it closed until one has
    /// held.
    async fn record(&self) -> NetworkWatch {
        let watch = NetworkWatch::begin(self.signals.read_network().await);
        match watch.recorded() {
            Some(network) => {
                self.waiting.set(false);
                self.gate.open();
                self.log.info(&format!(
                    "The primary network is on interface {}.",
                    network.interface
                ));
            }
            None => {
                self.waiting.set(true);
                self.log.warn(&format!(
                    "No IPv4 default route, so no VM is cloned until a primary network has \
                     held for {CHECKS_IN_A_ROW} checks."
                ));
            }
        }
        watch
    }

    fn observe(&self, watch: &mut NetworkWatch, reading: NetworkReading) -> Option<StopReason> {
        match watch.observe(reading) {
            WatchOutcome::Steady | WatchOutcome::Waiting => None,
            WatchOutcome::Differing => {
                self.log.warn(
                    "The primary network differs from its record; if the next check differs \
                     too, every runner is stopped.",
                );
                None
            }
            WatchOutcome::Recorded(network) => {
                self.waiting.set(false);
                self.gate.open();
                self.log.info(&format!(
                    "A primary network has held, on interface {}; VMs are cloned again.",
                    network.interface
                ));
                None
            }
            WatchOutcome::Changed(change) => Some(StopReason::NetworkChanged(change)),
        }
    }
}

impl<S: SignalSource, K: Clock> Monitor for SignalMonitor<'_, S, K> {
    fn begin(&self) {
        self.signals.rearm_sleep();
        self.gate.close();
    }

    async fn watch(&self) -> StopReason {
        let _waiting = Waiting(&self.waiting);
        let mut network = self.record().await;
        let mut samples = 0_u32;
        loop {
            self.tick().await;
            if self.signals.sleep_detected() {
                return StopReason::Slept;
            }
            self.note(self.signals.poll_keep_awake());
            samples += 1;
            if samples < self.cadence.samples_per_check {
                continue;
            }
            samples = 0;
            let reading = self.signals.read_network().await;
            if let Some(reason) = self.observe(&mut network, reading) {
                return reason;
            }
        }
    }

    async fn maintain(&self) -> Infallible {
        loop {
            self.tick().await;
            self.note(self.signals.poll_keep_awake());
        }
    }

    fn hold(&self) {
        self.note(self.signals.hold_keep_awake());
    }

    fn release(&self) {
        self.signals.release_keep_awake();
    }

    fn report(&self) -> Vec<(&'static str, String)> {
        let mut fields = Vec::new();
        if self.waiting.get() {
            fields.push((NETWORK_FIELD, WAITING_FOR_A_NETWORK.to_string()));
        }
        if let Some(warning) = self.signals.keep_awake_warning() {
            fields.push(("warning", warning));
        }
        fields
    }
}
