//! Where the lock, the control channel and the logs live.
//!
//! The roots are fixed for a host: on macOS a lock file in the shared directory and
//! a socket directory and a log directory under the user's library, on Windows two
//! named pipes and a log directory under the user's local application data. Only a
//! debug build can move them, for the tests, so a release binary cannot be pointed
//! at a lock, a channel or a log directory an attacker prepared.

use std::fmt;
#[cfg(debug_assertions)]
use std::path::Path;
use std::path::PathBuf;

/// Why a host has no roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootsError {
    /// Linux is not a supported host; a debug build can still be given a test root.
    UnsupportedHost,
    /// The user has no home directory to hold the socket directory.
    NoHomeDirectory,
    /// `LOCALAPPDATA` is not set, so the log directory has no place.
    NoLocalAppData,
}

impl fmt::Display for RootsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedHost => {
                "this operating system is not a supported host: only Windows and macOS are"
            }
            Self::NoHomeDirectory => "the current user has no home directory",
            Self::NoLocalAppData => "LOCALAPPDATA is not set, so the logs have no directory",
        })
    }
}

impl std::error::Error for RootsError {}

/// Where this process takes its lock and opens its channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    #[cfg(unix)]
    lock_file: PathBuf,
    #[cfg(unix)]
    channel_directory: PathBuf,
    #[cfg(windows)]
    pipe_suffix: String,
    log_directory: PathBuf,
}

impl Roots {
    /// The roots of the host this process runs on.
    ///
    /// # Errors
    ///
    /// See [`RootsError`].
    pub fn of_this_host() -> Result<Self, RootsError> {
        Self::host()
    }

    #[cfg(target_os = "macos")]
    fn host() -> Result<Self, RootsError> {
        let home = std::env::home_dir().ok_or(RootsError::NoHomeDirectory)?;
        Ok(Self {
            lock_file: PathBuf::from("/Users/Shared/.marshal-instance.lock"),
            channel_directory: home.join("Library/Application Support/marshal"),
            log_directory: home.join("Library/Logs/marshal"),
        })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn host() -> Result<Self, RootsError> {
        Err(RootsError::UnsupportedHost)
    }

    #[cfg(windows)]
    fn host() -> Result<Self, RootsError> {
        let local = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .ok_or(RootsError::NoLocalAppData)?;
        Ok(Self {
            pipe_suffix: String::new(),
            log_directory: PathBuf::from(local).join("marshal").join("logs"),
        })
    }

    /// Roots under `root`, for the tests. The directory must exist; a debug build
    /// only.
    #[cfg(all(unix, debug_assertions))]
    #[must_use]
    pub fn for_tests(root: &Path) -> Self {
        Self {
            lock_file: root.join(".marshal-instance.lock"),
            channel_directory: root.join("channel"),
            log_directory: root.join("logs"),
        }
    }

    /// Pipe names that carry a suffix derived from `root`, so tests that run side by
    /// side do not meet on one name. A debug build only.
    #[cfg(all(windows, debug_assertions))]
    #[must_use]
    pub fn for_tests(root: &Path) -> Self {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        root.hash(&mut hasher);
        Self {
            pipe_suffix: format!("-test-{:016x}", hasher.finish()),
            log_directory: root.join("logs"),
        }
    }

    /// The roots a caller asked for: the host's, unless a debug build was given a
    /// test root. A release build ignores the request, as it ignores a GitHub base
    /// address override.
    ///
    /// # Errors
    ///
    /// See [`RootsError`].
    pub fn resolve(test_root: Option<&std::path::Path>) -> Result<Self, RootsError> {
        #[cfg(debug_assertions)]
        if let Some(root) = test_root {
            return Ok(Self::for_tests(root));
        }
        #[cfg(not(debug_assertions))]
        let _ = test_root;
        Self::of_this_host()
    }

    /// The directory `start` writes its log files in; it must be private.
    #[must_use]
    pub fn log_directory(&self) -> &std::path::Path {
        &self.log_directory
    }
}

#[cfg(unix)]
impl Roots {
    #[must_use]
    pub fn lock_file(&self) -> &std::path::Path {
        &self.lock_file
    }

    /// The directory that holds the socket; it must be private.
    #[must_use]
    pub fn channel_directory(&self) -> &std::path::Path {
        &self.channel_directory
    }

    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.channel_directory.join("control.sock")
    }
}

#[cfg(windows)]
impl Roots {
    /// `\\.\pipe\marshal-instance`, which never carries a message.
    #[must_use]
    pub fn lock_pipe_name(&self) -> String {
        format!(r"\\.\pipe\marshal-instance{}", self.pipe_suffix)
    }

    /// `\\.\pipe\marshal-control-<user SID>`.
    #[must_use]
    pub fn control_pipe_name(&self, user_sid: &str) -> String {
        format!(r"\\.\pipe\marshal-control-{user_sid}{}", self.pipe_suffix)
    }
}
