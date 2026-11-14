//! the instance's signals on Windows, as the resident agent runs them: a sleep stops
//! every runner always, a change of the primary network only with
//! `stop_on_network_change` on, and the keep-awake request is held only with
//! `keep_awake` on. Both settings are read from those in effect at each tick, so a
//! change takes effect within a second.

use std::cell::{Cell, RefCell};
use std::convert::Infallible;

use crate::instance::lifecycle::{Monitor, StopReason};
use crate::lifecycle_settings::{Current, Setting};
use crate::macos::keep_awake::HoldEvent;
use crate::macos::monitor::Cadence;
use crate::macos::network::{NetworkReading, PrimaryNetwork};
use crate::macos::signals::SignalSource;
use crate::macos::watch::{NetworkWatch, WatchOutcome};
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;

pub struct WindowsMonitor<'a, S, K> {
    signals: S,
    settings: &'a Current,
    clock: &'a K,
    log: &'a dyn Log,
    cadence: Cadence,
    /// true from the agent's `hold` to its `release`, whatever the setting says.
    wanted: Cell<bool>,
    /// the primary network last recorded while running, which a resume compares with.
    recorded: RefCell<Option<PrimaryNetwork>>,
}

impl<'a, S: SignalSource, K: Clock> WindowsMonitor<'a, S, K> {
    #[must_use]
    pub fn new(
        signals: S,
        settings: &'a Current,
        clock: &'a K,
        log: &'a dyn Log,
        cadence: Cadence,
    ) -> Self {
        Self {
            signals,
            settings,
            clock,
            log,
            cadence,
            wanted: Cell::new(false),
            recorded: RefCell::new(None),
        }
    }

    fn note(&self, event: HoldEvent) {
        match event {
            HoldEvent::Unchanged => {}
            HoldEvent::Warned(text) => self.log.warn(&text),
            HoldEvent::Recovered => self.log.info("Keep-awake is held again."),
        }
    }

    /// holds the request while it is wanted and `keep_awake` is on, and ends it
    /// otherwise.
    fn reconcile(&self) {
        if self.wanted.get() && self.settings.is_on(Setting::KeepAwake) {
            self.note(self.signals.hold_keep_awake());
            self.note(self.signals.poll_keep_awake());
        } else {
            self.signals.release_keep_awake();
        }
    }

    async fn tick(&self) {
        self.clock.sleep(self.cadence.sample).await;
        tokio::task::yield_now().await;
    }

    fn stops_on_change(&self) -> bool {
        self.settings.is_on(Setting::StopOnNetworkChange)
    }

    /// whether the network is read at all: to stop on a change, or to know where to
    /// resume.
    fn reads_network(&self) -> bool {
        self.stops_on_change() || self.settings.is_on(Setting::ResumeAfterStop)
    }

    /// judges one reading; a change is the stop's reason when the setting asks for it,
    /// and otherwise the new network is recorded afresh.
    fn check(
        &self,
        watch: &mut Option<NetworkWatch>,
        reading: NetworkReading,
    ) -> Option<StopReason> {
        let Some(current) = watch.as_mut() else {
            *watch = Some(NetworkWatch::begin(reading));
            self.keep_record(watch.as_ref());
            return None;
        };
        let outcome = current.observe(reading.clone());
        match outcome {
            WatchOutcome::Changed(change) if self.stops_on_change() => {
                return Some(StopReason::NetworkChanged(change));
            }
            WatchOutcome::Changed(_) => *watch = Some(NetworkWatch::begin(reading)),
            WatchOutcome::Differing if self.stops_on_change() => self.log.warn(
                "The primary network differs from its record; if the next check differs too, \
                 every runner is stopped.",
            ),
            _ => {}
        }
        self.keep_record(watch.as_ref());
        None
    }

    fn keep_record(&self, watch: Option<&NetworkWatch>) {
        if let Some(network) = watch.and_then(NetworkWatch::recorded) {
            *self.recorded.borrow_mut() = Some(network.clone());
        }
    }
}

impl<S: SignalSource, K: Clock> Monitor for WindowsMonitor<'_, S, K> {
    fn begin(&self) {
        self.signals.rearm_sleep();
        *self.recorded.borrow_mut() = None;
    }

    async fn watch(&self) -> StopReason {
        let mut network = None;
        if self.reads_network() {
            let reading = self.signals.read_network().await;
            self.check(&mut network, reading);
        }
        let mut samples = 0_u32;
        loop {
            self.tick().await;
            // a sleep is judged before the network, so a wake on another network stops
            // with the sleep's reason
            if self.signals.sleep_detected() {
                return StopReason::Slept;
            }
            self.reconcile();
            samples += 1;
            if samples < self.cadence.samples_per_check {
                continue;
            }
            samples = 0;
            if !self.reads_network() {
                network = None;
                continue;
            }
            let reading = self.signals.read_network().await;
            if let Some(reason) = self.check(&mut network, reading) {
                return reason;
            }
        }
    }

    async fn maintain(&self) -> Infallible {
        loop {
            self.tick().await;
            self.reconcile();
        }
    }

    fn hold(&self) {
        self.wanted.set(true);
        self.reconcile();
    }

    fn release(&self) {
        self.wanted.set(false);
        self.reconcile();
    }

    fn report(&self) -> Vec<(&'static str, String)> {
        self.signals
            .keep_awake_warning()
            .map(|warning| ("warning", warning))
            .into_iter()
            .collect()
    }

    fn recorded(&self) -> Option<PrimaryNetwork> {
        self.recorded.borrow().clone()
    }

    async fn next_reading(&self) -> NetworkReading {
        for _ in 0..self.cadence.samples_per_check {
            self.tick().await;
        }
        self.signals.read_network().await
    }
}
