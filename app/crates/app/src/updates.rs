//! the app's update seams on Windows: Tauri's updater plugin reads the feed, downloads
//! the MSI and verifies its signature, and installs it; a process handle tells when the
//! agent has exited. The session drives them (see `marshal-session`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use marshal_feed::{DEVELOPMENT_VERSION, download_allowed};
use marshal_session::{Later, Process, Processes, Updater, Verified};
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use winsafe::{self as w, co, guard::CloseHandleGuard};

/// the longest a read of the feed takes.
const CHECK_WITHIN: Duration = Duration::from_secs(30);

/// the longest the MSI's download takes.
const DOWNLOAD_WITHIN: Duration = Duration::from_secs(10 * 60);

/// how often a wait looks at the agent's process.
const EXIT_POLL: Duration = Duration::from_millis(250);

/// the updater plugin, registered in the app mode only.
#[must_use]
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// the feed, or `None` when this build does not update itself: a development build, or
/// one with no committed update-feed key.
#[must_use]
pub fn feed<R: Runtime>(app: &AppHandle<R>, version: &str) -> Option<Arc<dyn Updater>> {
    let key = marshal_feed::committed_public_key().ok()?;
    if version == DEVELOPMENT_VERSION || key.trim().is_empty() {
        return None;
    }
    Some(Arc::new(Feed { app: app.clone() }))
}

struct Feed<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Updater for Feed<R> {
    fn check(&self) -> Later<Result<Option<Box<dyn Verified>>, String>> {
        let app = self.app.clone();
        Box::pin(async move {
            let updater = app
                .updater_builder()
                .timeout(CHECK_WITHIN)
                .build()
                .map_err(|error| error.to_string())?;
            let Some(mut update) = updater.check().await.map_err(|error| error.to_string())? else {
                return Ok(None);
            };
            download_allowed(&update.download_url).map_err(|refusal| refusal.to_string())?;
            // the check's timeout does not carry over to the download
            update.timeout = Some(DOWNLOAD_WITHIN);
            // `download` verifies the signature, and the version it names, before it returns
            let bytes = update
                .download(|_, _| {}, || {})
                .await
                .map_err(|error| error.to_string())?;
            Ok(Some(
                Box::new(Downloaded { update, bytes }) as Box<dyn Verified>
            ))
        })
    }
}

struct Downloaded {
    update: Update,
    bytes: Vec<u8>,
}

impl Verified for Downloaded {
    fn version(&self) -> &str {
        &self.update.version
    }

    /// verifies the bytes again and starts `msiexec`; the plugin ends this process when
    /// the installer has started.
    fn install(self: Box<Self>) -> Result<(), String> {
        self.update
            .install(&self.bytes)
            .map_err(|error| error.to_string())
    }
}

/// opens processes by ID with the right to wait on them.
pub struct Handles;

impl Processes for Handles {
    fn open(&self, pid: u32) -> Result<Option<Arc<dyn Process>>, String> {
        match w::HPROCESS::OpenProcess(
            co::PROCESS::SYNCHRONIZE | co::PROCESS::QUERY_LIMITED_INFORMATION,
            false,
            pid,
        ) {
            Ok(handle) => Ok(Some(Arc::new(Handle(Mutex::new(handle))))),
            // no such process: it has exited
            Err(co::ERROR::INVALID_PARAMETER) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}

struct Handle(Mutex<CloseHandleGuard<w::HPROCESS>>);

impl Process for Handle {
    fn has_exited(&self) -> bool {
        let handle = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        matches!(handle.WaitForSingleObject(Some(0)), Ok(co::WAIT::OBJECT_0))
    }

    fn wait(self: Arc<Self>, limit: Duration) -> Later<bool> {
        Box::pin(async move {
            let until = tokio::time::Instant::now() + limit;
            loop {
                if self.has_exited() {
                    return true;
                }
                if tokio::time::Instant::now() >= until {
                    return false;
                }
                tokio::time::sleep(EXIT_POLL).await;
            }
        })
    }
}
