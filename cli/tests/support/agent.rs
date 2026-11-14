//! a rig that runs the resident agent on fake seams, behind its real channel, and
//! talks to it with the library's client: the agent and the test script share one
//! current-thread runtime, so the fakes can be read and changed from the script.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use marshal_cli::agent::builder::ImageBuilder;
use marshal_cli::agent::machine::{Ended, Launch, Settings};
use marshal_cli::agent::server::Limits;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::wire::ClientInfo;
use marshal_cli::agent::{CallError, Client, Push, RawStream, Resident, connect, run_resident};
use marshal_cli::github_status::http::GitHubReader;
use marshal_cli::instance::lifecycle::StopTimes;
use marshal_cli::instance::lock::InstanceLock;
use marshal_cli::instance::roots::Roots;
use marshal_cli::lifecycle_settings::Current;
use marshal_cli::macos::fake::FakeSignals;
use marshal_cli::macos::monitor::Cadence;
use marshal_cli::runtime::clock::SystemClock;
use marshal_cli::runtime::fake::{Call, FakeBackend, FakeClock, FakeRegistrar, MemoryLog};
use marshal_cli::runtime::log::Log;
use marshal_cli::runtime::registration::HttpLimits;
use marshal_cli::token_store::{MemoryTokenStore, TokenKey, TokenStore};
use marshal_cli::windows_host::monitor::WindowsMonitor;
use marshal_cli::windows_host::run_key::MemorySignIn;
use serde_json::{Value, json};
use tokio::io::WriteHalf;
use tokio::sync::{Notify, mpsc, watch};

use super::TempDir;

pub const START: u64 = 1_709_618_828_009;

/// the token a test stores; obviously fake.
pub const TOKEN: &str = "fake-token-aaaa-1111";

/// another one, to replace the first.
pub const OTHER_TOKEN: &str = "fake-token-bbbb-2222";

/// the JIT configuration the fake GitHub hands out.
pub const JIT: &str = "fake-jit-config-cccc-3333";

/// one pool, `r`, of repository `o/r`; the token path names no file.
pub const ONE_POOL: &str = r#"{"imageName":"actions-runner:local","repositories":[
    {"owner":"o","repository":"r","name":"r","slots":1,"tokenPath":"C:\\no-such.token","volumes":[]}]}"#;

/// [`ONE_POOL`] as `config.toml` writes it.
pub const ONE_POOL_TOML: &str = r#"schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"

[[pool]]
name = "r"
repository = "o/r"
runners = 1
"#;

/// two pools of two repositories.
pub const TWO_POOLS: &str = r#"{"imageName":"actions-runner:local","repositories":[
    {"owner":"o","repository":"r","name":"r","slots":1,"tokenPath":"C:\\no-such.token","volumes":[]},
    {"owner":"o","repository":"d","name":"d","slots":1,"tokenPath":"C:\\no-such-either.token","volumes":[]}]}"#;

/// builds without Docker: it waits for a permit when told to hold, and ends with the
/// next scripted outcome, or ready when a stop interrupts a hold that is told to commit.
#[derive(Default)]
pub struct FakeBuilder {
    pub images: RefCell<Vec<String>>,
    pub results: RefCell<VecDeque<Result<(), String>>>,
    pub hold: Cell<bool>,
    pub release: Notify,
    /// set once the build saw a stop asked for.
    pub stopped: Cell<bool>,
    /// a stop that interrupts the hold ends the build ready, as a retag that commits
    /// within the grace of a stop.
    pub ok_after_stop: Cell<bool>,
}

impl ImageBuilder for FakeBuilder {
    async fn build(
        &self,
        image: &str,
        _log: &dyn Log,
        mut stop: watch::Receiver<Option<tokio::time::Instant>>,
    ) -> Result<(), String> {
        self.images.borrow_mut().push(image.to_string());
        if self.hold.get() {
            tokio::select! {
                () = self.release.notified() => {}
                _ = stop.wait_for(Option::is_some) => {
                    self.stopped.set(true);
                    if self.ok_after_stop.get() {
                        return Ok(());
                    }
                    return Err("the build was aborted".to_string());
                }
            }
        }
        self.results.borrow_mut().pop_front().unwrap_or(Ok(()))
    }
}

