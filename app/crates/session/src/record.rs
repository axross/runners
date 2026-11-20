//! the one-shot record the app writes when the agent ended for an update: the update's
//! version, when it was written, and whether the runners were running. The next launch
//! of that version within 30 minutes starts the agent as the record says, so a stop by
//! you or by a signal survives the update, and every launch removes the record it finds.

use std::path::PathBuf;
use std::time::Duration;

use marshal_cli::agent::Launch;
use serde::{Deserialize, Serialize};

use crate::seam::Record;

/// how long a record applies after it was written.
pub const RECORD_APPLIES_FOR: Duration = Duration::from_secs(30 * 60);

/// the record's file name, in Marshal's local data folder.
pub const RECORD_FILE: &str = "update-stopped-by-you.json";

#[derive(Serialize, Deserialize)]
struct Written {
    version: String,
    written_ms: u64,
    /// absent from a record written before it was added, which only ever recorded runners
    /// stopped by you.
    #[serde(default)]
    running: bool,
}

/// the record as a file.
pub struct RecordFile {
    path: PathBuf,
}

impl RecordFile {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// at launch: how the record says the agent starts after the update to `version`,
    /// written no more than 30 minutes before `now_ms`, or `None` when no record applies
    /// and the launch is fresh. Any record is removed, whatever it says.
    #[must_use]
    pub fn take(&self, version: &str, now_ms: u64) -> Option<Launch> {
        let read = std::fs::read(&self.path).ok();
        self.remove();
        let written = serde_json::from_slice::<Written>(&read?).ok()?;
        let limit = u64::try_from(RECORD_APPLIES_FOR.as_millis()).unwrap_or(u64::MAX);
        let applies = written.version == version
            && written.written_ms <= now_ms
            && now_ms - written.written_ms <= limit;
        applies.then_some(if written.running {
            Launch::Running
        } else {
            Launch::Stopped
        })
    }
}

impl Record for RecordFile {
    fn write(&self, version: &str, at_ms: u64, stopped: bool) -> Result<(), String> {
        let written = serde_json::to_vec(&Written {
            version: version.to_string(),
            written_ms: at_ms,
            running: !stopped,
        })
        .map_err(|error| error.to_string())?;
        std::fs::write(&self.path, written)
            .map_err(|error| format!("{}: {error}", self.path.display()))
    }

    fn remove(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// the marker the app leaves when its agent ended because Marshal was moved, renamed or
/// replaced while it ran, in Marshal's local data folder; the next launch takes it.
pub const MOVED_FILE: &str = "agent-ended-moved";

/// the marker as a file. It holds nothing: being there is all it says.
pub struct MovedFile {
    path: PathBuf,
}

impl MovedFile {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// leaves the marker for the next launch.
    pub fn write(&self) -> Result<(), String> {
        std::fs::write(&self.path, b"").map_err(|error| format!("{}: {error}", self.path.display()))
    }

    /// at launch: whether the marker was there; it is removed either way.
    #[must_use]
    pub fn take(&self) -> bool {
        std::fs::remove_file(&self.path).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::MovedFile;

    #[test]
    fn the_moved_marker_is_taken_once() {
        let folder = std::env::temp_dir().join(format!("marshal-moved-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let marker = MovedFile::new(folder.join(super::MOVED_FILE));
        assert!(!marker.take(), "no marker before one is written");
        marker.write().unwrap();
        assert!(marker.take());
        assert!(!marker.take(), "a second launch finds none");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
