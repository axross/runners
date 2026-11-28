//! the instance's signals on Windows, as the resident agent runs them: a sleep stops
//! every runner always, a change of the primary network only with
//! `stop_on_network_change` on, and the keep-awake request is held only with
//! `keep_awake` on. Both settings are read from those in effect at each tick, so a
//! change takes effect within a second.

use std::convert::Infallible;

use crate::instance::lifecycle::{Monitor, StopReason};
use crate::instance::monitor_state::MonitorState;
use crate::lifecycle_settings::{Current, Setting};
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
    /// the hold, the settings it follows and the network recorded; here the settings
    /// are required, and they also gate the stop and the network read.
    state: MonitorState<'a>,
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
            state: MonitorState::new().with_settings(settings),
        }
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
            self.state
                .keep_record(watch.as_ref().and_then(NetworkWatch::recorded));
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
        self.state
            .keep_record(watch.as_ref().and_then(NetworkWatch::recorded));
        None
    }
}

impl<S: SignalSource, K: Clock> Monitor for WindowsMonitor<'_, S, K> {
    fn begin(&self) {
        self.signals.rearm_sleep();
        self.state.forget_record();
    }

    async fn watch(&self) -> StopReason {
        let mut network = None;
        if self.reads_network() {
            let reading = self.signals.read_network().await;
            self.check(&mut network, reading);
        }
        let mut samples = 0_u32;
        loop {
            self.cadence.tick(self.clock).await;
            // a sleep is judged before the network, so a wake on another network stops
            // with the sleep's reason
            if self.signals.sleep_detected() {
                return StopReason::Slept;
            }
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
        self.cadence
            .maintain(self.clock, &self.state, &self.signals, self.log)
            .await
    }

    fn hold(&self) {
        self.state.hold(&self.signals, self.log);
    }

    fn release(&self) {
        self.state.release(&self.signals, self.log);
    }

    fn report(&self) -> Vec<(&'static str, String)> {
        self.signals
            .keep_awake_warning()
            .map(|warning| ("warning", warning))
            .into_iter()
            .collect()
    }

    fn recorded(&self) -> Option<PrimaryNetwork> {
        self.state.recorded()
    }

    async fn next_reading(&self) -> NetworkReading {
        self.cadence
            .read_after_a_check(self.clock, &self.signals)
            .await
    }
}
