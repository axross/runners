//! a scratch directory and a [`TartClient`] pointed at the fake `tart`.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use marshal_cli::runtime::tart::{TartClient, TartConfig};

use super::TempDir;

pub const FAKE_TART: &str = env!("CARGO_BIN_EXE_fake-tart");

/// the marker the client's `PATH` ends with, so a test can tell it from a `PATH` the
/// fake inherited.
pub const PATH_MARKER: &str = "marshal-test-path-marker";

pub struct Fake {
    pub dir: TempDir,
    pub client: TartClient,
    pub path: OsString,
}

impl Fake {
    pub fn new() -> Self {
        Self::with_config(|_| {})
    }

    /// a client whose configuration `adjust` has changed.
    pub fn with_config(adjust: impl FnOnce(&mut TartConfig)) -> Self {
        let dir = TempDir::new();
        let mut directories: Vec<_> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect();
        directories.push(PATH_MARKER.into());
        let path = std::env::join_paths(directories).unwrap();
        let mut config = TartConfig {
            program: OsString::from(FAKE_TART),
            environment: vec![(
                OsString::from("FAKE_TART_DIR"),
                dir.path().as_os_str().to_owned(),
            )],
            path: Some(path.clone()),
            ..TartConfig::default()
        };
        adjust(&mut config);
        Self {
            dir,
            client: TartClient::new(config),
            path,
        }
    }

    /// sets a script file the fake reads.
    pub fn script(&self, file: &str, content: &str) {
        fs::write(self.dir.path().join(file), content).unwrap();
    }

    /// gives the fake Tart these VMs: a name, a state and a source each.
    pub fn seed(&self, vms: &[(&str, &str, &str)]) {
        let lines: Vec<String> = vms
            .iter()
            .map(|(name, state, source)| format!("{name}\t{state}\t{source}\t\t\t"))
            .collect();
        self.script("vms", &lines.join("\n"));
    }

    /// the fake Tart's VMs, as the tab-separated lines it stores.
    pub fn vms(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.dir.path().join("vms"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.split('\t').map(str::to_string).collect())
            .collect()
    }

    /// the names of the VMs the fake Tart holds, in the order it stores them.
    pub fn vm_names(&self) -> Vec<String> {
        self.vms()
            .into_iter()
            .map(|fields| fields[0].clone())
            .collect()
    }

    /// every call the fake received, in order.
    pub fn invocations(&self) -> Vec<Invocation> {
        invocations(self.dir.path())
    }

    /// whether call `number` ends within `within`, seen through the lock it holds
    /// while it lives.
    pub fn ended_within(&self, number: usize, within: Duration) -> bool {
        ended_within(self.dir.path(), number, within)
    }
}

/// whether call `number` recorded in `dir` ends within `within`, seen through the
/// lock it holds while it lives.
pub fn ended_within(dir: &Path, number: usize, within: Duration) -> bool {
    let alive = dir.join(format!("inv-{number:06}.alive"));
    let (ended, wait) = mpsc::channel();
    std::thread::spawn(move || {
        let file = fs::File::open(alive).unwrap();
        file.lock().unwrap();
        let _ = ended.send(());
    });
    wait.recv_timeout(within).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub arguments: Vec<String>,
    pub pid: u32,
    /// the process group the call ran in, on Unix.
    pub pgid: Option<u32>,
    pub no_auto_prune: Option<String>,
    pub path: Option<String>,
    pub jit_present: bool,
    pub stdin_terminal: bool,
    pub stdout_terminal: bool,
    pub stderr_terminal: bool,
    /// what `exec -i` read from its standard input.
    pub stdin: Option<Vec<u8>>,
}

pub fn invocations(dir: &Path) -> Vec<Invocation> {
    let mut found = Vec::new();
    for number in 0.. {
        let stem = dir.join(format!("inv-{number:06}"));
        let Ok(argv) = fs::read(stem.with_extension("argv")) else {
            break;
        };
        let text = String::from_utf8(argv).unwrap();
        let environment = fs::read_to_string(stem.with_extension("env")).unwrap();
        let value = |name: &str| {
            environment
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{name}=")))
                .map(str::to_string)
        };
        found.push(Invocation {
            arguments: if text.is_empty() {
                Vec::new()
            } else {
                text.split('\0').map(str::to_string).collect()
            },
            pid: fs::read_to_string(stem.with_extension("pid"))
                .unwrap()
                .parse()
                .unwrap(),
            pgid: fs::read_to_string(stem.with_extension("pgid"))
                .ok()
                .and_then(|text| text.trim().parse().ok()),
            no_auto_prune: value("TART_NO_AUTO_PRUNE"),
            path: value("PATH"),
            jit_present: environment.lines().any(|line| line == "JIT=present"),
            stdin_terminal: value("stdin_terminal").as_deref() == Some("true"),
            stdout_terminal: value("stdout_terminal").as_deref() == Some("true"),
            stderr_terminal: value("stderr_terminal").as_deref() == Some("true"),
            stdin: fs::read(stem.with_extension("stdin")).ok(),
        });
    }
    found
}
