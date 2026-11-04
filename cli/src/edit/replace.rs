//! replaces the configuration file in one step: the new bytes go to a temporary file
//! in the same folder, flushed to disk, and the file is renamed over the old one, so
//! a reader sees the old file or the new one and never part of one. the temporary
//! files are named `config.toml.<random>.tmp`.

use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{EditError, Operation};

/// the file name of the configuration.
pub const CONFIG_FILE_NAME: &str = "config.toml";

const TEMPORARY_SUFFIX: &str = ".tmp";

const NAME_ATTEMPTS: usize = 8;

/// replaces `path` with `contents` if it still holds `expected`.
pub(super) fn replace(
    path: &Path,
    expected: &[u8],
    permissions: &fs::Permissions,
    contents: &[u8],
) -> Result<(), EditError> {
    replace_with(path, expected, permissions, contents, |from, to| {
        fs::rename(from, to)
    })
}

/// [`replace`] with the renaming supplied, so that a test can have the system refuse
/// it.
fn replace_with(
    path: &Path,
    expected: &[u8],
    permissions: &fs::Permissions,
    contents: &[u8],
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), EditError> {
    let failed = |operation, error: io::Error| EditError::Io {
        path: path.to_path_buf(),
        operation,
        kind: error.kind(),
    };
    let (mut temporary, mut file) =
        create_temporary(path).map_err(|error| failed(Operation::Write, error))?;
    write_contents(&mut file, contents, permissions)
        .map_err(|error| failed(Operation::Write, error))?;
    drop(file);
    match read_limited(path, expected.len()) {
        Ok((now, _)) if now == expected => {}
        Ok(_) => return Err(EditError::Changed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(EditError::Changed),
        Err(error) => return Err(failed(Operation::Read, error)),
    }
    rename(&temporary.path, path).map_err(|error| failed(Operation::Replace, error))?;
    temporary.renamed = true;
    Ok(())
}

fn write_contents(
    file: &mut File,
    contents: &[u8],
    permissions: &fs::Permissions,
) -> io::Result<()> {
    file.write_all(contents)?;
    #[cfg(unix)]
    file.set_permissions(permissions.clone())?;
    #[cfg(not(unix))]
    let _ = permissions;
    file.sync_all()
}

/// the bytes of the file with its permissions, at most `limit` of them and one more:
/// the extra byte tells a longer file.
pub(super) fn read_limited(path: &Path, limit: usize) -> io::Result<(Vec<u8>, fs::Permissions)> {
    let file = File::open(path)?;
    let permissions = file.metadata()?.permissions();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(cap).read_to_end(&mut bytes)?;
    Ok((bytes, permissions))
}

/// a temporary file that is removed when it goes out of scope, unless it was renamed.
struct Temporary {
    path: PathBuf,
    renamed: bool,
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.renamed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// creates a new temporary file next to `target`, private to the user on Unix.
fn create_temporary(target: &Path) -> io::Result<(Temporary, File)> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let folder = target
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(crate::private::FILE_MODE);
    }
    let mut last = io::Error::from(io::ErrorKind::AlreadyExists);
    for _ in 0..NAME_ATTEMPTS {
        let path = folder.join(format!("{name}.{}{TEMPORARY_SUFFIX}", random_id()));
        match options.open(&path) {
            Ok(file) => {
                return Ok((
                    Temporary {
                        path,
                        renamed: false,
                    },
                    file,
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => last = error,
            Err(error) => return Err(error),
        }
    }
    Err(last)
}

/// 32 hexadecimal digits that differ for every call.
fn random_id() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    (0..2)
        .map(|_| {
            let part = RandomState::new().hash_one((
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now(),
            ));
            format!("{part:016x}")
        })
        .collect()
}

/// what a clean-up of a folder removed, and what it could not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cleanup {
    pub removed: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, io::ErrorKind)>,
}

/// removes the files in `folder` named `config.toml.<anything>.tmp`, which an
/// interrupted write leaves behind, and nothing else. only regular files go, not
/// links or folders. it is for a start, before anything writes: it cannot tell a
/// leftover from the temporary file of a write in progress.
pub fn remove_leftover_temporary_files(folder: &Path) -> io::Result<Cleanup> {
    let mut cleanup = Cleanup::default();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let leftover = entry.file_name().to_str().is_some_and(is_temporary_name)
            && entry.file_type().is_ok_and(|kind| kind.is_file());
        if !leftover {
            continue;
        }
        let path = entry.path();
        match fs::remove_file(&path) {
            Ok(()) => cleanup.removed.push(path),
            Err(error) => cleanup.failed.push((path, error.kind())),
        }
    }
    Ok(cleanup)
}

