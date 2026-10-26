//! Files and directories only the current user may use.
//!
//! The control socket lives in a private directory, and on macOS a token file must be
//! private too. A private directory is a real directory, not a link, owned by the
//! current user with mode `0700`; a private file is a regular file owned by the
//! current user with mode `0600`. The checks read what the file system says and
//! never repair it, so a directory someone else made, or one that was opened up, is
//! refused rather than adopted.
//!
//! `std` has no call for the process's own user ID and `unsafe` is forbidden, so
//! [`current_uid`] reads it as the owner of a file the process creates.

use std::fmt;
use std::fs::{self, Metadata};
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

/// The mode of a private directory.
pub const DIRECTORY_MODE: u32 = 0o700;

/// The mode of a private file, a token file or a socket.
pub const FILE_MODE: u32 = 0o600;

/// The permission bits that count, including the special ones.
const MODE_MASK: u32 = 0o7777;

/// How many names [`current_uid`] tries for its file before giving up.
const UID_FILE_ATTEMPTS: usize = 8;

/// Why a path is not private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateError {
    Missing(PathBuf),
    NotADirectory(PathBuf),
    NotARegularFile(PathBuf),
    OwnedByAnother {
        path: PathBuf,
        owner: u32,
        expected: u32,
    },
    WrongMode {
        path: PathBuf,
        found: u32,
        wanted: u32,
    },
    Unreadable(PathBuf, io::ErrorKind),
}

impl fmt::Display for PrivateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "{} does not exist", path.display()),
            Self::NotADirectory(path) => {
                write!(
                    f,
                    "{} is not a directory (a link does not count)",
                    path.display()
                )
            }
            Self::NotARegularFile(path) => {
                write!(f, "{} is not a regular file", path.display())
            }
            Self::OwnedByAnother {
                path,
                owner,
                expected,
            } => write!(
                f,
                "{} is owned by user {owner}, not by the current user {expected}",
                path.display()
            ),
            Self::WrongMode {
                path,
                found,
                wanted,
            } => write!(
                f,
                "{} has mode {found:04o}, and {wanted:04o} is required",
                path.display()
            ),
            Self::Unreadable(path, kind) => {
                write!(f, "{} cannot be examined ({kind})", path.display())
            }
        }
    }
}

impl std::error::Error for PrivateError {}

fn examine(path: &Path) -> Result<Metadata, PrivateError> {
    fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            PrivateError::Missing(path.to_path_buf())
        } else {
            PrivateError::Unreadable(path.to_path_buf(), error.kind())
        }
    })
}

fn require_owner_and_mode(
    path: &Path,
    metadata: &Metadata,
    expected_uid: u32,
    wanted_mode: u32,
) -> Result<(), PrivateError> {
    if metadata.uid() != expected_uid {
        return Err(PrivateError::OwnedByAnother {
            path: path.to_path_buf(),
            owner: metadata.uid(),
            expected: expected_uid,
        });
    }
    let found = metadata.mode() & MODE_MASK;
    if found != wanted_mode {
        return Err(PrivateError::WrongMode {
            path: path.to_path_buf(),
            found,
            wanted: wanted_mode,
        });
    }
    Ok(())
}

/// Checks that `path` is a directory, not a link, owned by `uid`, with mode `0700`.
///
/// # Errors
///
/// See [`PrivateError`].
pub fn check_private_directory(path: &Path, uid: u32) -> Result<(), PrivateError> {
    let metadata = examine(path)?;
    if !metadata.file_type().is_dir() {
        return Err(PrivateError::NotADirectory(path.to_path_buf()));
    }
    require_owner_and_mode(path, &metadata, uid, DIRECTORY_MODE)
}

/// Creates `path` with mode `0700` when it does not exist, then checks it as
/// [`check_private_directory`] does. Its parent must exist. A directory that is
/// already there is checked, never changed.
///
/// # Errors
///
/// See [`PrivateError`].
pub fn ensure_private_directory(path: &Path, uid: u32) -> Result<(), PrivateError> {
    match fs::DirBuilder::new().mode(DIRECTORY_MODE).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(PrivateError::Missing(
                path.parent().unwrap_or(path).to_path_buf(),
            ));
        }
        Err(error) => return Err(PrivateError::Unreadable(path.to_path_buf(), error.kind())),
    }
    check_private_directory(path, uid)
}

/// Checks the metadata of the open file at `path`: a regular file owned by `uid`
/// with mode `0600`. The caller passes the metadata of the file it holds open, so
/// the answer is about the file it will read, not about whatever the path names
/// later.
///
/// # Errors
///
/// See [`PrivateError`].
pub fn check_private_file(path: &Path, metadata: &Metadata, uid: u32) -> Result<(), PrivateError> {
    if !metadata.file_type().is_file() {
        return Err(PrivateError::NotARegularFile(path.to_path_buf()));
    }
    require_owner_and_mode(path, metadata, uid, FILE_MODE)
}

/// The current user's ID, read as the owner of a file this process creates in the
/// temporary directory. It is read once.
///
/// # Errors
///
/// Fails when the temporary directory cannot hold a file.
pub fn current_uid() -> io::Result<u32> {
    static UID: OnceLock<u32> = OnceLock::new();
    if let Some(uid) = UID.get() {
        return Ok(*uid);
    }
    let uid = uid_from_new_file()?;
    Ok(*UID.get_or_init(|| uid))
}

