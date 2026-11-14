//! the app's update seams: Tauri's updater plugin reads the feed, downloads the
//! update and verifies its signature, and installs it, the MSI on Windows and the app
//! bundle on the Mac; a process handle tells when the agent has exited. The session
//! drives them (see `marshal-session`).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::sync::Arc;
use std::time::Duration;

use marshal_feed::download_allowed;
use marshal_session::{Later, Updater, Verified};
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

#[cfg(target_os = "macos")]
pub use macos::Watched as Processes;
#[cfg(windows)]
pub use windows::Handles as Processes;

/// the longest a read of the feed takes.
const CHECK_WITHIN: Duration = Duration::from_secs(30);

/// the longest the update's download takes.
const DOWNLOAD_WITHIN: Duration = Duration::from_secs(10 * 60);

/// how often a wait looks at the agent's process.
const EXIT_POLL: Duration = Duration::from_millis(250);

/// the updater plugin, registered in the app mode only.
#[must_use]
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// the feed, or `None` when this build has no committed update-feed key. A development
/// build gets the feed too; the session never checks at `0.0.0-dev`.
#[must_use]
pub fn feed<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<dyn Updater>> {
    let key = marshal_feed::committed_public_key().ok()?;
    if key.trim().is_empty() {
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
            // on the Mac the bundle is exchanged in place, so one that cannot be is not
            // downloaded
            #[cfg(target_os = "macos")]
            macos::replaceable()?;
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
    #[cfg(windows)]
    fn install(self: Box<Self>) -> Result<(), String> {
        self.update
            .install(&self.bytes)
            .map_err(|error| error.to_string())
    }

    /// verifies the bytes again and exchanges the bundle with the update's, then starts
    /// a shell that opens the new bundle once this process has exited.
    #[cfg(target_os = "macos")]
    fn install(self: Box<Self>) -> Result<(), String> {
        self.update
            .install(&self.bytes)
            .map_err(|error| error.to_string())?;
        macos::relaunch()
    }
}
