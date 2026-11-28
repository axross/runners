//! what a signal monitor keeps between ticks beside its signals: whether the instance
//! wants the keep-awake hold, the settings in effect the hold follows, and the primary
//! network last recorded while running. The Mac's and Windows' monitors each hold one,
//! so a fix to how the hold follows `keep_awake` lands once.

use std::cell::{Cell, RefCell};

use crate::lifecycle_settings::{Current, Setting};
use crate::macos::keep_awake::HoldEvent;
use crate::macos::network::PrimaryNetwork;
use crate::macos::signals::SignalSource;
use crate::runtime::log::Log;

#[derive(Debug, Default)]
pub struct MonitorState<'a> {
    /// the settings in effect, which the hold follows; without them it always holds.
    settings: Option<&'a Current>,
    /// true from the instance's `hold` to its `release`, whatever the setting says.
    wanted: Cell<bool>,
    /// the primary network last recorded while running, which a resume compares with.
    recorded: RefCell<Option<PrimaryNetwork>>,
}

impl<'a> MonitorState<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// this state with its hold following `keep_awake` in `settings`.
    #[must_use]
    pub fn with_settings(self, settings: &'a Current) -> Self {
        Self {
            settings: Some(settings),
            ..self
        }
    }

    /// wants the hold, as the instance begins to run or resume.
    pub fn hold(&self, signals: &impl SignalSource, log: &dyn Log) {
        self.wanted.set(true);
        self.reconcile(signals, log);
    }

    /// no longer wants the hold, as the instance stops.
    pub fn release(&self, signals: &impl SignalSource, log: &dyn Log) {
        self.wanted.set(false);
        self.reconcile(signals, log);
    }

    /// holds while the hold is wanted and `keep_awake` is on, and releases otherwise.
    pub fn reconcile(&self, signals: &impl SignalSource, log: &dyn Log) {
        let keep_awake = self
            .settings
            .is_none_or(|settings| settings.is_on(Setting::KeepAwake));
        if self.wanted.get() && keep_awake {
            note(signals.hold_keep_awake(), log);
            note(signals.poll_keep_awake(), log);
        } else {
            signals.release_keep_awake();
        }
    }

    /// records `network` as the one last seen while running; none leaves the record as
    /// it is.
    pub fn keep_record(&self, network: Option<&PrimaryNetwork>) {
        if let Some(network) = network {
            *self.recorded.borrow_mut() = Some(network.clone());
        }
    }

    /// forgets the record, as a new run begins.
    pub fn forget_record(&self) {
        *self.recorded.borrow_mut() = None;
    }

    /// the primary network last recorded.
    #[must_use]
    pub fn recorded(&self) -> Option<PrimaryNetwork> {
        self.recorded.borrow().clone()
    }
}

fn note(event: HoldEvent, log: &dyn Log) {
    match event {
        HoldEvent::Unchanged => {}
        HoldEvent::Warned(text) => log.warn(&text),
        HoldEvent::Recovered => log.info("Keep-awake is held again."),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::lifecycle_settings::InEffect;
    use crate::macos::fake::FakeSignals;
    use crate::runtime::fake::MemoryLog;
    use crate::validate::Lifecycle;

    fn keep_awake(value: Option<bool>) -> Current {
        let current = Current::default();
        current.set(InEffect::new(Lifecycle {
            keep_awake: value,
            ..Lifecycle::default()
        }));
        current
    }

    fn network(interface: &str) -> PrimaryNetwork {
        PrimaryNetwork {
            interface: interface.to_string(),
            router: Some(Ipv4Addr::new(192, 0, 2, 1)),
            hardware: None,
        }
    }

    #[test]
    fn without_settings_a_wanted_hold_is_held_and_a_release_ends_it() {
        let (state, signals, log) = (
            MonitorState::new(),
            FakeSignals::new(),
            MemoryLog::default(),
        );
        state.hold(&signals, &log);
        assert!(signals.is_held());
        state.release(&signals, &log);
        assert!(!signals.is_held());
    }

    #[test]
    fn a_hold_nobody_wants_is_never_taken_whatever_the_setting_says() {
        let settings = keep_awake(Some(true));
        let (state, signals, log) = (
            MonitorState::new().with_settings(&settings),
            FakeSignals::new(),
            MemoryLog::default(),
        );
        state.reconcile(&signals, &log);
        assert_eq!(signals.hold_calls(), 0);
        assert!(!signals.is_held());
    }

    #[test]
    fn the_hold_follows_keep_awake_at_each_reconcile_while_it_is_wanted() {
        let settings = keep_awake(Some(false));
        let (state, signals, log) = (
            MonitorState::new().with_settings(&settings),
            FakeSignals::new(),
            MemoryLog::default(),
        );
        state.hold(&signals, &log);
        assert!(!signals.is_held(), "keep_awake is off");
        settings.set(InEffect::new(Lifecycle {
            keep_awake: Some(true),
            ..Lifecycle::default()
        }));
        state.reconcile(&signals, &log);
        assert!(
            signals.is_held(),
            "keep_awake turned on takes the wanted hold"
        );
        settings.set(InEffect::new(Lifecycle {
            keep_awake: Some(false),
            ..Lifecycle::default()
        }));
        state.reconcile(&signals, &log);
        assert!(!signals.is_held(), "keep_awake turned off releases it");
        state.release(&signals, &log);
        settings.set(InEffect::new(Lifecycle::default()));
        state.reconcile(&signals, &log);
        assert!(!signals.is_held(), "a hold no longer wanted stays released");
    }

    #[test]
    fn the_hold_is_off_while_no_setting_is_in_effect() {
        let settings = Current::default();
        let (state, signals, log) = (
            MonitorState::new().with_settings(&settings),
            FakeSignals::new(),
            MemoryLog::default(),
        );
        state.hold(&signals, &log);
        assert_eq!(signals.hold_calls(), 0);
    }

    #[test]
    fn a_lost_hold_warns_once_and_a_recovered_one_says_so() {
        let (state, signals, log) = (
            MonitorState::new(),
            FakeSignals::new(),
            MemoryLog::default(),
        );
        signals.fail_keep_awake(Some("no caffeinate"));
        state.hold(&signals, &log);
        state.reconcile(&signals, &log);
        signals.fail_keep_awake(None);
        state.reconcile(&signals, &log);
        assert_eq!(
            log.lines(),
            ["WARN: no caffeinate", "INFO: Keep-awake is held again."]
        );
    }

    #[test]
    fn the_record_keeps_the_last_network_until_a_new_run_forgets_it() {
        let state = MonitorState::new();
        assert_eq!(state.recorded(), None);
        state.keep_record(Some(&network("en0")));
        state.keep_record(None);
        assert_eq!(state.recorded(), Some(network("en0")));
        state.keep_record(Some(&network("en1")));
        assert_eq!(state.recorded(), Some(network("en1")));
        state.forget_record();
        assert_eq!(state.recorded(), None);
    }
}
