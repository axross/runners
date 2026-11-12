//! the one-shot record the app writes when the agent ended for an update with the
//! runners "stopped by you": the update's version and when it was written. The next
//! launch of that version within 30 minutes starts the agent stopped, and every launch
//! removes the record it finds.

use std::path::PathBuf;
use std::time::Duration;

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

    /// at launch: whether the record says the runners were stopped by you when the agent
    /// ended for the update to `version`, no more than 30 minutes before `now_ms`. Any
    /// record is removed, whatever it says.
    #[must_use]
    pub fn take(&self, version: &str, now_ms: u64) -> bool {
        let read = std::fs::read(&self.path).ok();
        self.remove();
        let Some(written) = read.and_then(|bytes| serde_json::from_slice::<Written>(&bytes).ok())
        else {
            return false;
        };
        let limit = u64::try_from(RECORD_APPLIES_FOR.as_millis()).unwrap_or(u64::MAX);
        written.version == version
            && written.written_ms <= now_ms
            && now_ms - written.written_ms <= limit
    }
}

impl Record for RecordFile {
    fn write(&self, version: &str, at_ms: u64) -> Result<(), String> {
        let written = serde_json::to_vec(&Written {
            version: version.to_string(),
            written_ms: at_ms,
        })
        .map_err(|error| error.to_string())?;
        std::fs::write(&self.path, written)
            .map_err(|error| format!("{}: {error}", self.path.display()))
    }

    fn remove(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
