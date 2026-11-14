//! the `[lifecycle]` settings in the agent: `lifecycle_set`, which writes one setting
//! through the configuration's write path and puts it in effect at once, the sign-in
//! item kept in step with `start_at_login`, the resume after a signal stop, and the
//! snapshot's `lifecycle` part.
//! `docs/operations/cli.md` describes them.

use std::time::Instant;

use serde_json::json;

use crate::instance::lifecycle::Monitor;
use crate::instance::resume::{Armed, LIMIT};
use crate::lifecycle_settings::{InEffect, Setting, write};
use crate::macos::network::NetworkReading;
use crate::profile::Profile;
use crate::runtime::backend::Backend;
use crate::runtime::clock::Clock;
use crate::runtime::registration::Registrar;
use crate::windows_host::run_key::SignIn;

use super::super::api::{ApiError, ApiResult, Method};
use super::super::builder::ImageBuilder;
use super::super::snapshot::{LifecycleInfo, SettingInfo};
use super::super::state::{Phase, Reason};
use super::super::wire::ErrorCode;
use super::Agent;
use super::configure::{edit_error, revision};

/// the text of a launch that left the runners stopped, a stop by you.
pub const NOT_STARTED_AT_LAUNCH: &str = "not started at launch";

/// the snapshot's `resume` while a resume waits for the network the runners stopped on.
pub const RESUME_WAITING: &str = "waiting";

/// the snapshot's `resume` when that network has no gateway hardware address, so none
/// is armed.
pub const RESUME_NO_GATEWAY: &str = "no_gateway";

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
                ErrorCode::Busy,
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
        let file = self.read_for_change()?;
        if self.read_revision.borrow().as_deref() != Some(revision(file.bytes()).as_str()) {
            return Err(ApiError::new(
                ErrorCode::ConfigChanged,
                "config.toml changed after the agent read it; Start all reads it again",
            ));
        }
        let above = self.above();
        let profile = self.settings.profile;
        let edits = write::edits(setting, value);
        let written = file
            .edit(profile, above.as_deref(), &edits)
            .map_err(edit_error)?;
        *self.read_revision.borrow_mut() = Some(revision(&written));
        let settings = settings.with(setting, value);
        self.in_effect.set(settings);
        if !settings.is_on(Setting::ResumeAfterStop) && self.resume_note.take().is_some() {
            self.resume.borrow_mut().disarm();
            self.log.info(
                "resume_after_stop is off, so the runners will not start again on their own.",
            );
        }
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

    /// arms the resume at a signal stop for `reason` while `resume_after_stop` is on,
    /// and returns the reason the agent stops with: [`Reason::RESUMED_TOO_OFTEN`] when
    /// the runners resumed too often to arm another.
    pub(super) fn arm_resume(&self, reason: Reason) -> Reason {
        self.resume_note.set(None);
        // an agent that is ending arms nothing
        if self.exit.get().is_some()
            || !self
                .in_effect
                .get()
                .is_some_and(|settings| settings.is_on(Setting::ResumeAfterStop))
        {
            self.resume.borrow_mut().disarm();
            return reason;
        }
        let armed = self
            .resume
            .borrow_mut()
            .arm(Instant::now(), self.monitor.recorded());
        match armed {
            Armed::Waiting => {
                self.log.info(
                    "The runners start again once the primary network they stopped on has settled.",
                );
                self.resume_note.set(Some((RESUME_WAITING, reason.code)));
                reason
            }
            Armed::NoGateway => {
                self.log.info(
                    "No primary network with a gateway hardware address was recorded before \
                     the stop, so the runners will not start again on their own.",
                );
                self.resume_note.set(Some((RESUME_NO_GATEWAY, reason.code)));
                reason
            }
            Armed::TooOften => {
                self.log.warn(&format!(
                    "The runners resumed {LIMIT} times within an hour, so they will not start \
                     again on their own."
                ));
                Reason::new(
                    Reason::RESUMED_TOO_OFTEN,
                    format!(
                        "{}; the runners resumed {LIMIT} times within an hour, so they will not \
                         start again on their own",
                        reason.message
                    ),
                )
            }
        }
    }

    /// whether a resume waits for the network the runners stopped on.
    pub(super) fn resume_pending(&self) -> bool {
        self.resume.borrow().is_pending()
    }

    /// judges one reading while stopped: true when the resume fires, and the runners
    /// start again as Start all starts them.
    pub(super) fn observe_resume(&self, reading: &NetworkReading) -> bool {
        if !self.resume.borrow_mut().observe(Instant::now(), reading) {
            return false;
        }
        self.resume_note.set(None);
        self.log
            .info("The primary network the runners stopped on has settled, so they start again.");
        true
    }

    /// cancels the resume for `method`, a user's lifecycle method; a Stop all while a
    /// resume waits leaves the runners stopped by you.
    pub(super) fn cancel_resume(&self, method: &Method) {
        let pending = self.resume_pending();
        self.resume.borrow_mut().disarm();
        if self.resume_note.take().is_none() {
            return;
        }
        if pending && matches!(method, Method::StopAll) {
            self.log
                .info("Stop all cancelled the resume, so the runners stay stopped.");
            self.set_phase(Phase::Stopped(Reason::by_you()));
        } else {
            self.publish();
        }
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
        let warning = settings
            .is_on(Setting::KeepAwake)
            .then(|| self.monitor.report())
            .into_iter()
            .flatten()
            .find_map(|(field, text)| (field == "warning").then_some(text));
        let held = self.inner.borrow().phase.holds_keep_awake();
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
            resume: self.resume_note.get().map(|(state, _)| state.to_string()),
            resume_cause: self.resume_note.get().map(|(_, cause)| cause.to_string()),
        })
    }
}
