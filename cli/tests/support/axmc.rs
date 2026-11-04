//! a scratch axmc host for running `marshal start` and its clients as real processes
//! against the fake `tart`, the fake `softnet` and, in the VMs, the fake `docker`.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime};

use marshal_cli::runtime::tart::names::GoldenVmName;

use super::docker::FAKE_DOCKER;
use super::host::{Started, TOKEN};
use super::softnet::Host as SoftnetHost;
use super::tart::FAKE_TART;
use super::{BINARY, TempDir};

/// the owner and mode the fake `softnet` file is judged to have unless a test says
/// otherwise.
pub const SET_UP_SOFTNET: &str = "0:4755";

/// the name of the directory a diagnostics-enabled pool exports into.
const SINK: &str = "diagnostics";

/// creates the sink private to the user, as the exporter requires.
fn make_sink(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

/// what the scripted signal source reads as a Wi-Fi primary network.
pub const WIFI_NETWORK: &str = "interface=en0\nrouter=192.0.2.1\nhardware=0:0:5e:0:53:1\n";

/// another Wi-Fi network: the same interface behind another router.
pub const OTHER_WIFI_NETWORK: &str = "interface=en0\nrouter=198.51.100.1\nhardware=0:0:5e:0:53:2\n";

/// a VPN that takes the default route.
pub const VPN_NETWORK: &str = "interface=utun4\n";

/// one pool of a configuration.
pub struct Pool {
    pub name: &'static str,
    pub os: &'static str,
    pub slots: u32,
    /// whether the pool has diagnostics on, into the host's private `diagnostics`
    /// directory.
    pub diagnostics: bool,
}

impl Pool {
    /// the same pool with diagnostics on.
    pub const fn diagnostic(mut self) -> Self {
        self.diagnostics = true;
        self
    }
}

pub const fn linux(name: &'static str, slots: u32) -> Pool {
    Pool {
        name,
        os: "linux",
        slots,
        diagnostics: false,
    }
}

pub const fn macos(name: &'static str, slots: u32) -> Pool {
    Pool {
        name,
        os: "macos",
        slots,
        diagnostics: false,
    }
}

pub struct MacHost {
    pub root: TempDir,
    pub softnet: SoftnetHost,
    /// the configuration file `start` is given.
    pub config: PathBuf,
    /// the `PATH` entries that hold the fake `softnet`, when the host has one.
    path: std::ffi::OsString,
    owner_mode: String,
    tart_program: std::ffi::OsString,
}

impl MacHost {
    /// a host with a fake `tart` that holds one golden VM, a fake `softnet` that is set
    /// up and the pools of `pools`, each with its token file.
    pub fn new(pools: &[Pool]) -> Self {
        let root = TempDir::new();
        let softnet = SoftnetHost::new();
        softnet.link("bin");
        let path = softnet.path_with_system(&["bin"]);
        for directory in ["tart", "state", "signals"] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        if pools.iter().any(|pool| pool.diagnostics) {
            make_sink(&root.path().join(SINK));
        }
        let config = root.write("host.json", config_for(root.path(), pools).as_bytes());
        for pool in pools {
            root.write_token(
                &format!("{}.token", pool.name),
                format!("{TOKEN}\n").as_bytes(),
            );
        }
        let host = Self {
            root,
            softnet,
            config,
            path,
            owner_mode: SET_UP_SOFTNET.to_string(),
            tart_program: FAKE_TART.into(),
        };
        let golden = GoldenVmName::current(SystemTime::now())
            .unwrap()
            .to_string();
        host.tart_script("vms", &format!("{golden}\tstopped\tlocal\t\t\t"));
        host.tart_script("docker_program", FAKE_DOCKER);
        host.set_network(WIFI_NETWORK);
        fs::create_dir_all(host.tart_dir().join("docker")).unwrap();
        host
    }

    /// a host whose `PATH` has no `softnet`.
    pub fn without_softnet(mut self) -> Self {
        self.path = self.softnet.path(&["nowhere"]);
        self
    }

    /// a host whose `softnet` file is judged to have this owner and mode.
    pub fn with_softnet_file(mut self, uid: u32, mode: u32) -> Self {
        self.owner_mode = format!("{uid}:{mode:o}");
        self
    }

    /// a host whose `tart` is this program.
    pub fn with_tart_program(mut self, program: &str) -> Self {
        self.tart_program = program.into();
        self
    }

    /// the private directory of a pool with diagnostics on.
    pub fn sink(&self) -> PathBuf {
        self.root.path().join(SINK)
    }

    /// the bundles in the sink, sorted.
    pub fn bundles(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(self.sink())
            .map(|entries| entries.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default();
        found.sort();
        found
    }

    pub fn tart_dir(&self) -> PathBuf {
        self.root.path().join("tart")
    }

    /// the fake `docker`'s directory, shared by every VM.
    pub fn docker_dir(&self) -> PathBuf {
        self.tart_dir().join("docker")
    }

    /// the directory the scripted signal source reads, which a test writes to drive
    /// the signals of a running instance.
    pub fn signals_dir(&self) -> PathBuf {
        self.root.path().join("signals")
    }

    /// sets the primary network the scripted signals report: `network` is the file's
    /// text, or `none` for no default route.
    pub fn set_network(&self, network: &str) {
        // renamed into place, so the instance never reads a half-written file
        let staged = self.signals_dir().join("network.staged");
        fs::write(&staged, network).unwrap();
        fs::rename(staged, self.signals_dir().join("network")).unwrap();
    }

    /// makes the scripted signals report a sleep at their next sample.
    pub fn sleep(&self) {
        fs::write(self.signals_dir().join("sleep"), "").unwrap();
    }

    /// makes the keep-awake hold fail with `text`, or work again with `None`.
    pub fn fail_keep_awake(&self, text: Option<&str>) {
        let file = self.signals_dir().join("keep_awake_failure");
        match text {
            Some(text) => fs::write(file, text).unwrap(),
            None => {
                let _ = fs::remove_file(file);
            }
        }
    }

    /// waits until the scripted source has written `expected` about the hold: `held`,
    /// `warning` or `released`. The file is rewritten in place, so a read can see it
    /// half written.
    pub fn wait_for_keep_awake(&self, expected: &str) {
        let started = Instant::now();
        let mut last = String::new();
        while started.elapsed() < Duration::from_secs(10) {
            last = fs::read_to_string(self.signals_dir().join("keep_awake")).unwrap_or_default();
            if last == expected {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the keep-awake hold was never {expected}; last: {last:?}");
    }

    pub fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    /// sets a script file the fake `tart` reads.
    pub fn tart_script(&self, file: &str, content: &str) {
        fs::write(self.tart_dir().join(file), content).unwrap();
    }

    /// removes a script file of the fake `tart`.
    pub fn remove_tart_script(&self, file: &str) {
        let _ = fs::remove_file(self.tart_dir().join(file));
    }

    /// sets a script file the fake `docker` reads.
    pub fn docker_script(&self, file: &str, content: &str) {
        fs::write(self.docker_dir().join(file), content).unwrap();
    }

    /// sets a script file the fake `softnet` reads.
    pub fn softnet_script(&self, file: &str, content: &str) {
        self.softnet.script("bin", file, content);
    }

    /// the command for `marshal <args>` on this host.
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BINARY);
        command
            .args(args)
            .current_dir(self.root.path())
            .env("MARSHAL_TEST_ROOT", self.state())
            .env("MARSHAL_TEST_TART", &self.tart_program)
            .env("MARSHAL_TEST_SIGNALS", self.signals_dir())
            .env("FAKE_TART_DIR", self.tart_dir())
            .env("MARSHAL_TEST_SOFTNET_OWNER_MODE", &self.owner_mode)
            .env("PATH", &self.path)
            .stdin(Stdio::null());
        command
    }

    /// runs a client, such as `status`, to its end.
    pub fn client(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// starts `marshal start` for the configuration, against a GitHub at `github`.
    pub fn start(&self, github: &str) -> Started {
        let mut command = self.command(&["start", "--profile", "axmc"]);
        command
            .arg("--config")
            .arg(&self.config)
            .env("MARSHAL_TEST_GITHUB_API", github)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Started::spawn(command)
    }

    /// runs `marshal start` as a client of a stopped instance, to its end.
    pub fn start_again(&self, github: &str) -> Output {
        let mut command = self.command(&["start", "--profile", "axmc"]);
        command
            .arg("--config")
            .arg(&self.config)
            .env("MARSHAL_TEST_GITHUB_API", github);
        command.output().unwrap()
    }

    /// `status`, once.
    pub fn status(&self) -> String {
        String::from_utf8_lossy(&self.client(&["status"]).stdout).into_owned()
    }

    /// asks `status` until its output satisfies `ready`, and returns that output.
    pub fn wait_for_status(
        &self,
        what: &str,
        within: Duration,
        ready: impl Fn(&str) -> bool,
    ) -> String {
        let started = Instant::now();
        let mut last = String::new();
        while started.elapsed() < within {
            last = self.status();
            if ready(&last) {
                return last;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "status never showed {what}; last:\n{last}\nthe tart calls: {:?}\nthe log:\n{}",
            self.tart_subcommands(),
            self.log_text()
        );
    }

    /// every log file `start` wrote, in order.
    pub fn log_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.state().join("logs"))
            .map(|entries| entries.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default();
        files.sort();
        files
    }

    pub fn log_text(&self) -> String {
        self.log_files()
            .iter()
            .map(|file| fs::read_to_string(file).unwrap_or_default())
            .collect()
    }

    /// the arguments of every `tart` call that has recorded them, in order. A call is
    /// read from its arguments file alone, which the fake writes last, so a call that
    /// is still being recorded is left out instead of read half written.
    pub fn tart_calls(&self) -> Vec<Vec<String>> {
        recorded_calls(&self.tart_dir())
    }

    /// the first word of every `tart` call, in order.
    pub fn tart_subcommands(&self) -> Vec<String> {
        self.tart_calls()
            .into_iter()
            .filter_map(|call| call.into_iter().next())
            .collect()
    }

    /// how many `tart` calls begin with `subcommand`.
    pub fn count_of(&self, subcommand: &str) -> usize {
        self.tart_subcommands()
            .iter()
            .filter(|word| *word == subcommand)
            .count()
    }

    /// the fake `docker`'s containers, by name.
    pub fn containers(&self) -> Vec<String> {
        fs::read_to_string(self.docker_dir().join("containers"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split('\t').next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// waits until the fake `docker` holds every one of `names`.
    pub fn wait_for_containers(&self, names: &[&String], within: Duration) {
        let started = Instant::now();
        while started.elapsed() < within {
            let held = self.containers();
            if names.iter().all(|name| held.contains(name)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the fake engine never held {names:?}; it holds {:?}",
            self.containers()
        );
    }

    /// the names of the VMs the fake `tart` holds.
    pub fn vm_names(&self) -> Vec<String> {
        fs::read_to_string(self.tart_dir().join("vms"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split('\t').next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// ends a job the way a runner's end does: removes its container from the fake
    /// engine, which makes the container's client exit.
    pub fn end_job(&self, container: &str) {
        let status = Command::new(FAKE_DOCKER)
            .args(["rm", "-f", container])
            .env("FAKE_DOCKER_DIR", self.docker_dir())
            .status()
            .unwrap();
        assert!(status.success());
    }
}

/// a configuration of `pools`, each of one repository whose token file is
/// `<name>.token` in `root`.
fn config_for(root: &Path, pools: &[Pool]) -> String {
    let entries: Vec<String> = pools
        .iter()
        .map(|pool| {
            format!(
                r#"{{"owner":"o","repository":"{name}","name":"{name}","os":"{os}","slots":{slots},
                "tokenPath":"{token}","cpus":2,"memoryGb":4,"diskGb":30{diagnostics}}}"#,
                name = pool.name,
                os = pool.os,
                slots = pool.slots,
                token = root.join(format!("{}.token", pool.name)).display(),
                diagnostics = if pool.diagnostics {
                    format!(
                        r#","diagnostics":true,"diagnosticDirectory":"{}""#,
                        root.join(SINK).display()
                    )
                } else {
                    String::new()
                },
            )
        })
        .collect();
    format!(
        r#"{{"budget":{{"cpus":32,"memoryGb":64,"diskGb":400}},"repositories":[{}]}}"#,
        entries.join(",")
    )
}

fn recorded_calls(dir: &Path) -> Vec<Vec<String>> {
    let mut calls = Vec::new();
    for number in 0.. {
        let argv = dir.join(format!("inv-{number:06}.argv"));
        let Ok(bytes) = fs::read(argv) else {
            break;
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if text.is_empty() {
            continue;
        }
        calls.push(text.split('\0').map(str::to_string).collect());
    }
    calls
}
