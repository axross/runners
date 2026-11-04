//! the agent's log: a file in the log directory that is continued in a new one when
//! it reaches the size limit, with the newest ten files kept; every masked secret is
//! replaced, and nothing goes to a terminal, since the agent has none.

use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::instance::log_file::{LogFileError, MAX_FILE_BYTES, create_log_file};
use crate::runtime::log::{Log, Masks};
use crate::runtime::name::stamp;

struct Current {
    file: Option<File>,
    path: PathBuf,
    written: u64,
    /// when the newest file was named, so the next is named later.
    named: SystemTime,
    /// the lines dropped since the log stopped.
    lost: u64,
    /// when a stopped log may try to open a file again.
    retry_at: Instant,
}

/// how long a log that stopped waits before it tries to open a file again.
const RETRY_EVERY: Duration = Duration::from_secs(30);

/// the log the agent writes through.
pub struct AgentLog {
    masks: Masks,
    directory: PathBuf,
    max_bytes: u64,
    retry_every: Duration,
    current: RefCell<Current>,
}

impl AgentLog {
    /// opens the first file in `directory`; `max_bytes` is [`MAX_FILE_BYTES`] outside
    /// the tests.
    pub fn open(directory: &Path, max_bytes: u64) -> Result<Self, LogFileError> {
        let named = SystemTime::now();
        let raw = create_log_file(directory, named)?;
        let log = Self {
            masks: Masks::default(),
            directory: directory.to_path_buf(),
            max_bytes,
            retry_every: RETRY_EVERY,
            current: RefCell::new(Current {
                file: Some(raw.file),
                path: raw.path.clone(),
                written: 0,
                named,
                lost: 0,
                retry_at: Instant::now(),
            }),
        };
        log.info(&format!("Logging to {}.", raw.path.display()));
        if let Some(failure) = raw.pruning_failed {
            log.warn(&format!("An old log file could not be removed: {failure}"));
        }
        Ok(log)
    }

    /// the size limit of a file outside the tests.
    #[must_use]
    pub const fn default_max_bytes() -> u64 {
        MAX_FILE_BYTES
    }

    /// the file being written now.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.current.borrow().path.clone()
    }

    fn emit(&self, prefix: &str, line: &str) {
        let line = self.masks.apply(line);
        let text = format!("{} {prefix}{line}\n", stamp(SystemTime::now()));
        let mut current = self.current.borrow_mut();
        if current.file.is_none() && !self.reopen(&mut current) {
            current.lost = current.lost.saturating_add(1);
            return;
        }
        if current.written + text.len() as u64 > self.max_bytes {
            self.roll(&mut current);
        }
        self.write(&mut current, &text);
    }

    /// writes the text to the open file; a failed write stops the log, and a log with
    /// no file counts the line as lost.
    fn write(&self, current: &mut Current, text: &str) {
        let Some(file) = current.file.as_mut() else {
            current.lost = current.lost.saturating_add(1);
            return;
        };
        if file
            .write_all(text.as_bytes())
            .and_then(|()| file.flush())
            .is_err()
        {
            self.stop(current);
            current.lost = current.lost.saturating_add(1);
            return;
        }
        current.written += text.len() as u64;
    }

    /// the log has no file; it tries to get one again after the retry time.
    fn stop(&self, current: &mut Current) {
        current.file = None;
        current.retry_at = Instant::now() + self.retry_every;
    }

    /// tries to open a file for a log that stopped, no more often than the retry time,
    /// and says once in the new file how many lines were lost; true when it has one.
    fn reopen(&self, current: &mut Current) -> bool {
        if Instant::now() < current.retry_at {
            return false;
        }
        let at = SystemTime::now().max(current.named + Duration::from_millis(1));
        let Ok(raw) = create_log_file(&self.directory, at) else {
            current.retry_at = Instant::now() + self.retry_every;
            return false;
        };
        current.file = Some(raw.file);
        current.path = raw.path;
        current.written = 0;
        current.named = at;
        let notice = format!(
            "{} Logging recovered; {} lines were not written while it was stopped.\n",
            stamp(SystemTime::now()),
            current.lost
        );
        current.lost = 0;
        self.write(current, &notice);
        current.file.is_some()
    }

    /// continues the log in a new file, and keeps the old one when it cannot; a log
    /// that cannot be continued stops, as one whose write failed does, and tries
    /// again later.
    fn roll(&self, current: &mut Current) {
        let at = SystemTime::now().max(current.named + Duration::from_millis(1));
        let Ok(raw) = create_log_file(&self.directory, at) else {
            self.stop(current);
            return;
        };
        current.file = Some(raw.file);
        current.path = raw.path;
        current.written = 0;
        current.named = at;
    }
}

