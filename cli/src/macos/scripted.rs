//! the signal source a debug build reads from a directory named by
//! `MARSHAL_TEST_SIGNALS`, so a test can drive a running process from outside:
//!
//! - `sleep`: while the file exists, the next sample reports a sleep and removes it;
//! - `network`: `none` or a missing file is no default route, else lines
//!   `interface=<name>`, `router=<IPv4>` and `hardware=<six bytes>`, the last two
//!   optional;
//! - `keep_awake_failure`: while the file exists, the hold fails with the file's text;
//! - `keep_awake`: written by the source, `held`, `warning` or `released`.

use std::fs;
use std::future::Future;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::str::FromStr;

use super::fake::FakeSignals;
use super::keep_awake::HoldEvent;
use super::network::{NetworkReading, PrimaryNetwork, parse_hardware_address};
use super::signals::SignalSource;

/// the reading a `network` file describes; text that is not one is no default route.
#[must_use]
pub fn parse_scripted_network(text: &str) -> NetworkReading {
    let (mut interface, mut router, mut hardware) = (None, None, None);
    for line in text.lines() {
        match line.split_once('=') {
            Some(("interface", value)) => interface = Some(value.trim().to_string()),
            Some(("router", value)) => router = Ipv4Addr::from_str(value.trim()).ok(),
            Some(("hardware", value)) => hardware = parse_hardware_address(value.trim()),
            _ => {}
        }
    }
    match interface {
        Some(interface) => NetworkReading::Primary(PrimaryNetwork {
            interface,
            router,
            hardware,
        }),
        None => NetworkReading::NoDefaultRoute,
    }
}

pub struct ScriptedSignals {
    directory: PathBuf,
    state: FakeSignals,
}

impl ScriptedSignals {
    #[must_use]
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            state: FakeSignals::new(),
        }
    }

    fn pull(&self) {
        if fs::remove_file(self.directory.join("sleep")).is_ok() {
            self.state.sleep();
        }
        let network = fs::read_to_string(self.directory.join("network")).unwrap_or_default();
        self.state.set_network(parse_scripted_network(&network));
        let failure = fs::read_to_string(self.directory.join("keep_awake_failure")).ok();
        self.state.fail_keep_awake(failure.as_deref().map(|text| {
            if text.trim().is_empty() {
                "scripted"
            } else {
                text.trim()
            }
        }));
    }

    fn push(&self) {
        let state = if self.state.keep_awake_warning().is_some() {
            "warning"
        } else if self.state.is_held() {
            "held"
        } else {
            "released"
        };
        let _ = fs::write(self.directory.join("keep_awake"), state);
    }
}

impl SignalSource for ScriptedSignals {
    fn sleep_detected(&self) -> bool {
        self.pull();
        self.state.sleep_detected()
    }

    fn rearm_sleep(&self) {
        // a sleep file written while the instance was stopped is stale, as a sleep
        // of the Mac then would be
        let _ = fs::remove_file(self.directory.join("sleep"));
        self.state.rearm_sleep();
    }

    fn read_network(&self) -> impl Future<Output = NetworkReading> {
        self.pull();
        self.state.read_network()
    }

    fn hold_keep_awake(&self) -> HoldEvent {
        self.pull();
        let event = self.state.hold_keep_awake();
        self.push();
        event
    }

    fn poll_keep_awake(&self) -> HoldEvent {
        self.pull();
        let event = self.state.poll_keep_awake();
        self.push();
        event
    }

    fn release_keep_awake(&self) {
        self.state.release_keep_awake();
        self.push();
    }

    fn keep_awake_warning(&self) -> Option<String> {
        self.state.keep_awake_warning()
    }
}
