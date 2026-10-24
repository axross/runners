//! Helpers shared by the integration tests. Each test crate uses a subset.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod docker;
pub mod host;
pub mod instance;
pub mod stub;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const BINARY: &str = env!("CARGO_BIN_EXE_runners");

pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn run(args: &[&str]) -> Output {
    Command::new(BINARY).args(args).output().unwrap()
}

pub fn run_path(config: &Path, profile: Option<&str>) -> Output {
    let mut command = Command::new(BINARY);
    command.arg("validate").arg("--config").arg(config);
    if let Some(profile) = profile {
        command.arg("--profile").arg(profile);
    }
    command.output().unwrap()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A directory under the system temporary directory that is removed when dropped.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "cli-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.path.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        path
    }

    /// Writes a token file private to the user, as macOS requires.
    pub fn write_token(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.write(relative, contents);
        make_private(&path);
        path
    }
}

/// Writes a file and makes it private to the user, as macOS requires of a token file.
pub fn write_token_file(path: &Path, contents: &[u8]) {
    fs::write(path, contents).unwrap();
    make_private(path);
}

#[cfg(unix)]
fn make_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(not(unix))]
fn make_private(_path: &Path) {}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A slot of a two-slot pool named `example-repo-one`, reading its token from `token`.
pub fn slot_spec(token: PathBuf) -> runners_cli::runtime::pool::SlotSpec {
    runners_cli::runtime::pool::SlotSpec {
        pool: "example-repo-one".to_string(),
        owner: "example-owner".to_string(),
        repository: "example-repo-one".to_string(),
        number: 2,
        token_path: token,
        labels: vec![
            "self-hosted".to_string(),
            "linux".to_string(),
            "x64".to_string(),
            "axpc".to_string(),
        ],
        cpus: "1.5".to_string(),
        cpu_count: 2,
        cpu_offset: 2,
        memory_gb: 16,
        mounts: vec![runners_cli::runtime::backend::Mount {
            volume: "example-repo-one-npm".to_string(),
            target: "/home/runner/.npm".to_string(),
        }],
        image: "actions-runner:local".to_string(),
    }
}
