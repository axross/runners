//! a scratch host, a recording screen and log, and a session on the real library.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use marshal_cli::instance::roots::Roots;
use marshal_session::{
    Ended, Handle, Launcher, Library, Log, Present, Seams, Session, Shown, WallClock,
};
use tokio::task::JoinHandle;

pub const APP: &str = env!("CARGO_BIN_EXE_marshal-app");
pub const FAKE_DOCKER: &str = env!("CARGO_BIN_EXE_fake-docker");

/// a configuration with one pool whose token is not in any file, so it waits for one.
pub const ONE_POOL: &str = r#"{"imageName":"actions-runner:local","repositories":[
    {"owner":"o","repository":"r","name":"r","slots":1,"tokenPath":"C:\\no-such.token","volumes":[]}]}"#;

/// a directory under the system's temporary directory, removed when dropped.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "marshal-app-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        let scratch = Self { path };
        fs::create_dir_all(scratch.docker()).unwrap();
        scratch
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// the roots the agent and the app share: the agent reads them from
    /// `MARSHAL_TEST_ROOT`, and the app's library is given them here.
    pub fn roots(&self) -> Roots {
        Roots::for_tests(&self.path)
    }

    /// the fake docker's directory.
    pub fn docker(&self) -> PathBuf {
        self.path.join("docker")
    }

    pub fn write_config(&self, json: &str) {
        fs::write(self.path.join("config.json"), json).unwrap();
    }

    /// the variables that make an agent started from this executable use this host and
    /// the fake docker.
    pub fn agent_environment(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        vec![
            (
                "MARSHAL_TEST_ROOT".into(),
                self.path.clone().into_os_string(),
            ),
            ("MARSHAL_TEST_DOCKER".into(), FAKE_DOCKER.into()),
            ("FAKE_DOCKER_DIR".into(), self.docker().into_os_string()),
        ]
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// every view the session showed, in order.
#[derive(Default)]
pub struct Screens {
    shown: Mutex<Vec<Shown>>,
}

impl Present for Screens {
    fn present(&self, shown: &Shown) {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(shown.clone());
    }
}

impl Screens {
    pub fn latest(&self) -> Option<Shown> {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last()
            .cloned()
    }

    pub fn all(&self) -> Vec<Shown> {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// waits for the latest view to satisfy `condition`, and returns it.
    pub async fn until(
        &self,
        what: &str,
        within: Duration,
        condition: impl Fn(&Shown) -> bool,
    ) -> Shown {
        let started = Instant::now();
        loop {
            if let Some(shown) = self.latest().filter(&condition) {
                return shown;
            }
            assert!(
                started.elapsed() < within,
                "never: {what}; the window says {:?}",
                self.latest().map(|shown| heading(&shown))
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// the whole window's heading, when the window is not the list.
pub fn heading(shown: &Shown) -> Option<String> {
    shown.view.full().map(|full| full.heading.clone())
}

/// the lines the app logged.
#[derive(Default)]
pub struct Lines {
    lines: Mutex<Vec<String>>,
}

impl Lines {
    pub fn text(&self) -> String {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .join("\n")
    }
}

impl Log for Lines {
    fn info(&self, line: &str) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_string());
    }

    fn warn(&self, line: &str) {
        self.info(line);
    }
}

pub struct Running {
    pub handle: Handle,
    pub screens: Arc<Screens>,
    pub log: Arc<Lines>,
    pub ended: JoinHandle<Ended>,
}

/// a session on the real library, against `roots`, on the current runtime.
pub fn run_session(roots: Roots, launcher: Arc<dyn Launcher>) -> Running {
    let screens = Arc::new(Screens::default());
    let log = Arc::new(Lines::default());
    let (session, handle) = Session::new(Seams {
        dial: Library::new(roots, "test"),
        launcher,
        present: screens.clone(),
        time: Arc::new(WallClock::new(|| 0)),
        log: log.clone(),
        version: "test".to_string(),
        agent_version: "test".to_string(),
        updater: None,
        processes: Arc::new(marshal_session::NoUpdates),
        record: Arc::new(marshal_session::NoUpdates),
        launch: marshal_cli::agent::Launch::Running,
        fresh: false,
        login_item: None,
    });
    Running {
        handle,
        screens,
        log,
        ended: tokio::spawn(session.run()),
    }
}

/// a launcher that starts nothing, for a test of an agent that is already there.
pub struct NoLauncher;

impl Launcher for NoLauncher {
    fn start_agent(&self, _launch: marshal_cli::agent::Launch) -> Result<(), String> {
        Err("this test starts no agent".to_string())
    }
}

/// waits until an agent answers at the scratch host's channel.
pub async fn wait_for_agent(scratch: &Scratch) {
    let started = Instant::now();
    loop {
        let answered = marshal_cli::agent::connect(
            &scratch.roots(),
            &marshal_cli::agent::wire::ClientInfo::default(),
        )
        .await;
        if answered.is_ok() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "no agent answered"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// waits until nothing answers at the scratch host's channel.
pub async fn wait_for_no_agent(scratch: &Scratch) {
    let started = Instant::now();
    loop {
        let answered = marshal_cli::agent::connect(
            &scratch.roots(),
            &marshal_cli::agent::wire::ClientInfo::default(),
        )
        .await;
        if matches!(answered, Err(marshal_cli::agent::ConnectError::NoAgent)) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "an agent still answers"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