fn is_temporary_name(name: &str) -> bool {
    name.strip_prefix(CONFIG_FILE_NAME)
        .and_then(|rest| rest.strip_prefix('.'))
        .and_then(|rest| rest.strip_suffix(TEMPORARY_SUFFIX))
        .is_some_and(|random| !random.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "marshal-edit-replace-{}-{name}",
            std::process::id()
        ));
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

    fn permissions(path: &Path) -> fs::Permissions {
        fs::metadata(path).unwrap().permissions()
    }

    #[test]
    fn a_name_is_a_leftover_when_it_has_the_pattern_and_something_between() {
        for name in [
            "config.toml.0123.tmp",
            "config.toml.a.b.tmp",
            "config.toml.tmp.tmp",
        ] {
            assert!(is_temporary_name(name), "{name}");
        }
        for name in [
            "config.toml",
            "config.toml.tmp",
            "config.toml..tmp",
            "config.toml.0123.tmp.bak",
            "config.toml.0123",
            "config.json.0123.tmp",
            "xconfig.toml.0123.tmp",
            "config.toml0123.tmp",
        ] {
            assert!(!is_temporary_name(name), "{name}");
        }
    }

    #[test]
    fn a_refused_rename_removes_the_temporary_file_and_changes_nothing() {
        let dir = scratch("refused-rename");
        let path = dir.join("config.toml");
        fs::write(&path, b"old").unwrap();
        let error = replace_with(&path, b"old", &permissions(&path), b"new", |_, _| {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();
        assert_eq!(
            error,
            EditError::Io {
                path: path.clone(),
                operation: Operation::Replace,
                kind: io::ErrorKind::PermissionDenied,
            }
        );
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(names(&dir), ["config.toml"]);
    }

    #[test]
    fn a_file_that_differs_from_what_was_read_is_not_replaced() {
        let dir = scratch("changed");
        let path = dir.join("config.toml");
        fs::write(&path, b"someone else wrote this").unwrap();
        let error = replace(&path, b"old", &permissions(&path), b"new").unwrap_err();
        assert_eq!(error, EditError::Changed);
        assert_eq!(fs::read(&path).unwrap(), b"someone else wrote this");
        assert_eq!(names(&dir), ["config.toml"]);
    }

    #[test]
    fn a_file_that_is_gone_counts_as_changed() {
        let dir = scratch("gone");
        let path = dir.join("config.toml");
        fs::write(&path, b"old").unwrap();
        let kept = permissions(&path);
        fs::remove_file(&path).unwrap();
        assert_eq!(
            replace(&path, b"old", &kept, b"new").unwrap_err(),
            EditError::Changed
        );
        assert!(names(&dir).is_empty());
    }

    #[test]
    fn a_file_that_matches_is_replaced_whole() {
        let dir = scratch("replaced");
        let path = dir.join("config.toml");
        fs::write(&path, b"old").unwrap();
        replace(&path, b"old", &permissions(&path), b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(names(&dir), ["config.toml"]);
    }

    #[test]
    fn the_temporary_names_differ_and_hold_the_pattern() {
        let dir = scratch("names");
        let target = dir.join("config.toml");
        let first = create_temporary(&target).unwrap();
        let second = create_temporary(&target).unwrap();
        assert_ne!(first.0.path, second.0.path);
        for (temporary, _) in [&first, &second] {
            let name = temporary.path.file_name().unwrap().to_str().unwrap();
            assert!(is_temporary_name(name), "{name}");
        }
    }

    #[test]
    fn dropping_a_temporary_file_removes_it() {
        let dir = scratch("dropped");
        let (temporary, file) = create_temporary(&dir.join("config.toml")).unwrap();
        drop(file);
        drop(temporary);
        assert!(names(&dir).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_new_file_keeps_the_mode_of_the_old_one_and_the_temporary_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("mode");
        let path = dir.join("config.toml");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let (temporary, file) = create_temporary(&path).unwrap();
        let created = file.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(created & 0o077, 0, "{created:o}");
        drop(file);
        drop(temporary);
        replace(&path, b"old", &permissions(&path), b"new").unwrap();
        assert_eq!(permissions(&path).mode() & 0o777, 0o640);
    }

    #[test]
    fn the_clean_up_removes_only_the_matching_regular_files() {
        let dir = scratch("cleanup");
        let leftovers = ["config.toml.0001.tmp", "config.toml.0002.tmp"];
        let others = [
            "config.toml",
            "config.json",
            "config.toml.bak",
            "config.toml.tmp",
            "notes.tmp",
            "other.toml.0001.tmp",
        ];
        for name in leftovers.iter().chain(&others) {
            fs::write(dir.join(name), name.as_bytes()).unwrap();
        }
        fs::create_dir(dir.join("config.toml.0003.tmp")).unwrap();
        fs::write(dir.join("config.toml.0003.tmp").join("inside"), b"x").unwrap();

        let cleanup = remove_leftover_temporary_files(&dir).unwrap();

        let mut removed: Vec<_> = cleanup
            .removed
            .iter()
            .map(|path| path.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        removed.sort();
        assert_eq!(removed, leftovers);
        assert!(cleanup.failed.is_empty());
        let mut remaining: Vec<String> = others.iter().map(ToString::to_string).collect();
        remaining.push("config.toml.0003.tmp".to_string());
        remaining.sort();
        assert_eq!(names(&dir), remaining);
        assert!(dir.join("config.toml.0003.tmp").join("inside").exists());
    }

    #[test]
    fn the_clean_up_of_a_missing_folder_is_an_error() {
        let dir = scratch("cleanup-missing");
        assert!(remove_leftover_temporary_files(&dir.join("absent")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_clean_up_leaves_a_link_alone() {
        let dir = scratch("cleanup-link");
        fs::write(dir.join("target"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("target"), dir.join("config.toml.0001.tmp")).unwrap();
        let cleanup = remove_leftover_temporary_files(&dir).unwrap();
        assert!(cleanup.removed.is_empty());
        assert!(dir.join("config.toml.0001.tmp").symlink_metadata().is_ok());
    }
}