/// the fake clock with a real pause in each sleep, so a backoff loop that the fake
/// time would spin through lets the script and the channel run.
pub struct PacedClock {
    pub fake: FakeClock,
    /// the real pause in each sleep; a test that must see the state during a wait
    /// lengthens it.
    pub pause: Cell<Duration>,
}

impl std::ops::Deref for PacedClock {
    type Target = FakeClock;

    fn deref(&self) -> &FakeClock {
        &self.fake
    }
}

impl marshal_cli::runtime::clock::Clock for PacedClock {
    fn now(&self) -> std::time::SystemTime {
        self.fake.now()
    }

    async fn sleep(&self, duration: Duration) {
        self.fake.sleep(duration).await;
        tokio::time::sleep(self.pause.get()).await;
    }
}

pub struct Rig {
    pub root: TempDir,
    pub roots: Roots,
    pub backend: FakeBackend,
    pub clock: Rc<PacedClock>,
    pub registrar: FakeRegistrar,
    /// where GitHub status is read from: a loopback stub, or nothing listening.
    pub github_api: RefCell<String>,
    pub builder: FakeBuilder,
    pub store: Arc<MemoryTokenStore>,
    pub log: MemoryLog,
    pub limits: Cell<Limits>,
    pub times: Cell<StopTimes>,
    /// how long a drain for an update waits for busy runners.
    pub drain_bound: Cell<Duration>,
    /// the file the agent watches, when a test asks for it.
    pub executable: RefCell<Option<PathBuf>>,
    /// the agent has no token store, as on a host without one.
    pub no_store: Cell<bool>,
    /// the profile the agent checks its configuration under; axpc unless a test says.
    pub profile: Cell<marshal_cli::profile::Profile>,
    /// a store the agent uses in place of the in-memory one, as the Credential Manager.
    pub other_store: RefCell<Option<Arc<dyn TokenStore>>>,
    /// the signals the agent's Windows monitor reads; a test makes it sleep or move
    /// the network.
    pub signals: Rc<FakeSignals>,
    /// the settings in effect, which the agent sets and its monitor reads.
    pub in_effect: Current,
    /// the sign-in item, in memory.
    pub sign_in: MemorySignIn,
    lock: RefCell<Option<InstanceLock>>,
    ended_at: Cell<Option<Instant>>,
}

impl Rig {
    /// a rig whose configuration is `config`, with the token of `o/r` stored.
    pub fn new(config: &str) -> Self {
        let root = TempDir::new();
        std::fs::write(root.path().join("config.json"), config).unwrap();
        let roots = Roots::for_tests(root.path());
        let lock = InstanceLock::acquire(&roots).unwrap();
        let store = Arc::new(MemoryTokenStore::default());
        let registrar = FakeRegistrar::default();
        registrar
            .results
            .borrow_mut()
            .extend(std::iter::repeat_n(Ok(JIT.to_string()), 64));
        Self {
            root,
            roots,
            backend: FakeBackend::default(),
            clock: Rc::new(PacedClock {
                fake: FakeClock::at(START),
                pause: Cell::new(Duration::from_millis(60)),
            }),
            registrar,
            github_api: RefCell::new("http://127.0.0.1:9".to_string()),
            builder: FakeBuilder::default(),
            store,
            log: MemoryLog::default(),
            limits: Cell::new(Limits::default()),
            times: Cell::new(StopTimes::default()),
            drain_bound: Cell::new(marshal_cli::agent::machine::DRAIN_BOUND),
            executable: RefCell::new(None),
            no_store: Cell::new(false),
            profile: Cell::new(marshal_cli::profile::Profile::Axpc),
            other_store: RefCell::new(None),
            signals: Rc::new(FakeSignals::new()),
            in_effect: Current::default(),
            sign_in: MemorySignIn::default(),
            lock: RefCell::new(Some(lock)),
            ended_at: Cell::new(None),
        }
    }

