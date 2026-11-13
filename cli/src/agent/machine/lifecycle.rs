//! the `[lifecycle]` settings in the agent: `lifecycle_set`, which writes one setting
//! through the configuration's write path and puts it in effect at once, the sign-in
//! item kept in step with `start_at_login`, and the snapshot's `lifecycle` part.
//! `docs/operations/cli.md` describes them.

use serde_json::json;

use crate::edit;
use crate::instance::lifecycle::Monitor;
use crate::lifecycle_settings::{InEffect, Setting, write};
use crate::profile::Profile;
use crate::runtime::backend::Backend;
use crate::runtime::clock::Clock;
use crate::runtime::registration::Registrar;
use crate::windows_host::run_key::SignIn;

use super::super::api::{ApiError, ApiResult};
use super::super::builder::ImageBuilder;
use super::super::snapshot::{LifecycleInfo, SettingInfo};
use super::super::state::Phase;
use super::super::wire::ErrorCode;
use super::Agent;
use super::configure::edit_error;

/// the text of a launch that left the runners stopped, a stop by you.
pub const NOT_STARTED_AT_LAUNCH: &str = "not started at launch";

impl<B: Backend, K: Clock, R: Registrar, D: ImageBuilder, M: Monitor> Agent<'_, B, K, R, D, M> {
    /// whether this host has the `[lifecycle]` settings: on axmc they are M4's.
    pub(super) fn has_lifecycle(&self) -> bool {
        self.settings.profile == Profile::Axpc
    }

    /// whether `launch` starts the runners: with `start_runners_on_launch` on, and
    /// while no setting is in effect, so Start all's path reports why.
    pub(super) fn starts_at_launch(&self) -> bool {
        self.in_effect
            .get()
            .is_none_or(|settings| settings.is_on(Setting::StartRunnersOnLaunch))
    }

    /// puts in effect the settings of a configuration just read, and keeps the
    /// sign-in item in step with them.
    pub(super) fn take_settings(&self, settings: InEffect) {
        if !self.has_lifecycle() {
            return;
        }
        self.in_effect.set(settings);
        self.sync_sign_in();
    }

    /// writes or removes the sign-in item as `start_at_login` says; nothing while no
    /// setting is in effect, or on a host without one.
    pub(super) fn sync_sign_in(&self) {
        let (Some(item), Some(settings)) = (self.sign_in, self.in_effect.get()) else {
            return;
        };
        let state = if settings.is_on(Setting::StartAtLogin) {
            match std::env::current_exe() {
                Ok(executable) => item.register(&executable),
                Err(error) => SignIn::Failed(format!("the program's path is unknown: {error}")),
            }
        } else {
            item.unregister()
        };
        if let SignIn::Failed(text) = &state {
            self.log.warn(&format!(
                "The sign-in item is not in step with start_at_login: {text}."
            ));
        }
        *self.sign_in_state.borrow_mut() = Some(state);
    }

    /// answers `lifecycle_set`: writes `value` to `setting` and puts it in effect.
    pub(super) fn lifecycle_set(&self, setting: Setting, value: Option<bool>) -> ApiResult {
        if !self.has_lifecycle() {
            return Err(ApiError::new(
                ErrorCode::Refused,
                "this host has no [lifecycle] settings yet",
            ));
        }
        if self.inner.borrow().applying.is_some() {
            return Err(ApiError::new(
                ErrorCode::ConfigApplying,
                "an edit of the configuration is being applied",
            ));
        }
        let Some(settings) = self.in_effect.get() else {
            return Err(ApiError::new(
                ErrorCode::ConfigUnusable,
                "no valid configuration has been read, so no setting can be changed",
            ));
        };
        self.refuse_migration()?;
        let above = self.above();
        edit::edit_config(
            &self.config_path(),
            self.settings.profile,
            above.as_deref(),
            &write::edits(setting, value),
        )
        .map_err(edit_error)?;
        self.in_effect.set(settings.with(setting, value));
        self.log.info(&format!(
            "{} is now {}.",
            setting.name(),
            value.map_or("unset", |on| if on { "on" } else { "off" })
        ));
        if setting == Setting::StartAtLogin {
            self.sync_sign_in();
        }
        self.publish();
        Ok(json!({ "lifecycle": self.lifecycle_info() }))
    }

    /// the snapshot's `lifecycle` part, while settings are in effect.
    pub(super) fn lifecycle_info(&self) -> Option<LifecycleInfo> {
        let settings = self.in_effect.get()?;
        let (sign_in, sign_in_error) = match &*self.sign_in_state.borrow() {
            None => (None, None),
            Some(SignIn::Registered) => (Some("registered"), None),
            Some(SignIn::NotRegistered) => (Some("not_registered"), None),
            Some(SignIn::TurnedOffInStartupApps) => (Some("turned_off"), None),
            Some(SignIn::Failed(text)) => (Some("failed"), Some(text.clone())),
        };
        let warning = self
            .monitor
            .report()
            .into_iter()
            .find_map(|(field, text)| (field == "warning").then_some(text));
        let held = matches!(
            self.inner.borrow().phase,
            Phase::Running | Phase::Resuming | Phase::Building | Phase::Stopping
        );
        let keep_awake = if !settings.is_on(Setting::KeepAwake) {
            "off"
        } else if warning.is_some() {
            "warning"
        } else if held {
            "held"
        } else {
            "released"
        };
        Some(LifecycleInfo {
            settings: Setting::ALL
                .into_iter()
                .map(|setting| {
                    let value = settings.get(setting);
                    let info = SettingInfo {
                        on: value.on,
                        defaulted: value.defaulted,
                    };
                    (setting.name().to_string(), info)
                })
                .collect(),
            sign_in: sign_in.map(str::to_string),
            sign_in_error,
            keep_awake: keep_awake.to_string(),
            keep_awake_warning: warning,
        })
    }
}
