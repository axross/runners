//! the fake `tart`, the fake `softnet` and the fake `docker` wired as a Tart backend
//! runs them: the backend's `tart` is the fake, `softnet` is found through the
//! client's `PATH`, and a command run in a VM is run by the fake `docker`.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use marshal_cli::macos::gate::NetworkGate;
use marshal_cli::macos::softnet::OwnerMode;
use marshal_cli::runtime::backend::{HostRequest, JobHost};
use marshal_cli::runtime::clock::Clock;
use marshal_cli::runtime::fake::MemoryLog;
use marshal_cli::runtime::tart::names::{GoldenVmName, job_vm_name};
use marshal_cli::runtime::tart::{TartBackend, TartTimings, VmLauncher};

use super::BINARY;
use super::docker::{FAKE_DOCKER, Invocation as DockerInvocation, invocations as docker_calls};
use super::softnet::Host as SoftnetHost;
use super::tart::{Fake, Invocation};

pub const START_MILLIS: u64 = 1_709_618_828_009;
pub const POOL: &str = "example-repo-one";
pub const SLOT: u32 = 2;
pub const LAUNCHER: &str = "/usr/local/bin/marshal-launch";

/// the time never moves, so a job VM's name is the same in every test, and a sleep
/// is real.
#[derive(Debug, Clone, Copy)]
pub struct TestClock;

impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(START_MILLIS)
    }

    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        tokio::time::sleep(duration)
    }
}

/// the time bounds of a test: every wait is short.
pub fn quick_timings() -> TartTimings {
    TartTimings {
        boot_deadline: Duration::from_secs(5),
        agent_poll: Duration::from_millis(20),
        agent_call: Duration::from_secs(5),
        engine_wait: Duration::from_secs(2),
        engine_poll: Duration::from_millis(50),
        engine_call: Duration::from_secs(5),
        probe: Duration::from_secs(5),
        tart_poll: Duration::from_millis(50),
        image_check: Duration::from_secs(5),
        release: Duration::from_secs(10),
        macos_boot_deadline: Duration::from_secs(5),
        place_poll: Duration::from_millis(50),
        disk_wait: Duration::from_secs(2),
        disk_poll: Duration::from_millis(50),
    }
}

/// the name of a golden VM this CLI runs, built at the test clock's time.
pub fn golden_name() -> String {
    GoldenVmName::current(TestClock.now()).unwrap().to_string()
}

/// the job VM the test clock names for the test slot.
pub fn job_vm() -> String {
    job_vm_name(POOL, SLOT, TestClock.now())
}

pub fn host() -> JobHost {
    JobHost {
        pool: POOL.to_string(),
        slot: SLOT,
    }
}

pub fn request() -> HostRequest {
    HostRequest {
        host: host(),
        vcpus: 4,
        memory_gb: 16,
        disk_gb: Some(60),
        diagnostic: false,
    }
}

/// what a rig is made of.
pub struct Setup {
    pub launcher: VmLauncher,
    pub timings: TartTimings,
    /// the owner and mode the fake `softnet` file is judged to have.
    pub owner_mode: OwnerMode,
    /// whether a `softnet` is on the client's `PATH`.
    pub softnet: bool,
    /// the `tart` program, the fake unless this names another.
    pub tart: Option<String>,
    /// the program the backend runs as its exporter child.
    pub exporter: Option<PathBuf>,
    /// the network gate the backend waits at, open when there is none.
    pub gate: Option<NetworkGate>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            launcher: VmLauncher::Direct,
            timings: quick_timings(),
            owner_mode: OwnerMode {
                uid: 0,
                mode: 0o4755,
            },
            softnet: true,
            tart: None,
            exporter: None,
            gate: None,
        }
    }
}

pub struct Rig {
    pub fake: Fake,
    pub softnet: SoftnetHost,
    pub log: Rc<MemoryLog>,
    pub backend: TartBackend<TestClock>,
}

