//! the settings of `[lifecycle]` as the agent applies them: each setting's value in
//! effect, its Windows default when the file leaves it unset, and the write of one
//! setting through the configuration's write path. `docs/operations/cli.md`
//! describes what each setting does.

pub mod write;

use std::cell::Cell;

use crate::validate::{Key, Lifecycle};

/// one setting of `[lifecycle]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Setting {
    StartAtLogin,
    StartRunnersOnLaunch,
    KeepAwake,
    StopOnNetworkChange,
    ResumeAfterStop,
}

impl Setting {
    pub const ALL: [Self; 5] = [
        Self::StartAtLogin,
        Self::StartRunnersOnLaunch,
        Self::KeepAwake,
        Self::StopOnNetworkChange,
        Self::ResumeAfterStop,
    ];

    /// the setting's key in `[lifecycle]`.
    #[must_use]
    pub fn key(self) -> Key {
        match self {
            Self::StartAtLogin => Key::StartAtLogin,
            Self::StartRunnersOnLaunch => Key::StartRunnersOnLaunch,
            Self::KeepAwake => Key::KeepAwake,
            Self::StopOnNetworkChange => Key::StopOnNetworkChange,
            Self::ResumeAfterStop => Key::ResumeAfterStop,
        }
    }

    /// the value an unset setting takes on Windows.
    #[must_use]
    pub fn windows_default(self) -> bool {
        match self {
            Self::StartAtLogin | Self::ResumeAfterStop => false,
            Self::StartRunnersOnLaunch | Self::KeepAwake | Self::StopOnNetworkChange => true,
        }
    }

    /// the value the file gives the setting, or none when it leaves it unset.
    #[must_use]
    pub fn read(self, lifecycle: &Lifecycle) -> Option<bool> {
        match self {
            Self::StartAtLogin => lifecycle.start_at_login,
            Self::StartRunnersOnLaunch => lifecycle.start_runners_on_launch,
            Self::KeepAwake => lifecycle.keep_awake,
            Self::StopOnNetworkChange => lifecycle.stop_on_network_change,
            Self::ResumeAfterStop => lifecycle.resume_after_stop,
        }
    }

    fn assign(self, lifecycle: &mut Lifecycle, value: Option<bool>) {
        let slot = match self {
            Self::StartAtLogin => &mut lifecycle.start_at_login,
            Self::StartRunnersOnLaunch => &mut lifecycle.start_runners_on_launch,
            Self::KeepAwake => &mut lifecycle.keep_awake,
            Self::StopOnNetworkChange => &mut lifecycle.stop_on_network_change,
            Self::ResumeAfterStop => &mut lifecycle.resume_after_stop,
        };
        *slot = value;
    }
}

/// one setting's value in effect, and whether the file sets it or it takes its
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value {
    pub on: bool,
    pub defaulted: bool,
}

/// the settings in effect on Windows: the values of one `[lifecycle]` table, with
/// each unset one at its default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InEffect {
    lifecycle: Lifecycle,
}

impl InEffect {
    /// the settings in effect for the table a configuration holds.
    #[must_use]
    pub fn windows(lifecycle: Lifecycle) -> Self {
        Self { lifecycle }
    }

    /// `setting`'s value in effect.
    #[must_use]
    pub fn get(&self, setting: Setting) -> Value {
        match setting.read(&self.lifecycle) {
            Some(on) => Value {
                on,
                defaulted: false,
            },
            None => Value {
                on: setting.windows_default(),
                defaulted: true,
            },
        }
    }

    /// whether `setting` is on in effect.
    #[must_use]
    pub fn is_on(&self, setting: Setting) -> bool {
        self.get(setting).on
    }

    /// the settings after an accepted write of `value` to `setting`, which leaves the
    /// others as they are.
    #[must_use]
    pub fn with(mut self, setting: Setting, value: Option<bool>) -> Self {
        setting.assign(&mut self.lifecycle, value);
        self
    }
}

/// the settings in effect as the agent keeps them, which the Windows monitor reads;
/// none while the agent has never held a valid configuration.
#[derive(Debug, Default)]
pub struct Current(Cell<Option<InEffect>>);

impl Current {
    #[must_use]
    pub fn get(&self) -> Option<InEffect> {
        self.0.get()
    }

    pub fn set(&self, settings: InEffect) {
        self.0.set(Some(settings));
    }

    /// whether `setting` is on in effect; off while no setting is in effect.
    #[must_use]
    pub fn is_on(&self, setting: Setting) -> bool {
        self.get().is_some_and(|settings| settings.is_on(setting))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_setting_takes_its_windows_default_and_says_so() {
        let settings = InEffect::windows(Lifecycle::default());
        let defaults: Vec<(Setting, bool)> = Setting::ALL
            .iter()
            .map(|&setting| (setting, settings.get(setting).on))
            .collect();
        assert_eq!(
            defaults,
            vec![
                (Setting::StartAtLogin, false),
                (Setting::StartRunnersOnLaunch, true),
                (Setting::KeepAwake, true),
                (Setting::StopOnNetworkChange, true),
                (Setting::ResumeAfterStop, false),
            ]
        );
        assert!(
            Setting::ALL
                .iter()
                .all(|&setting| settings.get(setting).defaulted)
        );
    }

    #[test]
    fn a_set_value_wins_over_the_default_even_when_it_equals_it() {
        let settings = InEffect::windows(Lifecycle {
            start_at_login: Some(true),
            keep_awake: Some(true),
            stop_on_network_change: Some(false),
            ..Lifecycle::default()
        });
        assert_eq!(
            settings.get(Setting::StartAtLogin),
            Value {
                on: true,
                defaulted: false
            }
        );
        assert_eq!(
            settings.get(Setting::KeepAwake),
            Value {
                on: true,
                defaulted: false
            }
        );
        assert!(!settings.is_on(Setting::StopOnNetworkChange));
        assert!(settings.get(Setting::ResumeAfterStop).defaulted);
    }

    #[test]
    fn a_write_changes_only_its_setting_and_unsetting_returns_to_the_default() {
        let settings = InEffect::windows(Lifecycle {
            keep_awake: Some(false),
            resume_after_stop: Some(true),
            ..Lifecycle::default()
        });
        let after = settings.with(Setting::KeepAwake, None);
        assert_eq!(
            after.get(Setting::KeepAwake),
            Value {
                on: true,
                defaulted: true
            }
        );
        assert_eq!(
            after.get(Setting::ResumeAfterStop),
            settings.get(Setting::ResumeAfterStop)
        );
        let off = after.with(Setting::StopOnNetworkChange, Some(false));
        assert!(!off.is_on(Setting::StopOnNetworkChange));
        assert!(!off.get(Setting::StopOnNetworkChange).defaulted);
    }
}
