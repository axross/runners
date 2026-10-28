//! what `start` logs: every line goes to the terminal and to a log file of its own
//! in a private per-user directory, with every masked secret replaced in both; on
//! macOS the directory is `0700` and each file `0600`, and on Windows the CLI does
//! not check the directory's access, as `docs/operations/cli.md` describes.

use std::cell::RefCell;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::runtime::log::{Log, Masks};
use crate::runtime::name::stamp;

/// the size at which a log file stops, its last line included.
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

const KEPT_FILES: usize = 10;

const FULL_LINE: &str = "The log file reached 10 MiB, so nothing more is written to it.\n";

const FILE_PREFIX: &str = "marshal-";
const FILE_SUFFIX: &str = ".log";

/// why the log file could not be opened.
#[derive(Debug)]
pub struct LogFileError(String);

impl fmt::Display for LogFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the log file cannot be opened: {}", self.0)
    }
}

impl std::error::Error for LogFileError {}

fn failed(what: &str, path: &Path, error: &dyn fmt::Display) -> LogFileError {
    LogFileError(format!("{what} {}: {error}", path.display()))
}

/// a writer that stops at 10 MiB with a last line saying so.
pub struct Capped<W> {
    out: W,
    written: u64,
    full: bool,
}

impl<W: Write> Capped<W> {
    fn new(out: W) -> Self {
        Self {
            out,
            written: 0,
            full: false,
        }
    }

    /// writes `line`, which ends with its line break; a failed write stops the file
    /// as reaching the cap does.
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        if self.full {
            return Ok(());
        }
        let room = MAX_FILE_BYTES - self.written;
        let fits = (line.len() + FULL_LINE.len()) as u64 <= room;
        let text = if fits {
            line
        } else {
            self.full = true;
            FULL_LINE
        };
        let outcome = self
            .out
            .write_all(text.as_bytes())
            .and_then(|()| self.out.flush());
        if outcome.is_err() {
            self.full = true;
        }
        self.written += text.len() as u64;
        outcome
    }
}

/// the log `start` writes through.
pub struct StartLog {
    masks: Masks,
    file: RefCell<Capped<File>>,
}

impl StartLog {
    #[must_use]
    pub fn new(file: Capped<File>) -> Self {
        Self {
            masks: Masks::default(),
            file: RefCell::new(file),
        }
    }

    fn emit(&self, prefix: &str, line: &str) {
        let line = self.masks.apply(line);
        let _ = writeln!(io::stderr(), "{prefix}{line}");
        let stamped = format!("{} {prefix}{line}\n", stamp(SystemTime::now()));
        if let Err(error) = self.file.borrow_mut().write_line(&stamped) {
            let _ = writeln!(
                io::stderr(),
                "WARNING: writing the log file failed, so it stops here: {error}"
            );
        }
    }
}

impl Log for StartLog {
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

/// a new log file.
pub struct OpenedLog {
    pub path: PathBuf,
    pub file: Capped<File>,
    /// why an old file beyond the newest 10 could not be removed, when one could not.
    pub pruning_failed: Option<String>,
}

/// opens a new log file in `directory`, created private when missing, and removes
/// the oldest files beyond the newest 10; on macOS a directory or file that is not
/// private is an error.
pub fn open_log_file(directory: &Path) -> Result<OpenedLog, LogFileError> {
    prepare_directory(directory)?;
    let path = directory.join(format!(
        "{FILE_PREFIX}{}-{}{FILE_SUFFIX}",
        stamp(SystemTime::now()),
        std::process::id()
    ));
    let file = create_private(&path)?;
    Ok(OpenedLog {
        path,
        file: Capped::new(file),
        pruning_failed: prune(directory).err(),
    })
}

#[cfg(unix)]
fn prepare_directory(directory: &Path) -> Result<(), LogFileError> {
    use crate::private::{current_uid, ensure_private_directory};
    let uid = current_uid().map_err(|error| failed("finding the user for", directory, &error))?;
    ensure_private_directory(directory, uid)
        .map_err(|error| LogFileError(format!("the log directory is not private: {error}")))
}

#[cfg(windows)]
fn prepare_directory(directory: &Path) -> Result<(), LogFileError> {
    fs::create_dir_all(directory).map_err(|error| failed("creating", directory, &error))
}

/// creates the file, refusing one that exists; on Unix it is `0600` whatever the
/// umask, and checked as a private file.
#[cfg(unix)]
fn create_private(path: &Path) -> Result<File, LogFileError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    use crate::private::{FILE_MODE, check_private_file, current_uid};
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(path)
        .map_err(|error| failed("creating", path, &error))?;
    file.set_permissions(fs::Permissions::from_mode(FILE_MODE))
        .map_err(|error| failed("setting the mode of", path, &error))?;
    let metadata = file
        .metadata()
        .map_err(|error| failed("examining", path, &error))?;
    let uid = current_uid().map_err(|error| failed("finding the user for", path, &error))?;
    check_private_file(path, &metadata, uid)
        .map_err(|error| LogFileError(format!("the log file is not private: {error}")))?;
    Ok(file)
}

