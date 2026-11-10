//! creates the configuration file only where none exists: the bytes go to a temporary
//! file in the same folder, flushed to disk, and a hard link gives that file the final
//! name, which fails when the name is taken. a reader therefore sees no file or the
//! whole one, a file that appeared meanwhile is never overwritten, and the temporary
//! name is removed on every path.
//!
//! `std::fs::hard_link` reports a taken name as `AlreadyExists` on Linux, macOS and
//! Windows. Windows links only on NTFS, and some network shares refuse links anywhere;
//! there the failure is the `Failed` outcome, not a taken name, so the caller sees a
//! file that was not written rather than a wrong answer.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::replace::create_temporary;

/// what creating the file did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// the file holds the bytes.
    Written,
    /// a file or folder of that name exists, and was left as it was.
    Taken,
    /// the file was not written, and no temporary file is left.
    Failed(CreateFailure),
}

/// the step of the creation that the system refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateStep {
    /// making, filling or flushing the temporary file.
    Write,
    /// giving the temporary file its final name.
    Link,
}

/// why the file was not written: the step and the class of the system's error, with
/// no text of the system's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateFailure {
    pub path: PathBuf,
    pub step: CreateStep,
    pub kind: io::ErrorKind,
}

impl fmt::Display for CreateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let step = match self.step {
            CreateStep::Write => "write a new file beside",
            CreateStep::Link => "create",
        };
        write!(f, "could not {step} {}: {}", self.path.display(), self.kind)
    }
}

impl std::error::Error for CreateFailure {}

/// writes `contents` to `path` if no file of that name exists; see the module.
#[must_use]
pub fn create_new(path: &Path, contents: &[u8]) -> CreateOutcome {
    create_with(path, contents, |from, to| std::fs::hard_link(from, to))
}

/// [`create_new`] with the linking supplied, so that a test can have the system refuse
/// it.
fn create_with(
    path: &Path,
    contents: &[u8],
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> CreateOutcome {
    let failed = |step, error: io::Error| {
        CreateOutcome::Failed(CreateFailure {
            path: path.to_path_buf(),
            step,
            kind: error.kind(),
        })
    };
    let (temporary, mut file) = match create_temporary(path) {
        Ok(created) => created,
        Err(error) => return failed(CreateStep::Write, error),
    };
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        return failed(CreateStep::Write, error);
    }
    drop(file);
    let outcome = match link(&temporary.path, path) {
        Ok(()) => CreateOutcome::Written,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => CreateOutcome::Taken,
        Err(error) => failed(CreateStep::Link, error),
    };
    // dropping the temporary file removes its name; after a link the final name stays
    drop(temporary);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("marshal-edit-create-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_missing_name_is_written_whole_and_leaves_no_temporary_file() {
        let dir = scratch("written");
        let path = dir.join("config.toml");
        assert_eq!(create_new(&path, b"new"), CreateOutcome::Written);
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(names(&dir), ["config.toml"]);
    }

    #[test]
    fn a_taken_name_is_left_alone_and_leaves_no_temporary_file() {
        let dir = scratch("taken");
        let path = dir.join("config.toml");
        fs::write(&path, b"mine").unwrap();
        assert_eq!(create_new(&path, b"new"), CreateOutcome::Taken);
        assert_eq!(fs::read(&path).unwrap(), b"mine");
        assert_eq!(names(&dir), ["config.toml"]);
    }

    #[test]
    fn a_refused_link_removes_the_temporary_file_and_names_the_step() {
        let dir = scratch("refused");
        let path = dir.join("config.toml");
        let outcome = create_with(&path, b"new", |_, _| {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert_eq!(
            outcome,
            CreateOutcome::Failed(CreateFailure {
                path: path.clone(),
                step: CreateStep::Link,
                kind: io::ErrorKind::PermissionDenied,
            })
        );
        assert!(names(&dir).is_empty());
    }

    #[test]
    fn a_taken_name_reported_by_the_link_is_taken_and_removes_the_temporary_file() {
        let dir = scratch("link-taken");
        let path = dir.join("config.toml");
        let outcome = create_with(&path, b"new", |_, _| {
            Err(io::Error::from(io::ErrorKind::AlreadyExists))
        });
        assert_eq!(outcome, CreateOutcome::Taken);
        assert!(names(&dir).is_empty());
    }

    #[test]
    fn a_folder_that_is_gone_fails_at_the_write_step() {
        let dir = scratch("gone");
        let path = dir.join("absent").join("config.toml");
        let CreateOutcome::Failed(failure) = create_new(&path, b"new") else {
            panic!("expected a failure");
        };
        assert_eq!(failure.step, CreateStep::Write);
        assert_eq!(failure.kind, io::ErrorKind::NotFound);
        assert!(names(&dir).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_new_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("mode");
        let path = dir.join("config.toml");
        assert_eq!(create_new(&path, b"new"), CreateOutcome::Written);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
}