impl Log for AgentLog {
    fn info(&self, line: &str) {
        self.emit("", line);
    }

    fn warn(&self, line: &str) {
        self.emit("WARNING: ", line);
    }

    fn masks(&self) -> &Masks {
        &self.masks
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::runtime::secret::Secret;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("marshal-agent-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    fn files(directory: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        files
    }

    #[test]
    fn a_file_that_would_pass_its_limit_is_continued_in_a_new_one_and_ten_are_kept() {
        let directory = scratch("rolls");
        let log = AgentLog::open(&directory, 512).unwrap();
        for index in 0..400 {
            log.info(&format!("line {index} {}", "x".repeat(40)));
        }
        let files = files(&directory);
        assert_eq!(files.len(), 10, "{files:?}");
        for file in &files {
            assert!(fs::metadata(file).unwrap().len() <= 512, "{file:?}");
        }
        let last = fs::read_to_string(files.last().unwrap()).unwrap();
        assert!(last.contains("line 399"));
        assert_eq!(files.last().unwrap(), &log.path());
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_masked_secret_is_replaced_in_every_file() {
        let directory = scratch("masks");
        let log = AgentLog::open(&directory, 256).unwrap();
        log.mask(&Secret::new("fake-secret-value".to_string()));
        for _ in 0..20 {
            log.warn("the value is fake-secret-value");
        }
        for file in files(&directory) {
            let text = fs::read_to_string(file).unwrap();
            assert!(!text.contains("fake-secret-value"), "{text}");
        }
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_log_that_stopped_opens_a_file_again_and_says_how_much_it_lost() {
        let directory = scratch("recovers");
        let mut log = AgentLog::open(&directory, 256).unwrap();
        log.retry_every = Duration::from_millis(50);
        // the directory is replaced by a file, so the next roll cannot create one
        let aside = scratch("recovers-aside");
        fs::rename(&directory, &aside).unwrap();
        fs::write(&directory, b"in the way").unwrap();
        for index in 0..10 {
            log.info(&format!("lost {index} {}", "x".repeat(60)));
        }
        assert!(log.current.borrow().file.is_none());
        fs::remove_file(&directory).unwrap();
        // inside the retry time nothing is tried
        log.info("too early");
        assert!(log.current.borrow().file.is_none());
        std::thread::sleep(Duration::from_millis(120));
        log.info("back again");
        let text = fs::read_to_string(log.path()).unwrap();
        let written: usize = files(&aside)
            .iter()
            .map(|file| fs::read_to_string(file).unwrap().matches("lost ").count())
            .sum();
        assert!(written < 10, "the roll never failed");
        // every line that was not written is counted, the one that came too early too
        let lost = 10 - written + 1;
        assert!(
            text.contains(&format!("Logging recovered; {lost} lines were not written")),
            "{text}"
        );
        assert!(text.contains("back again"), "{text}");
        assert!(!text.contains("too early"), "{text}");
        fs::remove_dir_all(&directory).unwrap();
        fs::remove_dir_all(&aside).unwrap();
    }

    #[test]
    fn the_limit_outside_the_tests_is_ten_mib() {
        assert_eq!(AgentLog::default_max_bytes(), 10 * 1024 * 1024);
    }
}
