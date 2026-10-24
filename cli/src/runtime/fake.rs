//! In-memory stand-ins for the seams, for tests of the code above them.
//!
//! They record what they were asked and answer from a script. They are part of the
//! library so the integration tests and the later instance layer can use them. They
//! are test-only and never wired into a command.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::watch;

use crate::validate::PlannedVolume;

use super::backend::{
    Backend, BackendError, ContainerSpec, CpuRequest, PoolContainers, StopRequest,
};
use super::clock::Clock;
use super::log::Log;
use super::registration::{JitRequest, Registrar, RegistrationError};
use super::secret::Secret;

/// One call a [`FakeBackend`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    WaitUntilAvailable,
    ListPoolContainers(String),
    RemoveContainers(Vec<String>),
    PrepareVolumes { image: String, volumes: Vec<String> },
    SelectCpuset(CpuRequest),
    RunContainer(ContainerSpec),
    StopContainers(StopRequest),
}

/// A container the fake engine holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeContainer {
    pub name: String,
    pub diagnostic: bool,
}

/// What a [`FakeBackend`] answers and what it saw.
#[derive(Default)]
pub struct FakeState {
    pub calls: Vec<Call>,
    pub containers: Vec<FakeContainer>,
    pub list_error: Option<BackendError>,
    pub remove_error: Option<BackendError>,
    /// A volume name whose preparation fails.
    pub failing_volume: Option<String>,
    /// The answers to `select_cpuset`, then `0` for every call after them.
    pub cpusets: VecDeque<Result<String, BackendError>>,
    /// The exits of `run_container`; when they run out, the stop is asked for and
    /// the run exits 0.
    pub exits: VecDeque<Result<i32, BackendError>>,
    pub stop_when_exits_run_out: Option<watch::Sender<bool>>,
    /// Called at the start of every `run_container`, before its exit is chosen.
    pub on_run: Option<Box<dyn FnMut()>>,
    /// The JIT configurations handed to `run_container`, in order.
    pub jit_values: Vec<String>,
}

#[derive(Default)]
pub struct FakeBackend {
    pub state: RefCell<FakeState>,
}

impl FakeBackend {
    #[must_use]
    pub fn calls(&self) -> Vec<Call> {
        self.state.borrow().calls.clone()
    }
}

impl Backend for FakeBackend {
    async fn wait_until_available(&self) {
        self.state.borrow_mut().calls.push(Call::WaitUntilAvailable);
    }

    async fn list_pool_containers(&self, pool: &str) -> Result<PoolContainers, BackendError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::ListPoolContainers(pool.to_string()));
        if let Some(error) = state.list_error.clone() {
            return Err(error);
        }
        let prefix = format!("{pool}-");
        let found: Vec<&FakeContainer> = state
            .containers
            .iter()
            .filter(|container| container.name.starts_with(&prefix))
            .collect();
        Ok(PoolContainers {
            names: found
                .iter()
                .map(|container| container.name.clone())
                .collect(),
            diagnostic: found
                .iter()
                .filter(|container| container.diagnostic)
                .map(|container| container.name.clone())
                .collect(),
        })
    }

    async fn remove_containers(
        &self,
        names: &[String],
        _budget: Duration,
    ) -> Result<(), BackendError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::RemoveContainers(names.to_vec()));
        if let Some(error) = state.remove_error.clone() {
            return Err(error);
        }
        state
            .containers
            .retain(|container| !names.contains(&container.name));
        Ok(())
    }

    async fn prepare_volumes(
        &self,
        image: &str,
        volumes: &[PlannedVolume],
    ) -> Result<(), BackendError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::PrepareVolumes {
            image: image.to_string(),
            volumes: volumes.iter().map(|volume| volume.name.clone()).collect(),
        });
        let failing = state.failing_volume.clone();
        if volumes
            .iter()
            .any(|volume| Some(&volume.name) == failing.as_ref())
        {
            return Err(BackendError::new("Failed to create Docker volume"));
        }
        Ok(())
    }

    async fn select_cpuset(&self, request: &CpuRequest) -> Result<String, BackendError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::SelectCpuset(request.clone()));
        state
            .cpusets
            .pop_front()
            .unwrap_or_else(|| Ok("0".to_string()))
    }

    async fn run_container(&self, spec: &ContainerSpec, jit: &Secret) -> Result<i32, BackendError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::RunContainer(spec.clone()));
        state.jit_values.push(jit.expose().to_string());
        if let Some(hook) = state.on_run.as_mut() {
            hook();
        }
        match state.exits.pop_front() {
            Some(exit) => exit,
            None => {
                if let Some(stop) = &state.stop_when_exits_run_out {
                    let _ = stop.send(true);
                }
                Ok(0)
            }
        }
    }

    fn running_containers(&self) -> Vec<String> {
        Vec::new()
    }

    async fn stop_containers(&self, request: &StopRequest) -> Result<(), BackendError> {
        self.state
            .borrow_mut()
            .calls
            .push(Call::StopContainers(request.clone()));
        Ok(())
    }
}

