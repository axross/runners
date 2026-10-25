//! A scratch host for running `runners start` and its clients as real processes:
//! a test root for the lock, channel and logs, a configuration with one pool, its
//! token file, and the fake `docker` first on `PATH`.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::docker::FAKE_DOCKER;
use super::stub::{Reply, Stub};
use super::{BINARY, TempDir};

/// The token the pool reads.
pub const TOKEN: &str = "ghp_hosttoken0123456789abcdef";

/// The JIT configuration the stub hands out.
pub const JIT: &str = "jit-config-secret-0123456789";

/// The stub's answer to a registration.
pub fn registered() -> Reply {
    Reply::json(&format!(
        r#"{{"runner":{{"id":7}},"encoded_jit_config":"{JIT}"}}"#
    ))
}

pub struct Host {
    pub root: TempDir,
    /// The configuration file `start` is given.
    pub config: PathBuf,
}

/// The token path the configuration names. On Unix an axpc path is a relative name
/// there, read from the working directory, which is the host's root.
#[cfg(unix)]
fn token_path(_root: &Path) -> (String, PathBuf) {
    (r"C:\\r.token".to_string(), PathBuf::from(r"C:\r.token"))
}

#[cfg(windows)]
fn token_path(root: &Path) -> (String, PathBuf) {
    let path = root.join("r.token");
    (path.to_str().unwrap().replace('\\', r"\\"), path)
}

/// A configuration with one pool, `r`, of one slot.
pub fn config_for(token: &str) -> String {
    format!(
        r#"{{"imageName":"actions-runner:local","repositories":[{{"owner":"o","repository":"r",
        "name":"r","slots":1,"tokenPath":"{token}","volumes":[]}}]}}"#
    )
}

/// A configuration with pool `r`, as [`config_for`] writes it, and a second pool,
/// `d`, whose diagnostics go to `directory`. `d` is never started, so its token
/// file, `d.token` beside `r`'s, need not exist.
pub fn config_with_diagnostics(token: &str, directory: &str) -> String {
    let directory = directory.replace('\\', r"\\");
    let other = token.replace("r.token", "d.token");
    config_for(token).replace(
        r#""volumes":[]}]}"#,
        &format!(
            r#""volumes":[]}},{{"owner":"o","repository":"d","name":"d","slots":1,
            "tokenPath":"{other}","volumes":[],"diagnostics":true,
            "diagnosticDirectory":"{directory}"}}]}}"#
        ),
    )
}

impl Host {
    pub fn new() -> Self {
        let root = TempDir::new();
        let (json_token, token) = token_path(root.path());
        let config = root.write("host.json", config_for(&json_token).as_bytes());
        root.write_token(token.to_str().unwrap(), format!("{TOKEN}\n").as_bytes());
        for directory in ["bin", "docker", "state"] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        let docker = root
            .path()
            .join("bin")
            .join(format!("docker{}", std::env::consts::EXE_SUFFIX));
        fs::copy(FAKE_DOCKER, docker).unwrap();
        Self { root, config }
    }