    /// stores `token` for the repository `owner/name`.
    pub fn store_token(&self, owner: &str, name: &str, token: &str) {
        use marshal_cli::runtime::secret::Secret;
        self.store
            .set(&TokenKey::of(owner, name), &Secret::new(token.to_string()))
            .unwrap();
    }

    /// writes `config.json`, which the agent migrates when it starts and finds no
    /// `config.toml`.
    pub fn write_json(&self, config: &str) {
        std::fs::write(self.root.path().join("config.json"), config).unwrap();
    }

    /// writes `config.toml`, the file the agent reads.
    pub fn write_toml(&self, config: &str) {
        std::fs::write(self.root.path().join("config.toml"), config).unwrap();
    }

    /// removes a configuration file of the rig's folder.
    pub fn remove(&self, name: &str) {
        std::fs::remove_file(self.root.path().join(name)).unwrap();
    }

    /// how long after `since` the agent had ended and released the lock; the instant is
    /// taken inside `drive`, where the script's own waits do not count.
    pub fn ended_within(&self, since: Instant) -> Duration {
        self.ended_at
            .get()
            .expect("the rig was not driven")
            .saturating_duration_since(since)
    }

    /// whether the lock is free now that the agent has ended.
    pub fn lock_is_free(&self) -> bool {
        InstanceLock::acquire(&self.roots).is_ok()
    }

    /// lets the fake engine report the job containers it was asked to run as running,
    /// so a stop has something to stop.
    pub fn mark_running(&self) {
        let names: Vec<String> = self
            .backend
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::RunContainer(spec) => Some(spec.name),
                _ => None,
            })
            .collect();
        self.backend.state.borrow_mut().running = names;
    }

    /// holds every job container until a stop, as a job that runs.
    pub fn hold_jobs(&self) {
        self.backend.state.borrow_mut().hold_runs = true;
    }

    /// runs the agent, and `script` with a connection to it, until both end or a
    /// minute has passed; the script ends the agent, as a client's `quit` does.
    pub async fn drive<F, Fut, T>(&self, launch: Launch, script: F) -> (Ended, T)
    where
        F: FnOnce(Conn) -> Fut,
        Fut: Future<Output = T>,
    {
        let lock = self
            .lock
            .borrow_mut()
            .take()
            .expect("the rig was driven before");
        let reader = GitHubReader::new(
            Some(self.github_api.borrow().as_str()),
            HttpLimits::default(),
        )
        .unwrap();
        let agent = async {
            // the monitor ticks on the real clock, so its samples move no fake time
            let monitor = WindowsMonitor::new(
                Rc::clone(&self.signals),
                &self.in_effect,
                &SystemClock,
                &self.log,
                Cadence {
                    sample: Duration::from_millis(10),
                    samples_per_check: 5,
                },
            );
            let resident = Resident {
                backend: &self.backend,
                clock: &self.clock,
                registrar: &self.registrar,
                reader: &reader,
                builder: &self.builder,
                monitor: &monitor,
                in_effect: &self.in_effect,
                sign_in: Some(&self.sign_in),
                store: (!self.no_store.get()).then(|| {
                    self.other_store
                        .borrow()
                        .clone()
                        .unwrap_or_else(|| self.store.clone() as Arc<dyn TokenStore>)
                }),
                log: &self.log,
                roots: &self.roots,
                lock: &lock,
                settings: Settings {
                    version: "test".to_string(),
                    config_folder: self.root.path().to_path_buf(),
                    profile: self.profile.get(),
                    times: self.times.get(),
                    launch,
                    drain_bound: self.drain_bound.get(),
                },
                executable: self.executable.borrow().clone(),
                limits: self.limits.get(),
            };
            let ended = run_resident(resident).await;
            drop(lock);
            self.ended_at.set(Some(Instant::now()));
            ended
        };
        let both = async {
            let script = async { script(Conn::open(&self.roots).await).await };
            let (ended, value) = tokio::join!(agent, script);
            (ended.unwrap(), value)
        };
        tokio::time::timeout(Duration::from_secs(60), both)
            .await
            .expect("the agent and the script did not both finish in a minute")
    }
}

