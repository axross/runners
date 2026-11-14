//! the fake `tart`, `softnet` and `docker` wired as `build --profile axmc` runs them: the
//! build's `tart` is the fake, `softnet` is found through the client's `PATH`, and a
//! `docker` run in a VM is run by the fake `docker`.
#![cfg(unix)]
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use marshal_cli::build::golden::{GoldenBuild, GoldenTimings};
use marshal_cli::golden::pins::RETIRED_BASE_PINS;
use marshal_cli::image::contract::{CONTRACT_VERSION, RUNNER_VERSION_LABEL};
use marshal_cli::image::definition::declared_label;
use marshal_cli::macos::softnet::OwnerMode;
use marshal_cli::runtime::fake::MemoryLog;
use marshal_cli::runtime::tart::VmLauncher;
use marshal_cli::runtime::tart::boot::AgentWait;
use marshal_cli::runtime::tart::names::{GOLDEN_VM_VERSION, GoldenVmName};

use super::docker::FAKE_DOCKER;
use super::softnet::Host as SoftnetHost;
use super::tart::{FAKE_TART, Fake, invocations};
use super::{COMMANDS_HELPER, TempDir, repository_root};

pub const IMAGE: &str = "actions-runner:local";
pub const KEY_LISTING: &str = include_str!("../../src/golden/fixtures/docker-key-listing.txt");
pub const OWNER_MODE: &str = "0:4755";

/// when the old golden VM was built, in seconds since the epoch.
pub const OLD_BUILT_SECONDS: u64 = 1_704_067_200;

/// the golden VM of an earlier build, which this CLI runs.
pub fn old_golden() -> String {
    GoldenVmName::current(std::time::UNIX_EPOCH + Duration::from_secs(OLD_BUILT_SECONDS))
        .unwrap()
        .to_string()
}

/// asserts that `name` is a golden VM name whose three versions are this CLI's.
pub fn assert_names_this_cli(name: &str) {
    let parsed = GoldenVmName::parse(name).unwrap();
    assert_eq!(parsed.golden_version, GOLDEN_VM_VERSION, "{name}");
    assert_eq!(parsed.contract_version, CONTRACT_VERSION, "{name}");
    assert_eq!(
        Some(parsed.runner_version.as_str()),
        declared_label(RUNNER_VERSION_LABEL),
        "{name}"
    );
}

/// the time bounds of a test: every wait is short.
pub fn quick_timings() -> GoldenTimings {
    GoldenTimings {
        agent: AgentWait {
            deadline: Duration::from_secs(5),
            poll: Duration::from_millis(20),
            call: Duration::from_secs(5),
        },
        engine_wait: Duration::from_secs(1),
        engine_poll: Duration::from_millis(50),
        engine_call: Duration::from_secs(5),
        docker_call: Duration::from_secs(10),
        shutdown_grace: Duration::from_secs(5),
        shutdown_budget: Duration::from_secs(8),
        run_end: Duration::from_secs(5),
        leftover: Duration::from_secs(5),
        cleanup: Duration::from_secs(10),
        commit_grace_after_stop: Duration::from_secs(1),
        commit_listing: Duration::from_secs(4),
    }
}

pub struct Rig {
    pub fake: Fake,
    pub softnet: SoftnetHost,
    /// the `PATH` the build's `tart` calls carry.
    pub path: std::ffi::OsString,
    pub log: MemoryLog,
    pub timings: GoldenTimings,
    pub retired: Vec<&'static str>,
    pub launcher: VmLauncher,
    /// the owner and mode the fake `softnet` file is judged to have.
    pub owner_mode: OwnerMode,
    /// a scratch directory for the process tests.
    pub root: TempDir,
}

impl Rig {
    /// a host with one old golden VM, a `softnet` on the client's `PATH`, and a fake
    /// engine that builds an image with this CLI's contract.
    pub fn new() -> Self {
        Self::with_softnet(true)
    }