#[cfg(windows)]
fn create_private(path: &Path) -> Result<File, LogFileError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| failed("creating", path, &error))
}

/// removes the oldest log files, by name, beyond [`KEPT_FILES`]; only regular files
/// named as a log file count, and the first failure is returned after trying every
/// file.
fn prune(directory: &Path) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("listing {}: {error}", directory.display()))?;
    let mut logs: Vec<PathBuf> = entries
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
    logs.sort();
    let surplus = logs.len().saturating_sub(KEPT_FILES);
    let mut first_failure = None;
    for old in logs.iter().take(surplus) {
        if let Err(error) = fs::remove_file(old) {
            first_failure.get_or_insert_with(|| format!("removing {}: {error}", old.display()));
        }
    }
    first_failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capped() -> Capped<Vec<u8>> {
        Capped::new(Vec::new())
    }

    fn line_of(length: usize) -> String {
        let mut line = "x".repeat(length - 1);
        line.push('\n');
        line
    }

    #[test]
    fn a_line_that_leaves_exactly_room_for_the_last_line_is_written() {
        let mut file = capped();
        let line = line_of(usize::try_from(MAX_FILE_BYTES).unwrap() - FULL_LINE.len());
        file.write_line(&line).unwrap();
        assert!(!file.full);
        file.write_line("y\n").unwrap();
        assert!(file.full);
        assert_eq!(
            file.out.len() as u64,
            MAX_FILE_BYTES,
            "the cap is reached exactly"
        );
        assert!(file.out.ends_with(FULL_LINE.as_bytes()));
        file.write_line("z\n").unwrap();
        assert_eq!(
            file.out.len() as u64,
            MAX_FILE_BYTES,
            "nothing follows the last line"
        );
    }

    #[test]
    fn a_line_one_byte_longer_is_replaced_by_the_last_line() {
        let mut file = capped();
        let line = line_of(usize::try_from(MAX_FILE_BYTES).unwrap() - FULL_LINE.len() + 1);
        file.write_line(&line).unwrap();
        assert!(file.full);
        assert_eq!(file.out, FULL_LINE.as_bytes());
    }

    #[test]
    fn the_cap_is_ten_mib_and_ten_files_are_kept() {
        assert_eq!(MAX_FILE_BYTES, 10_485_760);
        assert_eq!(KEPT_FILES, 10);
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "marshal-log-file-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn names(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn log_name(index: usize) -> String {
        format!("{FILE_PREFIX}{index:017}-1{FILE_SUFFIX}")
    }

    #[test]
    fn ten_log_files_are_all_kept() {
        let directory = scratch("ten");
        for index in 1..=KEPT_FILES {
            fs::write(directory.join(log_name(index)), b"").unwrap();
        }
        prune(&directory).unwrap();
        assert_eq!(names(&directory).len(), KEPT_FILES);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn an_eleventh_log_file_removes_the_oldest_and_nothing_else() {
        let directory = scratch("eleven");
        for index in 1..=KEPT_FILES + 1 {
            fs::write(directory.join(log_name(index)), b"").unwrap();
        }
        fs::write(directory.join("notes.txt"), b"").unwrap();
        fs::create_dir(directory.join(log_name(0))).unwrap();
        prune(&directory).unwrap();
        let left = names(&directory);
        assert!(!left.contains(&log_name(1)), "the oldest file is removed");
        assert!(left.contains(&log_name(2)));
        assert!(left.contains(&"notes.txt".to_string()));
        assert!(left.contains(&log_name(0)), "a directory is not a log file");
        assert_eq!(left.len(), KEPT_FILES + 2);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_new_log_file_is_0600_in_a_0700_directory() {
        use std::os::unix::fs::PermissionsExt;
        let parent = scratch("private");
        let directory = parent.join("logs");
        let path = open_log_file(&directory).unwrap().path;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&directory), 0o700);
        assert_eq!(mode(&path), 0o600);
        fs::remove_dir_all(&parent).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn an_old_file_that_cannot_be_removed_leaves_the_new_one_open() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = scratch("held");
        for index in 1..=KEPT_FILES {
            fs::write(directory.join(log_name(index)), b"").unwrap();
        }
        // a file opened without delete sharing cannot be removed while it is open
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(directory.join(log_name(1)))
            .unwrap();
        let OpenedLog {
            path,
            file,
            pruning_failed,
        } = open_log_file(&directory).unwrap();
        assert!(path.is_file());
        let failure = pruning_failed.unwrap();
        assert!(failure.contains(&log_name(1)), "{failure}");
        drop((held, file));
        fs::remove_dir_all(&directory).unwrap();
    }
}
