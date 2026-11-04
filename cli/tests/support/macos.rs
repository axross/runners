//! a scratch directory, the fake macOS programs, and the configurations that point
//! the signals at them.
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

use marshal_cli::macos::keep_awake::KeepAwakeConfig;
use marshal_cli::macos::network::NetworkTools;

use super::TempDir;

pub const FAKE_MACOS: &str = env!("CARGO_BIN_EXE_fake-macos");

/// `route -n get default` on a Wi-Fi network, as macOS prints it.
pub const ROUTE_WIFI: &str = "   route to: default
destination: default
       mask: default
    gateway: 192.0.2.1
  interface: en0
      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>
 recvpipe  sendpipe  ssthresh  rtt,msec    rttvar  hopcount      mtu     expire
       0         0         0         0         0         0      1500         0
";

/// `arp -n 192.0.2.1` for that router, as macOS prints it.
pub const ARP_ROUTER: &str = "? (192.0.2.1) at 0:0:5e:0:53:1 on en0 ifscope [ethernet]\n";

pub struct Fake {
    pub dir: TempDir,
}

impl Fake {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new(),
        }
    }

    /// a fake that answers `route` with the Wi-Fi network and `arp` with its router.
    pub fn wifi() -> Self {
        let fake = Self::new();
        fake.script("route_stdout", ROUTE_WIFI);
        fake.script("arp_stdout", ARP_ROUTER);
        fake
    }

    pub fn script(&self, file: &str, content: &str) {
        fs::write(self.dir.path().join(file), content).unwrap();
    }

    pub fn unscript(&self, file: &str) {
        let _ = fs::remove_file(self.dir.path().join(file));
    }

    pub fn environment(&self) -> Vec<(OsString, OsString)> {
        vec![(
            OsString::from("FAKE_MACOS_DIR"),
            self.dir.path().as_os_str().to_owned(),
        )]
    }

    pub fn tools(&self) -> NetworkTools {
        NetworkTools {
            route: OsString::from(FAKE_MACOS),
            arp: OsString::from(FAKE_MACOS),
            environment: self.environment(),
            ..NetworkTools::default()
        }
    }

    pub fn keep_awake(&self, retry_interval: Duration) -> KeepAwakeConfig {
        KeepAwakeConfig {
            program: OsString::from(FAKE_MACOS),
            environment: self.environment(),
            retry_interval,
        }
    }

    pub fn invocations(&self) -> Vec<Invocation> {
        invocations(self.dir.path())
    }

    /// the calls the fake has recorded once there are at least `count` of them: a
    /// program a hold has spawned records itself after the spawn returns.
    pub fn invocations_after(&self, count: usize) -> Vec<Invocation> {
        let started = std::time::Instant::now();
        loop {
            let found = self.invocations();
            if found.len() >= count || started.elapsed() > Duration::from_secs(10) {
                return found;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// whether call `number` ends within `within`, seen through the lock it holds
    /// while it lives.
    pub fn ended_within(&self, number: usize, within: Duration) -> bool {
        let alive = self.dir.path().join(format!("inv-{number:06}.alive"));
        let (ended, wait) = mpsc::channel();
        std::thread::spawn(move || {
            let file = fs::File::open(alive).unwrap();
            file.lock().unwrap();
            let _ = ended.send(());
        });
        wait.recv_timeout(within).is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub arguments: Vec<String>,
    pub pid: u32,
    /// the process group the call ran in, on Unix.
    pub pgid: Option<u32>,
    pub jit_present: bool,
    pub stdin_terminal: bool,
    pub stdout_terminal: bool,
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
        let flag = |name: &str| {
            environment
                .lines()
                .any(|line| line == format!("{name}=true"))
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
            jit_present: environment.lines().any(|line| line == "JIT=present"),
            stdin_terminal: flag("stdin_terminal"),
            stdout_terminal: flag("stdout_terminal"),
        });
    }
    found
}
