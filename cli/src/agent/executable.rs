//! notices when the file the agent started from is moved, renamed, replaced or
//! deleted, so an agent never goes on running a program that is no longer the one
//! installed.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// how often the file is looked at; the agent ends within 5 s of a change.
pub const CHECK_EVERY: Duration = Duration::from_secs(1);

/// what makes a file this file and not another at the same path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    file: (u64, u64),
    #[cfg(windows)]
    created: u64,
}

impl Identity {
    fn of(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        #[cfg(windows)]
        use std::os::windows::fs::MetadataExt;
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            file: (metadata.dev(), metadata.ino()),
            #[cfg(windows)]
            created: metadata.creation_time(),
        }
    }
}

/// the file the agent started from.
#[derive(Debug, Clone)]
pub struct Executable {
    path: PathBuf,
    identity: Identity,
}

impl Executable {
    /// the file at `path` as it is now, or `None` when it cannot be read, in which
    /// case nothing is watched.
    #[must_use]
    pub fn at(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            identity: Identity::of(&metadata),
        })
    }

    /// whether the path no longer holds the file it did.
    #[must_use]
    pub fn changed(&self) -> bool {
        std::fs::metadata(&self.path).map_or(true, |now| Identity::of(&now) != self.identity)
    }

    /// returns once the file has changed; never, without a file to watch.
    pub async fn wait_for_change(watched: Option<Self>, every: Duration) {
        let Some(watched) = watched else {
            return std::future::pending().await;
        };
        loop {
            tokio::time::sleep(every).await;
            if watched.changed() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn scratch() -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("marshal-executable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn an_untouched_file_is_unchanged_and_a_moved_renamed_replaced_or_deleted_one_is_not() {
        let directory = scratch();
        let path = directory.join("agent");
        fs::write(&path, b"one").unwrap();
        let watched = Executable::at(&path).unwrap();
        assert!(!watched.changed());
        fs::rename(&path, directory.join("moved")).unwrap();
        assert!(watched.changed(), "renamed");
        fs::write(&path, b"one").unwrap();
        assert!(watched.changed(), "a new file of the same name and size");
        let replaced = Executable::at(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(replaced.changed(), "deleted");
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_not_watched() {
        assert!(Executable::at(Path::new("/no/such/file")).is_none());
    }
}
