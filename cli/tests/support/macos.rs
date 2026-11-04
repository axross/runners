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
use std::time::Duration;

use marshal_cli::macos::keep_awake::KeepAwakeConfig;
use marshal_cli::macos::network::NetworkTools;

use super::TempDir;
use super::tart::{Invocation, ended_within, invocations};

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
        ended_within(self.dir.path(), number, within)
    }
}
