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

/// what one look at the watched path found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Same,
    /// another file, or the same one changed.
    Replaced,
    /// the path could not be read.
    Unreadable,
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

    /// what the path holds now compared with the file the agent started from.
    #[must_use]
    pub fn look(&self) -> Look {
        match std::fs::metadata(&self.path) {
            Ok(now) if Identity::of(&now) == self.identity => Look::Same,
            Ok(_) => Look::Replaced,
            Err(_) => Look::Unreadable,
        }
    }

    /// returns once the file has changed; never, without a file to watch. A file that
    /// cannot be read counts only when it is unreadable twice in a row, so one failed
    /// look at a busy disk or an antivirus scan does not end the agent.
    pub async fn wait_for_change(watched: Option<Self>, every: Duration) {
        let Some(watched) = watched else {
            return std::future::pending().await;
        };
        let mut unreadable = false;
        loop {
            tokio::time::sleep(every).await;
            match watched.look() {
                Look::Same => unreadable = false,
                Look::Replaced => return,
                Look::Unreadable if unreadable => return,
                Look::Unreadable => unreadable = true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("marshal-executable-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn an_untouched_file_is_unchanged_and_a_moved_renamed_replaced_or_deleted_one_is_not() {
        let directory = scratch("identity");
        let path = directory.join("agent");
        fs::write(&path, b"one").unwrap();
        let watched = Executable::at(&path).unwrap();
        assert_eq!(watched.look(), Look::Same);
        fs::rename(&path, directory.join("moved")).unwrap();
        assert_eq!(watched.look(), Look::Unreadable, "renamed");
        fs::write(&path, b"one").unwrap();
        assert_eq!(
            watched.look(),
            Look::Replaced,
            "a new file of the same name and size"
        );
        let replaced = Executable::at(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(replaced.look(), Look::Unreadable, "deleted");
        fs::remove_dir_all(&directory).unwrap();
    }

    const EVERY: Duration = Duration::from_millis(100);

    #[tokio::test(flavor = "current_thread")]
    async fn a_file_unreadable_for_one_look_is_forgiven_and_for_two_ends_the_wait() {
        let directory = scratch("forgiven");
        let path = directory.join("agent");
        let hidden = directory.join("hidden");
        fs::write(&path, b"one").unwrap();
        let watched = Executable::at(&path).unwrap();
        let wait = tokio::spawn(Executable::wait_for_change(Some(watched), EVERY));
        // away for the look at 100 ms and back before the one at 200 ms
        tokio::time::sleep(Duration::from_millis(40)).await;
        fs::rename(&path, &hidden).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        fs::rename(&hidden, &path).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!wait.is_finished(), "one failed look ended the wait");
        fs::remove_file(&path).unwrap();
        tokio::time::timeout(Duration::from_secs(2), wait)
            .await
            .expect("two failed looks did not end the wait")
            .unwrap();
        fs::remove_dir_all(&directory).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_replaced_file_ends_the_wait_at_the_next_look() {
        let directory = scratch("replaced");
        let path = directory.join("agent");
        fs::write(&path, b"one").unwrap();
        let watched = Executable::at(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"two!").unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            Executable::wait_for_change(Some(watched), EVERY),
        )
        .await
        .expect("a replaced file did not end the wait");
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_not_watched() {
        assert!(Executable::at(Path::new("/no/such/file")).is_none());
    }
}