/// What a [`FakeRegistrar`] was asked for.
#[derive(Clone, PartialEq, Eq)]
pub struct SeenRegistration {
    pub owner: String,
    pub repository: String,
    pub name: String,
    pub labels: Vec<String>,
    pub token: String,
}

impl std::fmt::Debug for SeenRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeenRegistration")
            .field("owner", &self.owner)
            .field("repository", &self.repository)
            .field("name", &self.name)
            .field("labels", &self.labels)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Default)]
pub struct FakeRegistrar {
    /// The answers, then `jit` for every call after them.
    pub results: RefCell<VecDeque<Result<String, RegistrationError>>>,
    pub seen: RefCell<Vec<SeenRegistration>>,
    /// Asked for the stop during the first request, before it answers.
    pub stop_during_request: RefCell<Option<watch::Sender<bool>>>,
}

impl Registrar for FakeRegistrar {
    async fn request_jit(&self, request: &JitRequest<'_>) -> Result<Secret, RegistrationError> {
        self.seen.borrow_mut().push(SeenRegistration {
            owner: request.owner.to_string(),
            repository: request.repository.to_string(),
            name: request.name.to_string(),
            labels: request.labels.to_vec(),
            token: request.token.expose().to_string(),
        });
        if let Some(stop) = self.stop_during_request.borrow().as_ref() {
            let _ = stop.send(true);
        }
        self.results
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok("jit".to_string()))
            .map(Secret::new)
    }
}

/// A clock that does not wait: a sleep is recorded, moves the time forward, and
/// completes at once.
pub struct FakeClock {
    now: Cell<SystemTime>,
    sleeps: RefCell<Vec<Duration>>,
    stop_after_sleeps: Cell<Option<usize>>,
    stop: RefCell<Option<watch::Sender<bool>>>,
}

impl FakeClock {
    /// A clock that starts at `unix_millis` after the epoch.
    #[must_use]
    pub fn at(unix_millis: u64) -> Self {
        Self {
            now: Cell::new(UNIX_EPOCH + Duration::from_millis(unix_millis)),
            sleeps: RefCell::new(Vec::new()),
            stop_after_sleeps: Cell::new(None),
            stop: RefCell::new(None),
        }
    }

    /// Asks for a stop once this many sleeps have been recorded.
    pub fn stop_after(&self, sleeps: usize, stop: watch::Sender<bool>) {
        self.stop_after_sleeps.set(Some(sleeps));
        *self.stop.borrow_mut() = Some(stop);
    }

    #[must_use]
    pub fn sleeps(&self) -> Vec<Duration> {
        self.sleeps.borrow().clone()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.now.get()
    }

    async fn sleep(&self, duration: Duration) {
        self.sleeps.borrow_mut().push(duration);
        self.now.set(self.now.get() + duration);
        let recorded = self.sleeps.borrow().len();
        if self
            .stop_after_sleeps
            .get()
            .is_some_and(|limit| recorded >= limit)
            && let Some(stop) = &*self.stop.borrow()
        {
            let _ = stop.send(true);
        }
    }
}

/// Collects the lines, each prefixed `INFO: ` or `WARN: `.
#[derive(Default)]
pub struct MemoryLog {
    pub lines: RefCell<Vec<String>>,
}

impl MemoryLog {
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.lines.borrow().clone()
    }
}

impl Log for MemoryLog {
    fn info(&self, line: &str) {
        self.lines.borrow_mut().push(format!("INFO: {line}"));
    }

    fn warn(&self, line: &str) {
        self.lines.borrow_mut().push(format!("WARN: {line}"));
    }
}
