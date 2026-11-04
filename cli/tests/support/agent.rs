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
use marshal_cli::instance::lifecycle::StopTimes;
use marshal_cli::instance::lock::InstanceLock;
use marshal_cli::instance::roots::Roots;
use marshal_cli::runtime::fake::{Call, FakeBackend, FakeClock, FakeRegistrar, MemoryLog};
use marshal_cli::runtime::log::Log;
use marshal_cli::token_store::{MemoryTokenStore, RepositoryKey, TokenStore};
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

/// two pools of two repositories.
pub const TWO_POOLS: &str = r#"{"imageName":"actions-runner:local","repositories":[
    {"owner":"o","repository":"r","name":"r","slots":1,"tokenPath":"C:\\no-such.token","volumes":[]},
    {"owner":"o","repository":"d","name":"d","slots":1,"tokenPath":"C:\\no-such-either.token","volumes":[]}]}"#;

/// builds without Docker: it waits for a permit when told to hold, and ends with the
/// next scripted outcome.
#[derive(Default)]
pub struct FakeBuilder {
    pub images: RefCell<Vec<String>>,
    pub results: RefCell<VecDeque<Result<(), String>>>,
    pub hold: Cell<bool>,
    pub release: Notify,
    /// set once the build saw a stop asked for.
    pub stopped: Cell<bool>,
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
                    return Err("the build was aborted".to_string());
                }
            }
        }
        self.results.borrow_mut().pop_front().unwrap_or(Ok(()))
    }
}

/// the fake clock with a real pause in each sleep, so a backoff loop that the fake
/// time would spin through lets the script and the channel run.
pub struct PacedClock(pub FakeClock);

impl std::ops::Deref for PacedClock {
    type Target = FakeClock;

    fn deref(&self) -> &FakeClock {
        &self.0
    }
}

impl marshal_cli::runtime::clock::Clock for PacedClock {
    fn now(&self) -> std::time::SystemTime {
        self.0.now()
    }

    async fn sleep(&self, duration: Duration) {
        self.0.sleep(duration).await;
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
}

pub struct Rig {
    pub root: TempDir,
    pub roots: Roots,
    pub backend: FakeBackend,
    pub clock: Rc<PacedClock>,
    pub registrar: FakeRegistrar,
    pub builder: FakeBuilder,
    pub store: Arc<MemoryTokenStore>,
    pub log: MemoryLog,
    pub limits: Cell<Limits>,
    pub times: Cell<StopTimes>,
    /// the file the agent watches, when a test asks for it.
    pub executable: RefCell<Option<PathBuf>>,
    /// the agent has no token store, as on a host without one.
    pub no_store: Cell<bool>,
    /// a store the agent uses in place of the in-memory one, as the Credential Manager.
    pub other_store: RefCell<Option<Arc<dyn TokenStore>>>,
    lock: RefCell<Option<InstanceLock>>,
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
            clock: Rc::new(PacedClock(FakeClock::at(START))),
            registrar,
            builder: FakeBuilder::default(),
            store,
            log: MemoryLog::default(),
            limits: Cell::new(Limits::default()),
            times: Cell::new(StopTimes::default()),
            executable: RefCell::new(None),
            no_store: Cell::new(false),
            other_store: RefCell::new(None),
            lock: RefCell::new(Some(lock)),
        }
    }

    /// stores `token` for the repository `owner/name`.
    pub fn store_token(&self, owner: &str, name: &str, token: &str) {
        use marshal_cli::runtime::secret::Secret;
        self.store
            .set(
                &RepositoryKey::of(owner, name),
                &Secret::new(token.to_string()),
            )
            .unwrap();
    }

    pub fn write_config(&self, config: &str) {
        std::fs::write(self.root.path().join("config.json"), config).unwrap();
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
        let resident = Resident {
            backend: &self.backend,
            clock: &self.clock,
            registrar: &self.registrar,
            builder: &self.builder,
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
                config_path: self.root.path().join("config.json"),
                times: self.times.get(),
                launch,
            },
            executable: self.executable.borrow().clone(),
            limits: self.limits.get(),
        };
        let both = async {
            let script = async { script(Conn::open(&self.roots).await).await };
            let (ended, value) = tokio::join!(run_resident(resident), script);
            (ended.unwrap(), value)
        };
        let outcome = tokio::time::timeout(Duration::from_secs(60), both)
            .await
            .expect("the agent and the script did not both finish in a minute");
        drop(lock);
        outcome
    }
}

/// a client connection, with what the agent pushed on it.
pub struct Conn {
    pub client: Client<WriteHalf<RawStream>>,
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

pub fn quit_params() -> Value {
    json!(null)
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
