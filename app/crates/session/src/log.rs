//! the app's log: a file in the agent's log folder under the app's own name, continued in
//! a new file when it would pass 10 MiB, with the newest ten kept. It is not the agent's
//! log: the agent's files are named `marshal-<time>-<pid>.log` and the agent prunes
//! only those, so the two never take each other's place. Every line goes through
//! [`plain`], which replaces a word shaped like a GitHub token, in case one ever reached
//! it; the app never holds a token it could log.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use marshal_cli::runtime::name::stamp;
use marshal_view::plain;

use crate::seam::Log;

/// the size at which the app's log continues in a new file.
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// how many of the app's log files are kept, the newest.
pub const KEPT_FILES: usize = 10;

/// how long a log that stopped waits before it tries to open a file again, as the
/// agent's log does.
const RETRY_EVERY: Duration = Duration::from_secs(30);

const FILE_PREFIX: &str = "app-";
const FILE_SUFFIX: &str = ".log";

struct Current {
    file: Option<File>,
    written: u64,
    /// when the newest file was named, so the next is named later.
    named: SystemTime,
    /// when a log that stopped may try to open a file again.
    retry_at: Instant,
}

pub struct AppLog {
    directory: PathBuf,
    max_bytes: u64,
    current: Mutex<Current>,
}

impl AppLog {
    /// opens the first file in `directory`, which is created private when missing.
    pub fn open(directory: &Path, max_bytes: u64) -> io::Result<Self> {
        let named = SystemTime::now();
        let file = create(directory, named)?;
        Ok(Self {
            directory: directory.to_path_buf(),
            max_bytes,
            current: Mutex::new(Current {
                file: Some(file),
                written: 0,
                named,
                retry_at: Instant::now(),
            }),
        })
    }

    fn emit(&self, prefix: &str, line: &str) {
        let text = format!("{} {prefix}{}\n", stamp(SystemTime::now()), plain(line));
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let length = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if current.file.is_none() || current.written.saturating_add(length) > self.max_bytes {
            // a log that stopped drops its lines until the retry time, so a full disk
            // does not replace every kept file with a new empty one
            if current.file.is_none() && Instant::now() < current.retry_at {
                return;
            }
            let at = SystemTime::now().max(current.named + Duration::from_millis(1));
            current.file = create(&self.directory, at).ok();
            current.written = 0;
            current.named = at;
        }
        let Some(file) = current.file.as_mut() else {
            current.retry_at = Instant::now() + RETRY_EVERY;
            return;
        };
        if file
            .write_all(text.as_bytes())
            .and_then(|()| file.flush())
            .is_err()
        {
            current.file = None;
            current.retry_at = Instant::now() + RETRY_EVERY;
        }
        current.written = current.written.saturating_add(length);
    }
}

impl Log for AppLog {
    fn info(&self, line: &str) {
        self.emit("", line);
    }

    fn warn(&self, line: &str) {
        self.emit("WARNING: ", line);
    }
}

/// a new file in `directory`, and the oldest beyond the newest ten removed.
fn create(directory: &Path, time: SystemTime) -> io::Result<File> {
    prepare(directory)?;
    let path = directory.join(format!(
        "{FILE_PREFIX}{}-{}{FILE_SUFFIX}",
        stamp(time),
        std::process::id()
    ));
    let file = create_new(&path)?;
    prune(directory);
    Ok(file)
}

#[cfg(unix)]
fn prepare(directory: &Path) -> io::Result<()> {
    use marshal_cli::private::{current_uid, ensure_private_directory};
    let uid = current_uid()?;
    ensure_private_directory(directory, uid).map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(windows)]
fn prepare(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)
}

#[cfg(unix)]
fn create_new(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(marshal_cli::private::FILE_MODE)
        .open(path)
}

#[cfg(windows)]
fn create_new(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// removes the oldest of the app's files beyond [`KEPT_FILES`], by name; only a regular
/// file named as one of the app's counts, so the agent's files are never touched. A file
/// that cannot be removed is left.
fn prune(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX))
        })
        .map(|entry| entry.path())
        .collect();
    files.sort();
    let surplus = files.len().saturating_sub(KEPT_FILES);
    for old in files.iter().take(surplus) {
        let _ = fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_that_stopped_makes_no_file_before_its_retry_time() {
        let directory =
            std::env::temp_dir().join(format!("marshal-app-log-retry-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let logs = directory.join("logs");
        let log = AppLog::open(&logs, 64).unwrap();
        // the next line passes the limit, and no file can be made where the folder was
        fs::remove_dir_all(&logs).unwrap();
        fs::write(&logs, b"").unwrap();
        log.info(&"x".repeat(80));
        fs::remove_file(&logs).unwrap();
        log.info("dropped until the retry time");
        assert!(
            !logs.exists(),
            "a stopped log made a file before its retry time"
        );
        log.current.lock().unwrap().retry_at = Instant::now();
        log.info("written again");
        assert_eq!(fs::read_dir(&logs).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}
