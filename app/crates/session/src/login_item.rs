//! the macOS login item kept in step with `start_at_login` in effect, as the agent keeps
//! the Run value on Windows: at launch, each time the settings in effect change and once
//! after each Start all, the app registers its login item when the setting is on and
//! macOS reports it not registered, and unregisters it when the setting is off and macOS
//! reports it registered. It never registers over "requires approval", nor over a
//! removal by the maintainer, which a "not registered" while the record that Marshal
//! registered itself exists counts as. Showing the window only reads the state again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use marshal_cli::agent::snapshot::{LifecycleInfo, SettingInfo};
use marshal_cli::lifecycle_settings::Setting;
use marshal_view::LoginItemState;

use crate::location::in_applications;
use crate::seam::{Log, LoginItemRecord, LoginItemService, LoginStatus};

/// the record's file name, in Marshal's configuration folder.
pub const LOGIN_ITEM_RECORD_FILE: &str = "login-item-registered-by-marshal";

/// the login item and what the app last learned of the settings in effect.
pub struct LoginItem {
    service: Arc<dyn LoginItemService>,
    record: Arc<dyn LoginItemRecord>,
    in_applications: bool,
    told_outside: bool,
    settings: Option<BTreeMap<String, SettingInfo>>,
    state: Option<LoginItemState>,
}

impl LoginItem {
    /// the login item of the app that runs `executable`, which registers or unregisters
    /// only from a bundle directly inside `/Applications` or `~/Applications`.
    #[must_use]
    pub fn new(
        service: Arc<dyn LoginItemService>,
        record: Arc<dyn LoginItemRecord>,
        executable: &Path,
        home: Option<&Path>,
    ) -> Self {
        Self {
            service,
            record,
            in_applications: in_applications(executable, home),
            told_outside: false,
            settings: None,
            state: None,
        }
    }

    /// the login item's state as the latest bringing in step or reading left it; `None`
    /// before the first, or from outside Applications.
    #[must_use]
    pub fn state(&self) -> Option<LoginItemState> {
        self.state
    }

    /// what a snapshot's `lifecycle` part says: the login item is brought in step when the
    /// settings in effect differ from the last ones learned, the first ones included, and
    /// when `after_start_all` says the snapshot is the first to show a Start all.
    pub(crate) fn learn(
        &mut self,
        lifecycle: Option<&LifecycleInfo>,
        after_start_all: bool,
        log: &dyn Log,
    ) {
        let Some(lifecycle) = lifecycle else {
            return;
        };
        if self.settings.as_ref() == Some(&lifecycle.settings) && !after_start_all {
            return;
        }
        self.settings = Some(lifecycle.settings.clone());
        if !self.in_applications {
            if !self.told_outside {
                self.told_outside = true;
                log.info(
                    "Marshal runs from outside Applications, so it leaves its login item alone.",
                );
            }
            return;
        }
        if let Some(setting) = lifecycle.settings.get(Setting::StartAtLogin.name()) {
            self.state = Some(self.reconcile(setting.on, log));
        }
    }

    /// reads macOS's status again, so a removal or an approval made in Login Items shows,
    /// and registers and unregisters nothing. A failed registration keeps its code until
    /// macOS reports something else.
    pub(crate) fn refresh(&mut self) {
        let Some(state) = self.state else {
            return;
        };
        let status = self.service.status();
        let failed = matches!(state, LoginItemState::NotAdded { .. });
        let unregistered = matches!(status, LoginStatus::NotRegistered | LoginStatus::NotFound);
        if !(failed && unregistered) {
            self.state = Some(self.known(status));
        }
    }

    /// brings the login item in step with `on` and says the state it leaves it in. macOS
    /// reports a Marshal that has never registered as not found, so not found counts as
    /// not registered (plan revision 4).
    fn reconcile(&self, on: bool, log: &dyn Log) -> LoginItemState {
        let status = self.service.status();
        match (on, status) {
            (true, LoginStatus::Enabled) => {}
            (true, LoginStatus::RequiresApproval) => log.info(
                "Marshal's login item waits for approval in Login Items, so Marshal leaves it.",
            ),
            (true, LoginStatus::NotRegistered | LoginStatus::NotFound) if self.record.exists() => {
                log.info(
                    "Marshal's login item was turned off in Login Items, so Marshal leaves it off.",
                )
            }
            (true, LoginStatus::NotRegistered | LoginStatus::NotFound) => {
                match self.service.register() {
                    Ok(()) => {
                        log.info("Marshal registered its login item, as Start at login is on.");
                        if let Err(error) = self.record.write() {
                            log.warn(&format!(
                            "Marshal could not record that it registered its login item: {error}"
                        ));
                        }
                        // macOS may hold a new item for approval
                        return self.known(self.service.status());
                    }
                    Err(error) => {
                        log.warn(&format!(
                        "Marshal could not register its login item, and tries again at the next launch, Start all or change of a setting: {error}"
                    ));
                        return LoginItemState::NotAdded { code: error.code };
                    }
                }
            }
            (false, LoginStatus::Enabled | LoginStatus::RequiresApproval) => {
                match self.service.unregister() {
                    Ok(()) => {
                        log.info("Marshal unregistered its login item, as Start at login is off.");
                        self.forget(log);
                        return LoginItemState::Off;
                    }
                    Err(error) => log.warn(&format!(
                        "Marshal could not unregister its login item, and tries again at the next launch, Start all or change of a setting: {error}"
                    )),
                }
            }
            // the item is already off, so a removal by the maintainer has nothing left to
            // protect, and turning the setting on again registers it
            (false, LoginStatus::NotRegistered | LoginStatus::NotFound) => self.forget(log),
        }
        self.known(status)
    }

    /// the state `status` and the record give.
    fn known(&self, status: LoginStatus) -> LoginItemState {
        match status {
            LoginStatus::Enabled => LoginItemState::On,
            LoginStatus::RequiresApproval => LoginItemState::WaitingForApproval,
            LoginStatus::NotRegistered | LoginStatus::NotFound if self.record.exists() => {
                LoginItemState::Removed
            }
            LoginStatus::NotRegistered | LoginStatus::NotFound => LoginItemState::Off,
        }
    }

    fn forget(&self, log: &dyn Log) {
        if let Err(error) = self.record.remove() {
            log.warn(&format!(
                "Marshal could not remove the record that it registered its login item: {error}"
            ));
        }
    }
}

/// the record as a file, whose presence is the record.
pub struct LoginItemRecordFile {
    path: PathBuf,
}

impl LoginItemRecordFile {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl LoginItemRecord for LoginItemRecordFile {
    fn exists(&self) -> bool {
        self.path.is_file()
    }

    fn write(&self) -> Result<(), String> {
        std::fs::write(&self.path, b"").map_err(|error| format!("{}: {error}", self.path.display()))
    }

    fn remove(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {error}", self.path.display()))
            }
            _ => Ok(()),
        }
    }
}
