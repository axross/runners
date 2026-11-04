//! helpers shared by the integration tests; each test crate uses a subset.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[cfg(unix)]
pub mod axmc;
pub mod docker;
#[cfg(unix)]
pub mod golden;
pub mod host;
pub mod instance;
pub mod macos;
#[cfg(unix)]
pub mod rig;
pub mod softnet;
pub mod stub;
pub mod tart;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const BINARY: &str = env!("CARGO_BIN_EXE_marshal");

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

/// a directory under the system temporary directory that is removed when dropped.
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

    /// writes a token file private to the user, as macOS requires.
    pub fn write_token(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.write(relative, contents);
        make_private(&path);
        path
    }
}

/// runs `program` once, as `probe` configures it, retrying while the kernel refuses with
/// "text file busy". A child another test thread forks keeps a just-written file open
/// for writing until it execs, so the first run of a fresh executable can be refused;
/// once one run starts, no writer is left. The probe's exit code is ignored.
pub fn settle_executable(program: &Path, probe: impl Fn(&mut Command)) {
    const BUDGET: Duration = Duration::from_secs(10);
    let started = Instant::now();
    loop {
        let mut command = Command::new(program);
        probe(&mut command);
        let result = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match result {
            Ok(_) => return,
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && started.elapsed() < BUDGET =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{} could not run: {error}", program.display()),
        }
    }
}

/// writes a file and makes it private to the user, as macOS requires of a token file.
pub fn write_token_file(path: &Path, contents: &[u8]) {
    fs::write(path, contents).unwrap();
    make_private(path);
}

/// the names of a bundle's files, sorted.
pub fn files(bundle: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(bundle)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
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

/// a slot of a two-slot pool named `example-repo-one`, reading its token from `token`.
pub fn slot_spec(token: PathBuf) -> marshal_cli::runtime::pool::SlotSpec {
    marshal_cli::runtime::pool::SlotSpec {
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
        disk_gb: None,
        mounts: vec![marshal_cli::runtime::backend::Mount {
            volume: "example-repo-one-npm".to_string(),
            target: "/home/runner/.npm".to_string(),
        }],
        image: "actions-runner:local".to_string(),
        diagnostic: None,
    }
}

/// prepares the pools as the runtime core does, without its image check: admits
/// them, waits for Docker once when one is admitted, then prepares those admitted.
pub async fn prepare_pools<B: marshal_cli::runtime::backend::Backend>(
    backend: &B,
    pools: Vec<marshal_cli::runtime::pool::PoolSpec>,
    log: &dyn marshal_cli::runtime::log::Log,
    board: &marshal_cli::runtime::board::Board,
) -> Result<Vec<marshal_cli::runtime::pool::PoolSpec>, marshal_cli::runtime::pool::NoPoolStartable>
{
    use marshal_cli::runtime::pool::{NoPoolStartable, admit_pools, prepare_admitted};
    let admitted = admit_pools(pools, backend.engine_name(), log, board);
    if admitted.is_empty() {
        return Err(NoPoolStartable);
    }
    backend.wait_until_available().await;
    prepare_admitted(backend, admitted, log, board).await
}