    /// a host whose `PATH` has no `softnet`.
    pub fn without_softnet() -> Self {
        Self::with_softnet(false)
    }

    fn with_softnet(installed: bool) -> Self {
        let softnet = SoftnetHost::new();
        let path = if installed {
            softnet.link("bin");
            softnet.path_with_system(&["bin"])
        } else {
            softnet.directory("bin");
            softnet.path(&["bin"])
        };
        let fake = Fake::with_config(|config| config.path = Some(path.clone()));
        fake.seed(&[(&old_golden(), "stopped", "local")]);
        fake.script("docker_program", FAKE_DOCKER);
        fs::create_dir_all(fake.dir.path().join("docker")).unwrap();
        let rig = Self {
            fake,
            softnet,
            path,
            log: MemoryLog::default(),
            timings: quick_timings(),
            retired: RETIRED_BASE_PINS.to_vec(),
            launcher: VmLauncher::Direct,
            owner_mode: OwnerMode {
                uid: 0,
                mode: 0o4755,
            },
            root: TempDir::new(),
        };
        rig.docker_script("build_contract", &CONTRACT_VERSION.to_string());
        rig.fake.script(
            "exec_stdout-marshal-list-key",
            KEY_LISTING.trim_end_matches('\n'),
        );
        rig
    }

    /// sets a script file the fake `docker` in the VMs reads.
    pub fn docker_script(&self, file: &str, content: &str) {
        fs::write(self.fake.dir.path().join("docker").join(file), content).unwrap();
    }

    /// runs the build to its end.
    pub async fn run(&self) -> Result<(), String> {
        let (_stop, receiver) = tokio::sync::watch::channel(None);
        self.run_until(receiver).await
    }

    pub async fn run_until(
        &self,
        stop: tokio::sync::watch::Receiver<Option<tokio::time::Instant>>,
    ) -> Result<(), String> {
        // boxed as the command boxes it, so the test's own future stays small
        Box::pin(
            GoldenBuild {
                client: self.fake.client.clone(),
                launcher: self.launcher.clone(),
                image: IMAGE,
                context: b"the build context".to_vec(),
                log: &self.log,
                timings: self.timings,
                retired: &self.retired,
                softnet_owner_mode: Some(self.owner_mode),
                stop,
            }
            .run(),
        )
        .await
    }

    /// every `tart` call, as words that name what it did: a provisioning call by its
    /// script, a `docker` call by its first words, any other by its arguments.
    pub fn calls(&self) -> Vec<String> {
        self.arguments()
            .iter()
            .map(|words| describe(words))
            .collect()
    }

    /// the arguments of every call recorded so far. A call still being recorded has no
    /// argument file yet, because the fake publishes it last by rename, so this stops
    /// there and can be called while the build runs.
    fn arguments(&self) -> Vec<Vec<String>> {
        invocations(self.fake.dir.path())
            .into_iter()
            .map(|call| call.arguments)
            .collect()
    }

    /// the position of the first call that starts with `prefix`.
    pub fn position(&self, prefix: &str) -> Option<usize> {
        self.calls()
            .iter()
            .position(|call| call.starts_with(prefix))
    }

