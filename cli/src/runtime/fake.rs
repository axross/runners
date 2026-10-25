//! In-memory stand-ins for the seams, for tests of the code above them.
//!
//! They record what they were asked and answer from a script. They are part of the
//! library so the integration tests and the later instance layer can use them. They
//! are test-only and never wired into a command.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{Notify, watch};

use crate::image::check::{ImageCheck, ImageLabels};
use crate::image::contract::CONTRACT_VERSION;
use crate::validate::PlannedVolume;

use super::backend::{
    Backend, BackendError, ContainerSpec, CpuRequest, Deadlines, DiagnosticRecord, ExportOutcome,
    Finalized, PoolContainers, StopRequest,
};
use super::clock::Clock;
use super::log::{Log, Masks};
use super::registration::{JitRequest, Registrar, RegistrationError};
use super::retained::{Claim, Retained};
use super::secret::Secret;

/// One call a [`FakeBackend`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    WaitUntilAvailable,
    ListPoolContainers(String),
    RemoveContainers(Vec<String>),
    PrepareVolumes {
        image: String,
        volumes: Vec<String>,
    },
    SelectCpuset(CpuRequest),
    RunContainer(ContainerSpec),
    StopContainers(StopRequest),
    KillContainers(Vec<String>),
    /// A finalisation began, with the deadlines it was given.
    FinalizeDiagnostic(String, Deadlines),
    CountBundles(String),
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
    /// What `running_containers` reports; stopping or removing a name drops it.
    pub running: Vec<String>,
    /// A stop request fails with this error, once it is recorded.
    pub stop_error: Option<BackendError>,
    /// A stop request, once recorded, never ends, as a `docker stop` whose
    /// containers ignore the grace period would not before its budget.
    pub stop_hangs: bool,
    /// The images each `check_image` was asked about, in order. They are kept out
    /// of `calls`, so a test of the other calls sees only those.
    pub image_checks: Vec<String>,
    /// The answers to `check_image`; when they run out, every check matches, with
    /// [`matching_labels`] as the image's labels.
    pub image_answers: VecDeque<ImageCheck>,
    /// The outcomes of the exports `finalize_diagnostic` runs; when they run out,
    /// every export is complete. A complete export or one with a gap admits a
    /// bundle.
    pub export_results: VecDeque<ExportOutcome>,
    /// The bundles written, as container and record, in order.
    pub bundles: Vec<(String, DiagnosticRecord)>,
    /// When set, an export waits for a permit of this before it ends, which holds
    /// its container's finalisation.
    pub export_gate: Option<Rc<Notify>>,
    /// Called at the start of every export.
    pub on_export: Option<Box<dyn FnMut()>>,
    /// The answers to `count_bundles`; when they run out, the bundles written for
    /// the pool are counted.
    pub bundle_counts: VecDeque<Option<u32>>,
    /// A kill fails with this error, once it is recorded.
    pub kill_error: Option<BackendError>,
    /// Every `run_container` waits, as a running job does, until a stop, a kill or
    /// a removal is asked for, of any container.
    pub hold_runs: bool,
}

/// The labels of the image a [`FakeBackend`] answers with by default: this CLI's
/// contract and nothing else.
#[must_use]
pub fn matching_labels() -> ImageLabels {
    ImageLabels {
        contract: Some(CONTRACT_VERSION.to_string()),
        ..ImageLabels::default()
    }
}

#[derive(Default)]
pub struct FakeBackend {
    pub state: RefCell<FakeState>,
    pub retained: Retained,
    /// Fired by a stop, a kill or a removal, which ends every held run.
    ended: Notify,
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

    async fn check_image(&self, image: &str) -> ImageCheck {
        let mut state = self.state.borrow_mut();
        state.image_checks.push(image.to_string());
        state
            .image_answers
            .pop_front()
            .unwrap_or_else(|| ImageCheck::Matching(matching_labels()))
    }