fn uid_from_new_file() -> io::Result<u32> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    // a random part, so another user cannot create the names first
    let names = (0..UID_FILE_ATTEMPTS).map(|_| {
        let random = RandomState::new().hash_one(COUNTER.fetch_add(1, Ordering::Relaxed));
        format!(".marshal-uid-{}-{random:016x}", std::process::id())
    });
    uid_from_new_file_in(&std::env::temp_dir(), names)
}

/// Creates a file in `directory` under the first of `names` that is free, reads its
/// owner, and removes it. A name that is taken is skipped, never opened.
fn uid_from_new_file_in(
    directory: &Path,
    names: impl IntoIterator<Item = String>,
) -> io::Result<u32> {
    let mut last = io::Error::other("no name was tried");
    for name in names {
        let path = directory.join(name);
        let file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(FILE_MODE)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last = error;
                continue;
            }
            Err(error) => return Err(error),
        };
        let uid = file.metadata().map(|metadata| metadata.uid());
        drop(file);
        let _ = fs::remove_file(&path);
        return uid;
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("marshal-private-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn the_current_uid_is_the_one_the_system_reports() {
        let reported = std::process::Command::new("id").arg("-u").output().unwrap();
        let reported: u32 = String::from_utf8(reported.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(current_uid().unwrap(), reported);
        assert_eq!(current_uid().unwrap(), reported);
    }

    #[test]
    fn a_name_another_user_created_first_is_skipped_and_left_alone() {
        let parent = scratch("uid");
        let taken = parent.join("taken");
        fs::write(&taken, b"theirs").unwrap();
        let names = ["taken", "free"].map(String::from);

        assert_eq!(
            uid_from_new_file_in(&parent, names).unwrap(),
            current_uid().unwrap()
        );

        assert_eq!(fs::read(&taken).unwrap(), b"theirs");
        assert!(!parent.join("free").exists(), "the file was removed");
        let error = uid_from_new_file_in(&parent, ["taken".to_string()]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_missing_directory_is_created_0700_and_then_accepted_as_it_is() {
        let parent = scratch("create");
        let dir = parent.join("channel");
        let uid = current_uid().unwrap();
        ensure_private_directory(&dir, uid).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().mode() & MODE_MASK, 0o700);
        ensure_private_directory(&dir, uid).unwrap();
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_directory_that_is_open_to_others_is_refused_and_not_repaired() {
        let parent = scratch("open");
        let uid = current_uid().unwrap();
        for mode in [0o755, 0o750, 0o770, 0o701, 0o500, 0o1700] {
            let dir = parent.join(format!("d{mode:o}"));
            fs::create_dir(&dir).unwrap();
            set_mode(&dir, mode);
            let error = ensure_private_directory(&dir, uid).unwrap_err();
            assert!(
                matches!(error, PrivateError::WrongMode { found, wanted: 0o700, .. } if found == mode),
                "{mode:o}: {error}"
            );
            assert_eq!(fs::metadata(&dir).unwrap().mode() & MODE_MASK, mode);
        }
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_directory_owned_by_another_user_is_refused() {
        let parent = scratch("owner");
        let dir = parent.join("d");
        fs::create_dir(&dir).unwrap();
        set_mode(&dir, 0o700);
        let uid = current_uid().unwrap();
        assert_eq!(check_private_directory(&dir, uid), Ok(()));
        let error = check_private_directory(&dir, uid.wrapping_add(1)).unwrap_err();
        assert!(
            matches!(error, PrivateError::OwnedByAnother { owner, expected, .. }
                if owner == uid && expected == uid.wrapping_add(1)),
            "{error}"
        );
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_link_to_a_private_directory_is_not_a_private_directory() {
        let parent = scratch("link");
        let real = parent.join("real");
        fs::create_dir(&real).unwrap();
        set_mode(&real, 0o700);
        let link = parent.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let uid = current_uid().unwrap();
        assert!(matches!(
            check_private_directory(&link, uid),
            Err(PrivateError::NotADirectory(_))
        ));
        assert!(matches!(
            ensure_private_directory(&link, uid),
            Err(PrivateError::NotADirectory(_))
        ));
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_missing_parent_and_a_missing_directory_are_reported_as_missing() {
        let parent = scratch("missing");
        let uid = current_uid().unwrap();
        assert!(matches!(
            check_private_directory(&parent.join("absent"), uid),
            Err(PrivateError::Missing(_))
        ));
        assert!(matches!(
            ensure_private_directory(&parent.join("absent").join("deeper"), uid),
            Err(PrivateError::Missing(_))
        ));
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn a_file_must_be_regular_owned_by_the_user_and_mode_0600() {
        let parent = scratch("file");
        let file = parent.join("token");
        fs::write(&file, b"x").unwrap();
        let uid = current_uid().unwrap();
        for mode in [0o644, 0o640, 0o660, 0o606, 0o400, 0o700, 0o2600] {
            set_mode(&file, mode);
            let metadata = fs::metadata(&file).unwrap();
            assert!(
                matches!(check_private_file(&file, &metadata, uid),
                    Err(PrivateError::WrongMode { found, .. }) if found == mode),
                "{mode:o}"
            );
        }
        set_mode(&file, 0o600);
        let metadata = fs::metadata(&file).unwrap();
        assert_eq!(check_private_file(&file, &metadata, uid), Ok(()));
        assert!(matches!(
            check_private_file(&file, &metadata, uid.wrapping_add(1)),
            Err(PrivateError::OwnedByAnother { .. })
        ));
        let directory = fs::metadata(&parent).unwrap();
        assert!(matches!(
            check_private_file(&parent, &directory, uid),
            Err(PrivateError::NotARegularFile(_))
        ));
        fs::remove_dir_all(parent).unwrap();
    }
}
