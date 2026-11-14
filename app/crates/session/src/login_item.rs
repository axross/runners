//! the macOS login item kept in step with `start_at_login` in effect, as the agent keeps
//! the Run value on Windows: each time the settings in effect change, the app registers
//! its login item when the setting is on and macOS reports it not registered, and
//! unregisters it when the setting is off and macOS reports it registered. It never
//! registers over "requires approval", nor over a removal by the maintainer, which a
//! "not registered" while the record that Marshal registered itself exists counts as.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use marshal_cli::agent::snapshot::{LifecycleInfo, SettingInfo};
use marshal_cli::lifecycle_settings::Setting;

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
        }
    }

    /// what a snapshot's `lifecycle` part says: the login item is brought in step when the
    /// settings in effect differ from the last ones learned, the first ones included.
    pub(crate) fn learn(&mut self, lifecycle: Option<&LifecycleInfo>, log: &dyn Log) {
        let Some(lifecycle) = lifecycle else {
            return;
        };
        if self.settings.as_ref() == Some(&lifecycle.settings) {
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
            self.reconcile(setting.on, log);
        }
    }

    fn reconcile(&self, on: bool, log: &dyn Log) {
        match (on, self.service.status()) {
            (true, LoginStatus::Enabled) => {}
            (true, LoginStatus::RequiresApproval) => log.info(
                "Marshal's login item waits for approval in Login Items, so Marshal leaves it.",
            ),
            (true, LoginStatus::NotRegistered) if self.record.exists() => log.info(
                "Marshal's login item was turned off in Login Items, so Marshal leaves it off.",
            ),
            (true, LoginStatus::NotRegistered) => match self.service.register() {
                Ok(()) => {
                    log.info("Marshal registered its login item, as Start at login is on.");
                    if let Err(error) = self.record.write() {
                        log.warn(&format!(
                            "Marshal could not record that it registered its login item: {error}"
                        ));
                    }
                }
                Err(error) => log.warn(&format!(
                    "Marshal could not register its login item, and tries again when the settings change: {error}"
                )),
            },
            (false, LoginStatus::Enabled | LoginStatus::RequiresApproval) => {
                match self.service.unregister() {
                    Ok(()) => {
                        log.info("Marshal unregistered its login item, as Start at login is off.");
                        self.forget(log);
                    }
                    Err(error) => log.warn(&format!(
                        "Marshal could not unregister its login item, and tries again when the settings change: {error}"
                    )),
                }
            }
            // the item is already off, so a removal by the maintainer has nothing left to
            // protect, and turning the setting on again registers it
            (false, LoginStatus::NotRegistered) => self.forget(log),
            (_, LoginStatus::NotFound) => {
                log.warn("macOS cannot find Marshal's login item, so Marshal leaves it alone.");
            }
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

/// whether `executable` is the executable of a bundle directly inside `/Applications` or
/// `home`'s `Applications`.
fn in_applications(executable: &Path, home: Option<&Path>) -> bool {
    let Some(macos) = executable.parent() else {
        return false;
    };
    let Some(bundle) = macos.parent().and_then(Path::parent) else {
        return false;
    };
    let Some(folder) = bundle.parent() else {
        return false;
    };
    macos.ends_with("Contents/MacOS")
        && bundle
            .extension()
            .is_some_and(|extension| extension == "app")
        && (folder == Path::new("/Applications")
            || home.is_some_and(|home| folder == home.join("Applications")))
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