    async fn list_pool_containers(&self, pool: &str) -> Result<PoolContainers, BackendError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::ListPoolContainers(pool.to_string()));
        if let Some(error) = state.list_error.clone() {
            return Err(error);
        }
        let needle = format!("{pool}-");
        let found: Vec<&FakeContainer> = state
            .containers
            .iter()
            .filter(|container| container.name.contains(&needle))
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
        self.ended.notify_waiters();
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::RemoveContainers(names.to_vec()));
        if let Some(error) = state.remove_error.clone() {
            return Err(error);
        }
        state
            .containers
            .retain(|container| !names.contains(&container.name));
        state.running.retain(|name| !names.contains(name));
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
        let _job = spec
            .diagnostic
            .as_ref()
            .map(|record| self.retained.started(&spec.name, record));
        let hold = {
            let mut state = self.state.borrow_mut();
            state.calls.push(Call::RunContainer(spec.clone()));
            if spec.diagnostic.is_some() {
                state.containers.push(FakeContainer {
                    name: spec.name.clone(),
                    diagnostic: true,
                });
            }
            state.jit_values.push(jit.expose().to_string());
            if let Some(hook) = state.on_run.as_mut() {
                hook();
            }
            state.hold_runs
        };
        if hold {
            self.ended.notified().await;
        }
        let mut state = self.state.borrow_mut();
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
        self.state.borrow().running.clone()
    }

    async fn stop_containers(&self, request: &StopRequest) -> Result<(), BackendError> {
        let hangs = {
            let mut state = self.state.borrow_mut();
            state.calls.push(Call::StopContainers(request.clone()));
            state.stop_hangs
        };
        if hangs {
            std::future::pending::<()>().await;
        }
        self.ended.notify_waiters();
        let mut state = self.state.borrow_mut();
        if let Some(error) = state.stop_error.clone() {
            return Err(error);
        }
        state.running.retain(|name| !request.names.contains(name));
        Ok(())
    }

    async fn kill_containers(
        &self,
        names: &[String],
        _budget: Duration,
    ) -> Result<(), BackendError> {
        self.ended.notify_waiters();
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::KillContainers(names.to_vec()));
        state.kill_error.clone().map_or(Ok(()), Err)
    }

    fn retained(&self) -> &Retained {
        &self.retained
    }

    async fn finalize_diagnostic(&self, claim: &Claim<'_>, deadlines: Deadlines) -> Finalized {
        let name = claim.name().to_string();
        self.state
            .borrow_mut()
            .calls
            .push(Call::FinalizeDiagnostic(name.clone(), deadlines));
        let mut forced = self.retained.forced();
        let mut finalized = Finalized {
            export: ExportOutcome::AlreadyExported,
            removed: false,
            kept: false,
            bundles: None,
        };
        if !claim.exported() {
            let (hook, gate) = {
                let mut state = self.state.borrow_mut();
                (state.on_export.take(), state.export_gate.clone())
            };
            if let Some(mut hook) = hook {
                hook();
                self.state.borrow_mut().on_export = Some(hook);
            }
            if let Some(gate) = gate {
                tokio::select! {
                    biased;
                    _ = forced.wait_for(|forced| *forced) => {
                        finalized.kept = true;
                        return finalized;
                    }
                    () = gate.notified() => {}
                }
            }
            let mut state = self.state.borrow_mut();
            let outcome = state
                .export_results
                .pop_front()
                .unwrap_or(ExportOutcome::Complete);
            if matches!(outcome, ExportOutcome::Complete | ExportOutcome::Gap) {
                claim.mark_exported();
                state.bundles.push((name.clone(), claim.record().clone()));
            }
            finalized.export = outcome;
            finalized.bundles = Some(bundles_of(&state, &claim.record().pool));
        }
        if *forced.borrow() {
            finalized.kept = true;
            return finalized;
        }
        if self
            .remove_containers(std::slice::from_ref(&name), Duration::ZERO)
            .await
            .is_ok()
        {
            claim.removed();
            finalized.removed = true;
        }
        finalized
    }

    async fn count_bundles(
        &self,
        record: &DiagnosticRecord,
        _deadline: tokio::time::Instant,
    ) -> Option<u32> {
        let mut state = self.state.borrow_mut();
        state.calls.push(Call::CountBundles(record.pool.clone()));
        let counted = state.bundle_counts.pop_front();
        counted.unwrap_or_else(|| Some(bundles_of(&state, &record.pool)))
    }
}

/// How many bundles the fake wrote for `pool`.
fn bundles_of(state: &FakeState, pool: &str) -> u32 {
    let count = state
        .bundles
        .iter()
        .filter(|(_, record)| record.pool == pool)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
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

    /// Moves the time forward without a sleep, as time passing between events.
    pub fn advance(&self, duration: Duration) {
        self.now.set(self.now.get() + duration);
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

/// Collects the lines, each prefixed `INFO: ` or `WARN: `, with every masked secret
/// replaced as a real sink replaces it.
#[derive(Default)]
pub struct MemoryLog {
    pub lines: RefCell<Vec<String>>,
    masks: Masks,
}

impl MemoryLog {
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.lines.borrow().clone()
    }
}

impl Log for MemoryLog {
    fn info(&self, line: &str) {
        let line = self.masks.apply(line);
        self.lines.borrow_mut().push(format!("INFO: {line}"));
    }

    fn warn(&self, line: &str) {
        let line = self.masks.apply(line);
        self.lines.borrow_mut().push(format!("WARN: {line}"));
    }

    fn masks(&self) -> &Masks {
        &self.masks
    }
}