/// a client connection, with what the agent pushed on it.
pub struct Conn {
    pub client: Client,
    pub pushes: mpsc::UnboundedReceiver<Push>,
    pub roots: Roots,
}

impl Conn {
    pub async fn open(roots: &Roots) -> Self {
        // the agent opens its channel as the first thing it does, but on a runtime
        // shared with this script, so it is retried for a moment
        let started = Instant::now();
        loop {
            match connect(roots, &ClientInfo::default()).await {
                Ok((client, pushes)) => {
                    return Self {
                        client,
                        pushes,
                        roots: roots.clone(),
                    };
                }
                Err(error) if started.elapsed() > Duration::from_secs(20) => {
                    panic!("the agent could not be reached: {error}")
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    }

    /// another connection to the same agent.
    pub async fn another(&self) -> Self {
        Self::open(&self.roots).await
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, CallError> {
        self.client.call(method, params).await
    }

    pub async fn ok(&self, method: &str) -> Value {
        self.call(method, Value::Null)
            .await
            .unwrap_or_else(|error| panic!("{method} failed: {error}"))
    }

    pub async fn status(&self) -> Snapshot {
        self.client.status().await.unwrap()
    }

    /// the stable code of an error the agent answered with.
    pub async fn code(&self, method: &str, params: Value) -> String {
        match self.call(method, params).await {
            Err(CallError::Agent(body)) => body.code,
            other => panic!("{method} was not refused: {other:?}"),
        }
    }

    /// polls `status` until `ready` accepts the snapshot, for at most 20 s.
    pub async fn until(&self, what: &str, ready: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let started = Instant::now();
        loop {
            let snapshot = self.status().await;
            if ready(&snapshot) {
                return snapshot;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "never saw {what}; the last snapshot: {snapshot:#?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn until_state(&self, state: &str) -> Snapshot {
        self.until(state, |snapshot| snapshot.agent.state == state)
            .await
    }

    /// waits for the first runner of the first pool to be running its job.
    pub async fn until_runner_runs(&self) -> Snapshot {
        self.until("a runner running", |snapshot| snapshot.runners_up > 0)
            .await
    }

    pub async fn set_token(&self, repository: &str, token: &str) -> Result<(), CallError> {
        self.client.token_set(repository, token).await
    }

    /// the next push, within 20 s.
    pub async fn push(&mut self) -> Push {
        tokio::time::timeout(Duration::from_secs(20), self.pushes.recv())
            .await
            .expect("nothing was pushed in 20 s")
            .expect("the push channel ended")
    }
}

/// a connection the test speaks on by hand, line by line, to say what the library's
/// client never would.
pub struct Raw {
    reader: tokio::io::BufReader<tokio::io::ReadHalf<RawStream>>,
    writer: WriteHalf<RawStream>,
    /// everything the agent wrote on this connection, as text.
    pub heard: String,
}

impl Raw {
    pub async fn open(roots: &Roots) -> Self {
        let started = Instant::now();
        loop {
            match marshal_cli::agent::connect_raw(roots).await {
                Ok(stream) => {
                    let (read, writer) = tokio::io::split(stream);
                    return Self {
                        reader: tokio::io::BufReader::new(read),
                        writer,
                        heard: String::new(),
                    };
                }
                Err(error) if started.elapsed() > Duration::from_secs(20) => {
                    panic!("the agent could not be reached: {error}")
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    }

    /// opens a connection and completes the handshake.
    pub async fn hello(roots: &Roots) -> Self {
        let mut raw = Self::open(roots).await;
        raw.send(&json!({"type": "hello", "versions": [2], "client": {"name": "test"}}))
            .await;
        let welcome = raw.next().await.unwrap();
        assert_eq!(welcome["type"], "welcome", "{welcome}");
        raw
    }

    pub async fn send_text(&mut self, text: &str) {
        use tokio::io::AsyncWriteExt;
        self.writer.write_all(text.as_bytes()).await.unwrap();
        self.writer.write_all(b"\n").await.unwrap();
        self.writer.flush().await.unwrap();
    }

    /// writes `bytes` as they are, with no line break.
    pub async fn send_bytes(&mut self, bytes: &[u8]) {
        use tokio::io::AsyncWriteExt;
        self.writer.write_all(bytes).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    /// writes `bytes` as they are, for a peer that may close before it has read them
    /// all: a write the peer cut off, which is `BrokenPipe` or `ConnectionReset` (and
    /// on Windows the pipe's own closed codes), is not an error. Returns whether all
    /// of it was written. Any other error fails the test; a write that must succeed
    /// uses `send_bytes`.
    pub async fn send_bytes_until_cut_off(&mut self, bytes: &[u8]) -> bool {
        use tokio::io::AsyncWriteExt;
        let written = match self.writer.write_all(bytes).await {
            Ok(()) => self.writer.flush().await,
            Err(error) => Err(error),
        };
        match written {
            Ok(()) => true,
            Err(error) if is_cut_off(&error) => false,
            Err(error) => panic!("the write failed other than by being cut off: {error:?}"),
        }
    }

    /// `send` for a peer that may have closed already; see `send_bytes_until_cut_off`.
    pub async fn send_until_cut_off(&mut self, message: &Value) -> bool {
        let mut line = message.to_string().into_bytes();
        line.push(b'\n');
        self.send_bytes_until_cut_off(&line).await
    }

    pub async fn send(&mut self, message: &Value) {
        self.send_text(&message.to_string()).await;
    }

    pub async fn request(&mut self, id: u64, method: &str, params: Value) {
        self.send(&json!({"type": "request", "id": id, "method": method, "params": params}))
            .await;
    }

    /// the next message within `wait`; `None` when the connection ended first.
    pub async fn next_within(&mut self, wait: Duration) -> Option<Value> {
        use tokio::io::AsyncBufReadExt;
        let mut line = String::new();
        match tokio::time::timeout(wait, self.reader.read_line(&mut line)).await {
            Ok(Ok(0)) | Ok(Err(_)) => None,
            Ok(Ok(_)) => {
                self.heard.push_str(&line);
                Some(serde_json::from_str(line.trim_end()).unwrap())
            }
            Err(_) => panic!(
                "nothing was heard within {wait:?}; heard so far: {}",
                self.heard
            ),
        }
    }

    pub async fn next(&mut self) -> Option<Value> {
        self.next_within(Duration::from_secs(20)).await
    }

    /// the next message that is not an event or a heartbeat.
    pub async fn answer(&mut self) -> Value {
        loop {
            let message = self.next().await.expect("the connection ended");
            if message["type"] != "event" && message["type"] != "heartbeat" {
                return message;
            }
        }
    }

    /// whether the agent has closed this connection (within `wait`), reading what
    /// remains.
    pub async fn closed_within(&mut self, wait: Duration) -> bool {
        use tokio::io::AsyncBufReadExt;
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let mut line = String::new();
            match tokio::time::timeout(left, self.reader.read_line(&mut line)).await {
                Ok(Ok(0)) | Ok(Err(_)) => return true,
                Ok(Ok(_)) => self.heard.push_str(&line),
                Err(_) => return false,
            }
        }
    }
}

/// whether a write failed because the peer closed the connection: `BrokenPipe` or
/// `ConnectionReset`, which a socket and, as Rust maps them, a pipe report; on
/// Windows also the codes of a pipe whose server has closed (109, 232 and 233), in
/// case a version of Rust leaves one unmapped.
fn is_cut_off(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    if matches!(
        error.kind(),
        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset
    ) {
        return true;
    }
    cfg!(windows) && matches!(error.raw_os_error(), Some(109 | 232 | 233))
}

/// the runtime's worker and blocking-pool threads of this process, where `/proc`
/// says (Linux); the test harness's own threads are not counted, and a current-thread
/// runtime has none of these, so a thread an agent spawned per connection would show.
pub fn runtime_threads() -> Option<usize> {
    let tasks = std::fs::read_dir("/proc/self/task").ok()?;
    let mut count = 0;
    for task in tasks {
        let name = std::fs::read_to_string(task.ok()?.path().join("comm")).ok()?;
        // the kernel keeps 15 characters of a thread's name
        if name.starts_with("tokio-runtime-w") {
            count += 1;
        }
    }
    Some(count)
}

/// a configuration of `pools` pools of `slots` slots each, one repository apiece,
/// with names long enough to make a big snapshot.
pub fn many_pools(pools: usize, slots: u32) -> String {
    let entries: Vec<String> = (0..pools)
        .map(|n| {
            format!(
                r#"{{"owner":"o","repository":"repository-{n:04}","name":"pool-with-a-long-name-{n:04}-{}","slots":{slots},"tokenPath":"C:\\no-such-{n}.token","volumes":[]}}"#,
                "x".repeat(24)
            )
        })
        .collect();
    format!(
        r#"{{"imageName":"actions-runner:local","repositories":[{}]}}"#,
        entries.join(",")
    )
}

impl Rig {
    /// stores a token for each of the repositories `many_pools` names.
    pub fn store_many(&self, pools: usize) {
        for n in 0..pools {
            self.store_token("o", &format!("repository-{n:04}"), TOKEN);
        }
    }
}

impl Rig {
    /// ends every job container the fake engine holds, as jobs that finish, without
    /// stopping the agent; each runner then registers again.
    pub async fn end_jobs(&self) {
        use marshal_cli::runtime::backend::{Backend, StopRequest};
        self.backend
            .stop_containers(&StopRequest {
                names: Vec::new(),
                grace: Duration::ZERO,
                budget: Duration::from_secs(1),
            })
            .await
            .unwrap();
    }

    /// the tokens the registrations so far were made with, in order.
    pub fn registered_with(&self) -> Vec<String> {
        self.registrar
            .seen
            .borrow()
            .iter()
            .map(|seen| seen.token.clone())
            .collect()
    }
}

impl Conn {
    /// polls until `ready` holds, for at most 20 s.
    pub async fn eventually(&self, what: &str, ready: impl Fn() -> bool) {
        let started = Instant::now();
        while !ready() {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "never saw {what}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

pub const HELPER: &str = env!("CARGO_BIN_EXE_instance-helper");

/// the resident agent as a process of the helper, on the fake `docker` and a GitHub stub.
pub struct AgentProcess {
    child: std::process::Child,
    pub roots: Roots,
}

impl AgentProcess {
    /// takes on an agent `command` started, whose state is under `state`.
    pub fn from_command(mut command: std::process::Command, state: &std::path::Path) -> Self {
        let child = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            child,
            roots: Roots::for_tests(state),
        }
    }

    /// starts the agent under `state`, with `extra` arguments after the usual ones.
    pub fn spawn(host: &super::host::Host, github: &str, extra: &[&str]) -> Self {
        let state = host.state();
        let child = std::process::Command::new(HELPER)
            .args(["agent", "--root"])
            .arg(&state)
            .args([
                "--github-api",
                github,
                "--docker",
                super::docker::FAKE_DOCKER,
            ])
            .arg("--docker-dir")
            .arg(host.docker())
            .args(extra)
            .current_dir(host.root.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            child,
            roots: Roots::for_tests(&state),
        }
    }

    /// waits up to `within` for the process to end, and returns its exit code and what
    /// it wrote to standard error.
    pub async fn finish(mut self, within: Duration) -> (Option<i32>, String) {
        let started = Instant::now();
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                started.elapsed() < within,
                "the agent did not end in {within:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let mut text = String::new();
        if let Some(mut stderr) = self.child.stderr.take() {
            std::io::Read::read_to_string(&mut stderr, &mut text).unwrap();
        }
        (status.code(), text)
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