    /// The token path as the configuration names it, escaped for JSON.
    pub fn json_token(&self) -> String {
        token_path(self.root.path()).0
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    /// The fake `docker`'s directory.
    pub fn docker(&self) -> PathBuf {
        self.path("docker")
    }

    /// The test root that holds the lock, the channel and the logs.
    pub fn state(&self) -> PathBuf {
        self.path("state")
    }

    /// Sets a script file the fake `docker` reads.
    pub fn script(&self, file: &str, content: &str) {
        fs::write(self.docker().join(file), content).unwrap();
    }

    /// The command for `runners <args>` on this host.
    pub fn command(&self, args: &[&str]) -> Command {
        let mut path = vec![self.path("bin")];
        path.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let mut command = Command::new(BINARY);
        command
            .args(args)
            .current_dir(self.root.path())
            .env("RUNNERS_TEST_ROOT", self.state())
            .env("FAKE_DOCKER_DIR", self.docker())
            .env("PATH", std::env::join_paths(path).unwrap())
            .stdin(Stdio::null());
        command
    }

    /// Runs a client, such as `status`, to its end.
    pub fn client(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Starts `runners start` for `config` against `stub`.
    pub fn start_with(&self, config: &Path, stub: &Stub) -> Started {
        let mut command = self.command(&["start", "--profile", "axpc"]);
        command
            .arg("--config")
            .arg(config)
            .env("RUNNERS_TEST_GITHUB_API", &stub.address)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Started::spawn(command)
    }

    pub fn start(&self, stub: &Stub) -> Started {
        self.start_with(&self.config, stub)
    }

    /// Asks `status` until its output holds `needle`, and returns that output.
    pub fn wait_for_status(&self, needle: &str, within: Duration) -> String {
        let started = Instant::now();
        let mut last = String::new();
        while started.elapsed() < within {
            let output = self.client(&["status"]);
            last = String::from_utf8_lossy(&output.stdout).into_owned();
            if last.contains(needle) {
                return last;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("status never showed {needle:?}; last:\n{last}");
    }

    /// The fake engine's containers, by name.
    pub fn containers(&self) -> Vec<String> {
        fs::read_to_string(self.docker().join("containers"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split('\t').next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Every call the fake `docker` recorded, as its arguments, in order.
    pub fn invocations(&self) -> Vec<Vec<String>> {
        let mut stems: Vec<PathBuf> = fs::read_dir(self.docker())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "argv")
            })
            .collect();
        stems.sort();
        stems
            .iter()
            .map(|path| {
                fs::read_to_string(path)
                    .unwrap()
                    .split('\0')
                    .map(str::to_string)
                    .collect()
            })
            .collect()
    }

    /// The process ID of the job container's `docker run` client, once it runs.
    pub fn job_client_pid(&self) -> u32 {
        let mut stems: Vec<PathBuf> = fs::read_dir(self.docker())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "argv")
            })
            .collect();
        stems.sort();
        for argv in stems {
            let text = fs::read_to_string(&argv).unwrap();
            let words: Vec<&str> = text.split('\0').collect();
            if words.first() == Some(&"run") && !words.contains(&"runners.cpu-probe=1") {
                return fs::read_to_string(argv.with_extension("pid"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
            }
        }
        panic!("no job container was run");
    }

    /// Every log file `start` wrote.
    pub fn log_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.state().join("logs"))
            .map(|entries| entries.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default();
        files.sort();
        files
    }
}

/// A stream read to its end on a thread of its own.
struct Collector {
    text: Arc<Mutex<String>>,
    ended: mpsc::Receiver<()>,
}

impl Collector {
    fn spawn(mut stream: impl Read + Send + 'static) -> Self {
        let text = Arc::new(Mutex::new(String::new()));
        let (sender, ended) = mpsc::channel();
        let shared = text.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => shared
                        .lock()
                        .unwrap()
                        .push_str(&String::from_utf8_lossy(&buffer[..read])),
                }
            }
            let _ = sender.send(());
        });
        Self { text, ended }
    }

    fn text(&self) -> String {
        self.text.lock().unwrap().clone()
    }
}

/// A running `start`.
pub struct Started {
    child: Child,
    stdout: Collector,
    stderr: Collector,
}

/// How a `start` ended.
pub struct Finished {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    /// How long `finish` waited for the process to end.
    pub waited: Duration,
}

impl Started {
    fn spawn(mut command: Command) -> Self {
        let mut child = command.spawn().unwrap();
        let stdout = Collector::spawn(child.stdout.take().unwrap());
        let stderr = Collector::spawn(child.stderr.take().unwrap());
        Self {
            child,
            stdout,
            stderr,
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn stderr(&self) -> String {
        self.stderr.text()
    }

    /// Kills the process outright.
    pub fn kill(&mut self) {
        self.child.kill().unwrap();
    }

    /// Waits until every writer of standard error has closed it: the process and
    /// every child that inherited it.
    pub fn wait_for_stderr_to_close(&self, within: Duration) -> bool {
        self.stderr.ended.recv_timeout(within).is_ok()
    }

    /// Waits up to `within` for the process to end, and kills it if it does not.
    pub fn finish(mut self, within: Duration) -> Finished {
        let started = Instant::now();
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() > within {
                let _ = self.child.kill();
                panic!(
                    "start did not end within {within:?}; its output:\n{}",
                    self.stderr.text()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let waited = started.elapsed();
        let _ = self.stdout.ended.recv_timeout(Duration::from_secs(10));
        let _ = self.stderr.ended.recv_timeout(Duration::from_secs(10));
        Finished {
            status,
            stdout: self.stdout.text(),
            stderr: self.stderr.text(),
            waited,
        }
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Sends `signal`, such as `INT`, to `pid` with the `kill` command, since a
/// signal sent from the test itself would need `unsafe`.
#[cfg(unix)]
pub fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .unwrap();
    assert!(status.success(), "kill -{signal} {pid} failed");
}

/// The text of `output`'s standard output and standard error.
pub fn texts(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}