    pub fn calls_starting(&self, prefix: &str) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| call.starts_with(prefix))
            .collect()
    }

    /// the local VMs the fake Tart holds.
    pub fn local_vms(&self) -> Vec<String> {
        self.fake
            .vms()
            .into_iter()
            .filter(|fields| fields[2] == "local")
            .map(|fields| fields[0].clone())
            .collect()
    }

    pub fn oci_images(&self) -> Vec<String> {
        self.fake
            .vms()
            .into_iter()
            .filter(|fields| fields[2] == "OCI")
            .map(|fields| fields[0].clone())
            .collect()
    }

    pub fn build_vms(&self) -> Vec<String> {
        self.local_vms()
            .into_iter()
            .filter(|name| name.starts_with("marshal-build-linux-"))
            .collect()
    }

    pub fn golden_vms(&self) -> Vec<String> {
        self.local_vms()
            .into_iter()
            .filter(|name| name.starts_with("marshal-golden-linux-"))
            .collect()
    }

    /// waits until a call starting with `prefix` has been made.
    pub async fn wait_for_call(&self, prefix: &str, within: Duration) {
        let started = Instant::now();
        while self.position(prefix).is_none() {
            assert!(
                started.elapsed() < within,
                "no call starting {prefix:?}; the calls: {:?}",
                self.calls()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// the command for `marshal <args>` with the fakes, on this rig's state.
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(COMMANDS_HELPER);
        command
            .args(args)
            .current_dir(self.root.path())
            .env("MARSHAL_TEST_ROOT", self.state())
            .env("MARSHAL_TEST_TART", FAKE_TART)
            .env("FAKE_TART_DIR", self.fake.dir.path())
            .env("MARSHAL_TEST_SOFTNET_OWNER_MODE", OWNER_MODE)
            .env("PATH", &self.path)
            .stdin(Stdio::null());
        command
    }

    pub fn state(&self) -> PathBuf {
        let state = self.root.path().join("state");
        fs::create_dir_all(&state).unwrap();
        state
    }

    /// writes the fixture `name` of the accepted axmc configurations and returns its path.
    pub fn config(&self, name: &str) -> PathBuf {
        let fixture = repository_root()
            .join("tests/host-configuration/rust/accepted")
            .join(name);
        self.root.write("host.json", &fs::read(fixture).unwrap())
    }

    /// starts the commands helper's `build --profile axmc` with its standard error in a
    /// file.
    pub fn spawn_build(&self, config: &std::path::Path) -> Spawned {
        let errors = self.root.path().join("build.stderr");
        let mut command = self.command(&["build", "--profile", "axmc", "--config"]);
        command
            .arg(config)
            .stdout(Stdio::null())
            .stderr(fs::File::create(&errors).unwrap());
        Spawned {
            child: command.spawn().unwrap(),
            errors,
        }
    }
}

/// a running `build` of the commands helper.
pub struct Spawned {
    pub child: Child,
    errors: PathBuf,
}

impl Spawned {
    pub fn stderr(&self) -> String {
        fs::read_to_string(&self.errors).unwrap_or_default()
    }

    /// waits up to `within` for the process to end and returns its exit code, or
    /// panics.
    pub fn finish(&mut self, within: Duration) -> Option<i32> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code();
            }
            assert!(
                started.elapsed() < within,
                "the build did not end within {within:?}; its output:\n{}",
                self.stderr()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn describe(words: &[String]) -> String {
    if words.first().map(String::as_str) == Some("exec") {
        if let Some(at) = words.iter().position(|word| word == "bash") {
            return format!("exec {}", words[at + 3]);
        }
        if let Some(at) = words.iter().position(|word| word == "docker") {
            let docker = &words[at + 1..];
            let count = if docker.first().map(String::as_str) == Some("image") {
                2
            } else {
                1
            };
            return format!("docker {}", docker[..count.min(docker.len())].join(" "));
        }
        return "exec ask".to_string();
    }
    words.join(" ")
}

/// the stack of the thread a test's build runs on. The debug build needs a little
/// under 2 MiB on Linux with the golden future boxed, and about 3.5 MiB without it,
/// which also fits in 4 MiB, so this is not a test of the production boxing. It
/// guards the boxed future's growth: a test fails once that need passes about twice
/// its measured size, before the command fails on the main thread's 8 MiB.
const STACK_BYTES: usize = 4 * 1024 * 1024;

/// runs a test's future on a thread of [`STACK_BYTES`], which is larger than a test
/// thread's 2 MiB, whose stack a debug build's polling of the whole build exhausts.
pub fn big_stack<F, Fut>(make: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let thread = std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(make());
        })
        .unwrap();
    if let Err(panic) = thread.join() {
        std::panic::resume_unwind(panic);
    }
}
