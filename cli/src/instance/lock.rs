//! The machine-wide single-instance lock.
//!
//! It is taken before the configuration is validated or anything is cleaned up, and
//! the operating system releases it when the process ends, however it ends. No child
//! process inherits it.
//!
//! On Windows the lock is the first instance of the named pipe
//! `\\.\pipe\runners-instance`, which only the creating user may connect to. The
//! pipe is kept open for the life of the process and never carries a message. A
//! second creation of the name, by any user, fails, and that failure is "held".
//!
//! On Unix it is an exclusive advisory lock on a file that every user may open: on
//! macOS `/Users/Shared/.runners-instance.lock`. The creator sets its mode to `0644`
//! whatever the umask. A user who may not write it opens it read-only, and the lock
//! then refuses that user rather than the open.

use std::fmt;

use super::roots::Roots;

/// Why the lock was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Another process holds the lock.
    Held,
    /// The lock file was replaced or removed after it was locked, so the lock no
    /// longer excludes anyone.
    Lost,
    /// The lock could not be opened or created at all.
    CannotOpen(String),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Held => f.write_str("the instance lock is held by another process"),
            Self::Lost => f.write_str(
                "the instance lock file was replaced or removed after it was locked, \
                 so the lock is lost",
            ),
            Self::CannotOpen(detail) => {
                write!(f, "the instance lock cannot be opened: {detail}")
            }
        }
    }
}

impl std::error::Error for LockError {}

/// The held lock. Dropping it releases it.
#[derive(Debug)]
pub struct InstanceLock {
    _holder: Holder,
}

impl InstanceLock {
    /// Takes the lock for this machine, or says why it could not.
    ///
    /// # Errors
    ///
    /// See [`LockError`].
    pub fn acquire(roots: &Roots) -> Result<Self, LockError> {
        Ok(Self {
            _holder: acquire(roots)?,
        })
    }
}

#[cfg(unix)]
use unix::{Holder, acquire};
#[cfg(windows)]
use windows::{Holder, acquire};

#[cfg(unix)]
mod unix {
    use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
    use std::io;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::Path;

    use super::{LockError, Roots};

    /// Everyone may read the lock file, so everyone can try the lock.
    const LOCK_FILE_MODE: u32 = 0o644;

    /// How often creating the file races with its removal before giving up.
    const OPEN_ATTEMPTS: usize = 3;

    /// The open file; the lock lasts as long as it does.
    #[derive(Debug)]
    pub struct Holder {
        _file: File,
    }

    fn cannot_open(path: &Path, error: &io::Error) -> LockError {
        LockError::CannotOpen(format!("{}: {error}", path.display()))
    }

    /// Creates the file with mode `0644` regardless of the umask, or opens the one
    /// that exists, for writing when the user may and read-only when not.
    fn open(path: &Path) -> Result<File, LockError> {
        let mut last = io::Error::other("no attempt was made");
        for _ in 0..OPEN_ATTEMPTS {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(LOCK_FILE_MODE)
                .open(path)
            {
                Ok(file) => {
                    file.set_permissions(Permissions::from_mode(LOCK_FILE_MODE))
                        .map_err(|error| cannot_open(path, &error))?;
                    return Ok(file);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(cannot_open(path, &error)),
            }
            match OpenOptions::new().write(true).open(path) {
                Ok(file) => return Ok(file),
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    return OpenOptions::new()
                        .read(true)
                        .open(path)
                        .map_err(|error| cannot_open(path, &error));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => last = error,
                Err(error) => return Err(cannot_open(path, &error)),
            }
        }
        Err(cannot_open(path, &last))
    }

    /// Whether `path` still names the file `file` is. A link, a missing path, and a
    /// different file all say no.
    fn still_named(file: &File, path: &Path) -> Result<bool, LockError> {
        let locked = file.metadata().map_err(|error| cannot_open(path, &error))?;
        match fs::symlink_metadata(path) {
            Ok(named) => Ok(named.file_type().is_file()
                && named.dev() == locked.dev()
                && named.ino() == locked.ino()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(cannot_open(path, &error)),
        }
    }

    pub fn acquire(roots: &Roots) -> Result<Holder, LockError> {
        let path = roots.lock_file();
        let file = open(path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(LockError::Held),
            Err(TryLockError::Error(error)) => return Err(cannot_open(path, &error)),
        }
        if !still_named(&file, path)? {
            return Err(LockError::Lost);
        }
        Ok(Holder { _file: file })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn scratch(name: &str) -> std::path::PathBuf {
            let dir =
                std::env::temp_dir().join(format!("runners-lock-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            dir
        }

        #[test]
        fn a_path_names_the_locked_file_only_while_it_is_that_file() {
            let dir = scratch("named");
            let path = dir.join("lock");
            let file = open(&path).unwrap();
            assert!(still_named(&file, &path).unwrap());

            fs::remove_file(&path).unwrap();
            assert!(!still_named(&file, &path).unwrap(), "removed");

            fs::write(&path, b"").unwrap();
            assert!(
                !still_named(&file, &path).unwrap(),
                "replaced by another file"
            );

            fs::remove_file(&path).unwrap();
            let other = dir.join("other");
            fs::write(&other, b"").unwrap();
            std::os::unix::fs::symlink(&other, &path).unwrap();
            let through_link = OpenOptions::new().read(true).open(&path).unwrap();
            assert!(
                !still_named(&through_link, &path).unwrap(),
                "a link is not the file"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::num::NonZeroU8;

    use interprocess::os::windows::named_pipe::{
        PipeListener, PipeListenerOptions, PipeMode, pipe_mode,
    };

    use super::super::windows::{current_user_sid, name_is_taken, only_user_descriptor};
    use super::{LockError, Roots};

    #[derive(Debug)]
    pub struct Holder {
        _pipe: PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    }

    pub fn acquire(roots: &Roots) -> Result<Holder, LockError> {
        let detail = |error: std::io::Error| LockError::CannotOpen(error.to_string());
        let descriptor =
            only_user_descriptor(&current_user_sid().map_err(detail)?).map_err(detail)?;
        let name = roots.lock_pipe_name();
        let created = PipeListenerOptions::new()
            .path(OsStr::new(&name))
            .mode(PipeMode::Bytes)
            .instance_limit(NonZeroU8::new(1))
            .security_descriptor(Some(descriptor))
            .create_duplex::<pipe_mode::Bytes>();
        match created {
            Ok(pipe) => Ok(Holder { _pipe: pipe }),
            Err(error) if name_is_taken(&error) => Err(LockError::Held),
            Err(error) => Err(detail(error)),
        }
    }
}
