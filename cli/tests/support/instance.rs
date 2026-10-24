//! A rig that runs an [`Instance`] on fake seams and talks to it like the control
//! channel does.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use runners_cli::instance::fake::{FakeCore, FakeSource};
use runners_cli::instance::lifecycle::{Ended, Instance, Settings};
use runners_cli::instance::protocol::{Incoming, Request, Response};
use runners_cli::profile::Profile;
use runners_cli::runtime::fake::{FakeBackend, FakeClock, MemoryLog};
use runners_cli::validate::{Plan, validate};
use tokio::sync::{mpsc, oneshot};

pub const CONFIG_PATH: &str = "/etc/runners/host.json";

pub const START: u64 = 1_709_618_828_009;

/// A valid axpc configuration with one pool of one slot.
pub const CONFIG: &str = r#"{"imageName":"actions-runner:local","repositories":[{"owner":"o","repository":"r",
        "name":"r","slots":1,"tokenPath":"C:\\r.token","volumes":[]}]}"#;

fn plan() -> Plan {
    validate(CONFIG.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap()
}

pub struct Rig {
    pub backend: FakeBackend,
    pub clock: Rc<FakeClock>,
    pub core: FakeCore,
    pub source: FakeSource,
    pub log: MemoryLog,
    plan: Plan,
}

impl Rig {
    pub fn new() -> Self {
        let clock = Rc::new(FakeClock::at(START));
        let core = FakeCore::default();
        *core.clock.borrow_mut() = Some(clock.clone());
        let plan = plan();
        Self {
            backend: FakeBackend::default(),
            clock,
            core,
            source: FakeSource::new(plan.clone()),
            log: MemoryLog::default(),
            plan,
        }
    }

    pub fn settings() -> Settings {
        Settings {
            config_path: CONFIG_PATH.to_string(),
            profile: Profile::Axpc,
        }
    }

    /// Runs the instance and the script together. The script ends the instance, as
    /// a client's `stop` does, and the rig fails a run that takes over ten seconds.
    pub async fn drive<F, Fut, T>(&self, script: F) -> (Ended, T)
    where
        F: FnOnce(Control) -> Fut,
        Fut: Future<Output = T>,
    {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let instance = Instance::new(
            Self::settings(),
            &self.backend,
            &self.clock,
            &self.core,
            &self.source,
            &self.log,
        );
        let run = instance.run(self.plan.clone(), &mut receiver);
        let both = async { tokio::join!(run, script(Control { sender })) };
        tokio::time::timeout(Duration::from_secs(10), both)
            .await
            .expect("the instance and the script did not both finish")
    }
}

pub struct Control {
    sender: mpsc::UnboundedSender<Incoming>,
}

impl Control {
    /// Sends a request and returns where its answer will arrive.
    pub fn send(&self, request: Request) -> oneshot::Receiver<Response> {
        let (reply, answer) = oneshot::channel();
        self.sender.send(Incoming { request, reply }).unwrap();
        answer
    }

    pub async fn ask(&self, request: Request) -> Response {
        self.send(request).await.unwrap()
    }

    pub async fn state(&self) -> String {
        self.ask(Request::Status)
            .await
            .field("state")
            .unwrap()
            .to_string()
    }

    /// Waits until `status` reports `state`, giving the instance's tasks turns.
    pub async fn wait_for_state(&self, state: &str) {
        for _ in 0..10_000 {
            if self.state().await == state {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the instance never reached the {state} state");
    }

    pub async fn stop(&self) -> Response {
        self.ask(Request::Stop).await
    }
}

pub fn resume(profile: Profile, path: &str) -> Request {
    Request::resume(profile, std::path::Path::new(path)).unwrap()
}
