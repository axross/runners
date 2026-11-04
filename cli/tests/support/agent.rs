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
use marshal_cli::token_store::{MemoryTokenStore, RepositoryKey};
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

pub struct Rig {
    pub root: TempDir,
    pub roots: Roots,
    pub backend: FakeBackend,
    pub clock: Rc<FakeClock>,
    pub registrar: FakeRegistrar,
    pub builder: FakeBuilder,
    pub store: Arc<MemoryTokenStore>,
    pub log: MemoryLog,
    pub limits: Cell<Limits>,
    pub times: Cell<StopTimes>,
    /// the file the agent watches, when a test asks for it.
    pub executable: RefCell<Option<PathBuf>>,
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
            clock: Rc::new(FakeClock::at(START)),
            registrar,
            builder: FakeBuilder::default(),
            store,
            log: MemoryLog::default(),
            limits: Cell::new(Limits::default()),
            times: Cell::new(StopTimes::default()),
            executable: RefCell::new(None),
            lock: RefCell::new(Some(lock)),
        }
    }

    /// stores `token` for the repository `owner/name`.
    pub fn store_token(&self, owner: &str, name: &str, token: &str) {
        use marshal_cli::runtime::secret::Secret;
        use marshal_cli::token_store::TokenStore;
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
            store: Some(self.store.clone()),
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

/// the threads of this process, where `/proc` says (Linux).
pub fn threads() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .and_then(|count| count.trim().parse().ok())
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