impl Rig {
    pub fn new() -> Self {
        Self::with(VmLauncher::Direct, quick_timings())
    }

    pub fn with(launcher: VmLauncher, timings: TartTimings) -> Self {
        Self::build(&Setup {
            launcher,
            timings,
            ..Setup::default()
        })
    }

    /// a host whose `PATH` has no `softnet`.
    pub fn without_softnet() -> Self {
        Self::build(&Setup {
            softnet: false,
            ..Setup::default()
        })
    }

    /// a host whose `softnet` file has this owner and mode.
    pub fn with_softnet_file(uid: u32, mode: u32) -> Self {
        Self::build(&Setup {
            owner_mode: OwnerMode { uid, mode },
            ..Setup::default()
        })
    }

    /// a host whose backend runs this CLI as its exporter child, as `start` does.
    pub fn exporting() -> Self {
        Self::build(&Setup {
            exporter: Some(BINARY.into()),
            ..Setup::default()
        })
    }

    /// a host whose backend waits at this network gate.
    pub fn with_gate(gate: NetworkGate) -> Self {
        Self::build(&Setup {
            gate: Some(gate),
            ..Setup::default()
        })
    }

    /// a host whose `tart` is this program.
    pub fn with_tart_program(program: &str) -> Self {
        Self::build(&Setup {
            tart: Some(program.to_string()),
            ..Setup::default()
        })
    }

    fn build(setup: &Setup) -> Self {
        let softnet = SoftnetHost::new();
        let path = if setup.softnet {
            softnet.link("bin");
            softnet.path_with_system(&["bin"])
        } else {
            softnet.directory("bin");
            softnet.path(&["bin"])
        };
        let fake = Fake::with_config(|config| {
            config.path = Some(path.clone());
            if let Some(program) = &setup.tart {
                config.program = program.into();
            }
        });
        fake.seed(&[(&golden_name(), "stopped", "local")]);
        fake.script("docker_program", FAKE_DOCKER);
        fs::create_dir_all(fake.dir.path().join("docker")).unwrap();
        let log = Rc::new(MemoryLog::default());
        let mut backend = TartBackend::new(
            fake.client.clone(),
            setup.launcher.clone(),
            setup.timings,
            TestClock,
            log.clone(),
        )
        .with_softnet_owner_mode(setup.owner_mode)
        .with_exporter(setup.exporter.clone(), None);
        if let Some(gate) = &setup.gate {
            backend = backend.with_network_gate(gate.clone());
        }
        Self {
            fake,
            softnet,
            log,
            backend,
        }
    }

    /// sets a script file the fake `docker` in the VMs reads.
    pub fn docker_script(&self, file: &str, content: &str) {
        fs::write(self.docker_dir().join(file), content).unwrap();
    }

    pub fn docker_dir(&self) -> PathBuf {
        self.fake.dir.path().join("docker")
    }

    /// the calls the fake `docker` received in the VMs.
    pub fn docker_calls(&self) -> Vec<DockerInvocation> {
        docker_calls(&self.docker_dir())
    }

    /// the fake `docker`'s containers.
    pub fn containers(&self) -> Vec<String> {
        fs::read_to_string(self.docker_dir().join("containers"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split('\t').next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// the arguments of every `tart` call, in order.
    pub fn tart_calls(&self) -> Vec<Vec<String>> {
        self.fake
            .invocations()
            .into_iter()
            .map(|call| call.arguments)
            .collect()
    }

    /// the `tart` calls that begin with `subcommand`.
    pub fn tart_calls_of(&self, subcommand: &str) -> Vec<Invocation> {
        self.fake
            .invocations()
            .into_iter()
            .filter(|call| call.arguments.first().map(String::as_str) == Some(subcommand))
            .collect()
    }

    /// the lines of the log that start with `prefix`.
    pub fn log_lines(&self, prefix: &str) -> Vec<String> {
        self.log
            .lines()
            .into_iter()
            .filter(|line| line.starts_with(prefix))
            .collect()
    }
}
