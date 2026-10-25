//! A scratch directory and a [`DockerBackend`] pointed at the fake `docker`.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsString;
use std::path::Path;
use std::rc::Rc;

use runners_cli::runtime::docker::{DockerBackend, DockerConfig, DockerLimits};
use runners_cli::runtime::fake::{FakeClock, MemoryLog};

use super::TempDir;

pub const FAKE_DOCKER: &str = env!("CARGO_BIN_EXE_fake-docker");

pub struct Fake {
    pub dir: TempDir,
    pub log: Rc<MemoryLog>,
    pub backend: DockerBackend<Rc<FakeClock>>,
    pub clock: Rc<FakeClock>,
}

impl Fake {
    pub fn new() -> Self {
        Self::with_limits(DockerLimits::default())
    }

    pub fn with_limits(limits: DockerLimits) -> Self {
        let dir = TempDir::new();
        let log = Rc::new(MemoryLog::default());
        let clock = Rc::new(FakeClock::at(1_709_618_828_009));
        let config = DockerConfig {
            program: OsString::from(FAKE_DOCKER),
            environment: vec![(
                OsString::from("FAKE_DOCKER_DIR"),
                dir.path().as_os_str().to_owned(),
            )],
            limits,
            ..DockerConfig::default()
        };
        let backend = DockerBackend::new(config, clock.clone(), log.clone());
        Self {
            dir,
            log,
            backend,
            clock,
        }
    }

    /// Sets a script file the fake reads.
    pub fn script(&self, file: &str, content: &str) {
        std::fs::write(self.dir.path().join(file), content).unwrap();
    }

    /// The containers the fake engine holds, as `name` lines.
    pub fn containers(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("containers"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split('\t').next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Every call the fake received, in order: its arguments and whether the JIT
    /// variable was set, and to what.
    pub fn invocations(&self) -> Vec<Invocation> {
        invocations(self.dir.path())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub arguments: Vec<String>,
    pub jit: Option<String>,
    pub pid: u32,
    /// The process group the call ran in, on Unix.
    pub pgid: Option<u32>,
}

pub fn invocations(dir: &Path) -> Vec<Invocation> {
    let mut found = Vec::new();
    for number in 0.. {
        let stem = dir.join(format!("inv-{number:06}"));
        let Ok(argv) = std::fs::read(stem.with_extension("argv")) else {
            break;
        };
        let text = String::from_utf8(argv).unwrap();
        found.push(Invocation {
            arguments: if text.is_empty() {
                Vec::new()
            } else {
                text.split('\0').map(str::to_string).collect()
            },
            pgid: std::fs::read_to_string(stem.with_extension("pgid"))
                .ok()
                .and_then(|text| text.trim().parse().ok()),
            jit: std::fs::read_to_string(stem.with_extension("jit")).ok(),
            pid: std::fs::read_to_string(stem.with_extension("pid"))
                .ok()
                .and_then(|text| text.parse().ok())
                .unwrap_or(0),
        });
    }
    found
}
