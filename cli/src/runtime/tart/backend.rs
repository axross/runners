//! the Tart backend: a job VM per job, cloned from the golden VM and booted for one
//! job, with every Docker command of the run contract run inside it through the
//! guest agent; `docs/operations/cli.md` describes the steps, the budgets and the
//! texts.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::Instant;

use crate::affinity::select_cpus;
use crate::diagnostics::ExportRequest;
use crate::image::check::{IMAGE_CHECK_BUDGET, ImageCheck};
use crate::macos::gate::NetworkGate;
use crate::macos::prerequisites::{
    Cause, Check, VERSION_BUDGET, check_softnet, check_softnet_with, check_tart,
};
use crate::macos::softnet::OwnerMode;
use crate::runtime::backend::{
    Backend, BackendError, ContainerSpec, CpuRequest, DiagnosticRecord, ExportOutcome, Exported,
    HostPreparation, HostRequest, JobHost, PoolContainers, RELEASE_WITHIN, StopRequest,
};
use crate::runtime::clock::Clock;
use crate::runtime::diagnostic::join_all;
use crate::runtime::docker::{RunningGuard, forward_lines};
use crate::runtime::docker_args::{
    RUN_COMMAND, info_arguments, job_run_arguments, kill_arguments, probe_run_arguments,
    remove_arguments, stop_arguments,
};
use crate::runtime::exports::Exports;
use crate::runtime::log::Log;
use crate::runtime::name::{slot_of_container_name, slot_of_probe_name};
use crate::runtime::retained::Retained;
use crate::runtime::scrub::scrub_error_text;
use crate::runtime::secret::Secret;
use crate::validate::PlannedVolume;

use super::boot::{AgentWait, BootEnd, JOB_VM, boot_failure_text, wait_for_agent};
use super::client::{Captured, TartClient, TartError};
use super::golden_check::{check_golden, clone_source};
use super::guest::Guest;
use super::hold::{HeldVm, VmLauncher};
use super::names::{job_vm_name, leftover_job_vms, parse_job_vm_name};
use super::teardown::{Stop, tear_down};

/// how long the core waits between checks while Tart does not answer.
pub const TART_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// from `tart run` to the guest agent's first answer.
pub const BOOT_DEADLINE: Duration = Duration::from_secs(120);

/// the time the stop sequence reserves to release the job VMs, after its container
/// steps: five tenths of it go to the graceful stop, two to the forced stop, two to
/// the deletion and one to the listing that tells a deletion that failed because the VM
/// was already gone. With the container stop at 12 s, the removal at 6 s and this, the
/// sequence fits the 28 s exit deadline; a diagnostic container's column ends 6 s
/// earlier than a container backend's, which leaves this much too.
pub const RELEASE_RESERVE: Duration = Duration::from_secs(10);

/// the time bounds of the steps of a job VM's cycle, which tests shorten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TartTimings {
    /// from `tart run` to the guest agent's first answer.
    pub boot_deadline: Duration,
    /// the wait between the agent's answers while the VM boots.
    pub agent_poll: Duration,
    /// the bound on one question to the agent while the VM boots.
    pub agent_call: Duration,
    /// how long the VM's Docker Engine gets to answer once the agent does.
    pub engine_wait: Duration,
    pub engine_poll: Duration,
    /// the bound on one question to the Docker Engine.
    pub engine_call: Duration,
    /// the bound on the CPU probe's `docker run`.
    pub probe: Duration,
    /// the wait between checks while Tart does not answer.
    pub tart_poll: Duration,
    /// the bound on the golden VM check.
    pub image_check: Duration,
    /// the time the stop sequence reserves to release the job VMs.
    pub release: Duration,
}

impl Default for TartTimings {
    fn default() -> Self {
        Self {
            boot_deadline: BOOT_DEADLINE,
            agent_poll: Duration::from_secs(1),
            agent_call: Duration::from_secs(5),
            engine_wait: Duration::from_secs(60),
            engine_poll: Duration::from_secs(2),
            engine_call: Duration::from_secs(10),
            probe: Duration::from_secs(30),
            tart_poll: TART_POLL_INTERVAL,
            image_check: IMAGE_CHECK_BUDGET,
            release: RELEASE_RESERVE,
        }
    }
}

/// a slot's job VM, from the start of its preparation until it is released.
#[derive(Default)]
struct Host {
    /// the VM's name, set before it is cloned so a clone that fails halfway is still
    /// deleted.
    vm: RefCell<Option<String>>,
    /// the process that holds the VM's `tart run`.
    hold: RefCell<Option<Rc<HeldVm>>>,
    /// held by the release in progress, so a second release waits for the first.
    release: tokio::sync::Mutex<()>,
    released: Cell<bool>,
}

impl Host {
    fn vm(&self) -> Option<String> {
        self.vm.borrow().clone()
    }
}

#[derive(Default)]
struct InUse {
    versions: Option<String>,
    warnings: Vec<String>,
}

pub struct TartBackend<C> {
    client: TartClient,
    launcher: VmLauncher,
    timings: TartTimings,
    clock: C,
    log: Rc<dyn Log>,
    hosts: RefCell<BTreeMap<(String, u32), Rc<Host>>>,
    /// the job VM of every container started and not yet released with its VM.
    containers: RefCell<BTreeMap<String, String>>,
    running: RefCell<BTreeSet<String>>,
    retained: Retained,
    exports: Exports,
    in_use: RefCell<InUse>,
    /// the owner and mode a test gives Softnet's file; the environment's override
    /// applies when it is unset.
    softnet_owner_mode: Option<OwnerMode>,
    /// closed while no primary network is recorded; no VM is cloned until it opens.
    network_gate: NetworkGate,
}

fn text_of(error: &TartError) -> String {
    scrub_error_text(&error.to_string(), "")
}

fn first_error(results: Vec<Result<(), BackendError>>) -> Result<(), BackendError> {
    results.into_iter().find(Result::is_err).unwrap_or(Ok(()))
}

impl<C: Clock> TartBackend<C> {
    pub fn new(
        client: TartClient,
        launcher: VmLauncher,
        timings: TartTimings,
        clock: C,
        log: Rc<dyn Log>,
    ) -> Self {
        Self {
            client,
            launcher,
            timings,
            clock,
            log,
            hosts: RefCell::new(BTreeMap::new()),
            containers: RefCell::new(BTreeMap::new()),
            running: RefCell::new(BTreeSet::new()),
            retained: Retained::default(),
            exports: Exports::new(None, None),
            in_use: RefCell::new(InUse::default()),
            softnet_owner_mode: None,
            network_gate: NetworkGate::default(),
        }
    }

    /// runs the exporter child as `exporter`, this CLI, and keeps every sink outside
    /// `checkout`; without an exporter, every export fails and a finalisation still
    /// deletes its VM.
    #[must_use]
    pub fn with_exporter(mut self, exporter: Option<PathBuf>, checkout: Option<PathBuf>) -> Self {
        self.exports = Exports::new(exporter, checkout);
        self
    }

    /// clones no job VM while `gate` is closed, which the instance's signals do until
    /// a primary network is recorded.
    #[must_use]
    pub fn with_network_gate(mut self, gate: NetworkGate) -> Self {
        self.network_gate = gate;
        self
    }

    /// judges Softnet's file as if it had this owner and mode, in a debug build only,
    /// because a test process cannot set the environment override.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    #[must_use]
    pub fn with_softnet_owner_mode(mut self, owner_mode: OwnerMode) -> Self {
        self.softnet_owner_mode = Some(owner_mode);
        self
    }

    /// the name of the VM a prepared host runs on, once it has one.
    #[must_use]
    pub fn host_vm(&self, host: &JobHost) -> Option<String> {
        self.host(&host.pool, host.slot)?.vm()
    }

    fn host(&self, pool: &str, slot: u32) -> Option<Rc<Host>> {
        self.hosts.borrow().get(&(pool.to_string(), slot)).cloned()
    }

    fn vm_of_container(&self, name: &str) -> Option<String> {
        self.containers.borrow().get(name).cloned()
    }

    async fn guest_docker(
        &self,
        vm: &str,
        arguments: &[String],
        budget: Duration,
    ) -> Result<Captured, BackendError> {
        Guest::new(&self.client, vm)
            .docker(arguments, budget)
            .await
            .map_err(|error| BackendError::new(text_of(&error)))
    }

    async fn guest_docker_checked(
        &self,
        vm: &str,
        arguments: &[String],
        budget: Duration,
        failure: &str,
    ) -> Result<Captured, BackendError> {
        let captured = self.guest_docker(vm, arguments, budget).await?;
        if captured.succeeded() {
            return Ok(captured);
        }
        Err(BackendError::new(format!(
            "{failure}: {}",
            captured.error_text()
        )))
    }

    /// the Softnet check, judging its file as a test says when it names an owner and
    /// mode.
    async fn check_softnet_once(&self) -> Check {
        let path = self.client.config().path.as_deref();
        match self.softnet_owner_mode {
            Some(owner_mode) => check_softnet_with(path, Some(owner_mode), VERSION_BUDGET).await,
            None => check_softnet(path).await,
        }
    }

    /// the Tart and Softnet checks before a boot: `None` when both pass, else the
    /// reason that holds the slot. A Tart that is missing or does not answer is waited
    /// for here, as the core waits for Docker.
    async fn wait_for_prerequisites(&self) -> Option<String> {
        let tart = loop {
            match check_tart(&self.client).await.into_result() {
                Ok(running) => break running,
                Err(refusal) => match refusal.cause {
                    Cause::Missing | Cause::NotAnswering => {
                        self.log.warn(&format!(
                            "Tart is not available ({}) - waiting.",
                            refusal.message
                        ));
                        self.clock.sleep(self.timings.tart_poll).await;
                    }
                    Cause::Version | Cause::Setup => return Some(refusal.message),
                },
            }
        };
        let softnet = match self.check_softnet_once().await.into_result() {
            Ok(running) => running,
            Err(refusal) => return Some(refusal.message),
        };
        self.note_in_use(
            &tart.0,
            &softnet.0,
            [tart.1, softnet.1].into_iter().flatten(),
        );
        None
    }

    /// logs the versions in use when they change, and each warning when it first
    /// appears, and keeps the warnings for `status`.
    fn note_in_use(&self, tart: &str, softnet: &str, warnings: impl Iterator<Item = String>) {
        let versions = format!("Tart {tart} and Softnet {softnet}");
        let warnings: Vec<String> = warnings.collect();
        let mut in_use = self.in_use.borrow_mut();
        if in_use.versions.as_deref() != Some(versions.as_str()) {
            self.log.info(&format!("Using {versions}."));
        }
        for warning in &warnings {
            if !in_use.warnings.contains(warning) {
                self.log.warn(warning);
            }
        }
        in_use.versions = Some(versions);
        in_use.warnings = warnings;
    }

    /// clones the golden VM, sizes and boots the clone, and waits for its guest agent.
    async fn boot(&self, request: &HostRequest, host: &Host) -> Result<(), BackendError> {
        let label = format!("{} slot {}", request.host.pool, request.host.slot);
        let entries = self.client.list().await.map_err(|error| {
            BackendError::new(format!(
                "the golden VM could not be listed ({})",
                text_of(&error)
            ))
        })?;
        let source = clone_source(&entries)
            .ok_or_else(|| {
                BackendError::new("no golden VM that runs here was found when the VM was cloned")
            })?
            .to_string();
        let vcpus = u32::try_from(request.vcpus)
            .map_err(|_| BackendError::new("the pool's vCPU count is out of range"))?;
        let disk_gb = request
            .disk_gb
            .ok_or_else(|| BackendError::new("the pool has no disk size for its VMs"))?;
        let vm = job_vm_name(&request.host.pool, request.host.slot, self.clock.now());
        host.vm.replace(Some(vm.clone()));
        self.client
            .clone_vm(&source, &vm)
            .await
            .map_err(|error| BackendError::new(text_of(&error)))?;
        self.client
            .set_resources(&vm, vcpus, request.memory_gb, disk_gb)
            .await
            .map_err(|error| BackendError::new(text_of(&error)))?;
        let started = if request.diagnostic {
            HeldVm::start_diagnostic(&self.client, &self.launcher, &vm, &request.host.pool)
        } else {
            HeldVm::start(&self.client, &self.launcher, &vm)
        };
        let held = started
            .map(Rc::new)
            .map_err(|error| boot_failure(&vm, &BootEnd::NoAnswer(text_of(&error))))?;
        host.hold.replace(Some(held.clone()));
        self.log
            .info(&format!("{label}: booting job VM {vm} from {source}."));
        match self.wait_for_agent(&vm, &held).await {
            Ok(()) => {
                self.log.info(&format!("{label}: job VM {vm} is up."));
                Ok(())
            }
            Err(end) => Err(boot_failure(&vm, &end)),
        }
    }

    /// waits until the guest agent of `vm` answers, `tart run` ends, or the boot
    /// deadline, counted from now, passes.
    async fn wait_for_agent(&self, vm: &str, held: &HeldVm) -> Result<(), BootEnd> {
        let wait = AgentWait {
            deadline: self.timings.boot_deadline,
            poll: self.timings.agent_poll,
            call: self.timings.agent_call,
        };
        wait_for_agent(&self.client, &self.clock, wait, vm, held).await
    }

    /// waits until the Docker Engine in the VM answers, the VM's release begins, or the
    /// wait runs out. The slot's stop does not end this wait, because the release the
    /// stop sequence starts deletes the VM, which no answer follows.
    async fn wait_for_engine(&self, vm: &str) -> Result<(), BackendError> {
        let deadline = Instant::now() + self.timings.engine_wait;
        loop {
            let answered = self
                .guest_docker(vm, &info_arguments(), self.timings.engine_call)
                .await
                .is_ok_and(|captured| captured.succeeded());
            if answered {
                return Ok(());
            }
            if self.vm_is_released_or_releasing(vm) {
                return Err(BackendError::new("CPU discovery: the job VM was released."));
            }
            if Instant::now() + self.timings.engine_poll >= deadline {
                return Err(BackendError::new(format!(
                    "CPU discovery: the Docker Engine in the job VM did not answer within {} s.",
                    self.timings.engine_wait.as_secs()
                )));
            }
            self.clock.sleep(self.timings.engine_poll).await;
        }
    }

    /// reads the CPUs the probe container may use in the VM and picks the slot's.
    async fn probe_cpus(&self, vm: &str, request: &CpuRequest) -> Result<String, BackendError> {
        let captured = self
            .guest_docker(
                vm,
                &probe_run_arguments(&request.probe_name, &request.image),
                self.timings.probe,
            )
            .await
            .map_err(|_| BackendError::new("CPU discovery: allowed CPU probe unavailable."))?;
        if !captured.succeeded() {
            return Err(BackendError::new(
                "CPU discovery: allowed CPU probe failed.",
            ));
        }
        if captured.stdout_cut || !captured.stdout.is_ascii() {
            return Err(BackendError::new(
                "CPU discovery: invalid allowed CPU list.",
            ));
        }
        let list = captured.stdout.trim_end_matches(['\n', '\r']);
        select_cpus(list, request.count, request.offset)
            .map_err(|error| BackendError::new(error.to_string()))
    }

    /// stops the host's VM as its state allows and deletes it, then forgets its
    /// containers.
    async fn delete_host_vm(&self, host: &Host, budget: Duration) -> Result<(), BackendError> {
        let Some(vm) = host.vm() else {
            return Ok(());
        };
        let hold = host.hold.borrow().clone();
        let stop = match &hold {
            Some(held) if !held.has_ended() => Stop::Graceful,
            _ => Stop::NotStarted,
        };
        tear_down(&self.client, &vm, stop, budget).await?;
        self.containers.borrow_mut().retain(|name, container_vm| {
            let gone = *container_vm == vm;
            if gone {
                self.retained.forget_if_exported(name);
            }
            !gone
        });
        // the VM is gone, so its holder has nothing left to clean up
        host.hold.borrow_mut().take();
        Ok(())
    }

    /// deletes the VM of the host that `hosts` holds under `key` and forgets the host,
    /// within `budget`; a host another release has already deleted is left alone.
    async fn release_host(
        &self,
        key: &(String, u32),
        entry: &Rc<Host>,
        budget: Duration,
    ) -> Result<(), BackendError> {
        let release = async {
            let _turn = entry.release.lock().await;
            if entry.released.get() {
                return Ok(());
            }
            self.delete_host_vm(entry, budget).await?;
            entry.released.set(true);
            let mut hosts = self.hosts.borrow_mut();
            if hosts
                .get(key)
                .is_some_and(|current| Rc::ptr_eq(current, entry))
            {
                hosts.remove(key);
            }
            Ok(())
        };
        tokio::time::timeout(budget, release)
            .await
            .unwrap_or_else(|_| {
                Err(BackendError::new(format!(
                    "releasing the job VM of {} slot {} did not finish within {} s",
                    key.0,
                    key.1,
                    budget.as_secs()
                )))
            })
    }

    /// whether the VM of `host` runs a diagnostic container that no finalisation has
    /// removed yet, whose evidence the VM's deletion would lose.
    fn holds_evidence(&self, host: &JobHost) -> bool {
        let Some(vm) = self
            .host(&host.pool, host.slot)
            .and_then(|entry| entry.vm())
        else {
            return false;
        };
        self.containers
            .borrow()
            .iter()
            .any(|(name, container_vm)| *container_vm == vm && self.retained.contains(name))
    }

    /// deletes the VM of the retained diagnostic container `name`, as a finalisation
    /// removes a container; a container whose VM is already gone has nothing left to
    /// delete.
    async fn delete_evidence_host(&self, name: &str, budget: Duration) -> Result<(), BackendError> {
        let Some(vm) = self.vm_of_container(name) else {
            return Ok(());
        };
        let found = self
            .hosts
            .borrow()
            .iter()
            .find(|(_, host)| host.vm().as_deref() == Some(vm.as_str()))
            .map(|(key, host)| (key.clone(), host.clone()));
        let Some((key, host)) = found else {
            return Ok(());
        };
        self.release_host(&key, &host, budget).await
    }

    /// the command that runs the exporter child for `request` with this backend's
    /// `tart` as its client, in the environment and with the `PATH` every `tart`
    /// call here gets.
    fn exporter(&self, request: &ExportRequest) -> Option<Command> {
        let config = self.client.config();
        let mut command = self.exports.command(request, &config.program)?;
        for (name, value) in &config.environment {
            command.env(name, value);
        }
        match &config.path {
            Some(path) => command.env("PATH", path),
            None => command.env_remove("PATH"),
        };
        Some(command)
    }

    /// removes one name `remove_containers` was given: a leftover job VM, deleted at
    /// once, a retained diagnostic container, whose VM is deleted, or a container,
    /// removed in its VM.
    async fn remove_one(&self, name: &str, budget: Duration) -> Result<(), BackendError> {
        if parse_job_vm_name(name).is_some() {
            return tear_down(&self.client, name, Stop::Forced, budget).await;
        }
        if self.retained.contains(name) {
            return self.delete_evidence_host(name, budget).await;
        }
        let Some(vm) = self.vm_of_container(name) else {
            return Ok(());
        };
        let removed = self
            .guest_docker_checked(
                &vm,
                &remove_arguments(&[name.to_string()]),
                budget,
                "Failed to remove containers",
            )
            .await
            .map(drop);
        // the container's worker releases its VM as soon as the container ends, and
        // the VM takes the container with it, so a removal that fails then costs
        // nothing. Every caller that reaches here, the stop sequence's container
        // steps, is followed by `release_hosts`, which is why only a failure with no
        // release under way is an error
        if removed.is_err() && self.vm_is_released_or_releasing(&vm) {
            return Ok(());
        }
        removed
    }

    /// whether the host that runs on `vm` is gone, or its release has begun.
    fn vm_is_released_or_releasing(&self, vm: &str) -> bool {
        let hosts = self.hosts.borrow();
        hosts
            .values()
            .find(|host| host.vm().as_deref() == Some(vm))
            .is_none_or(|host| host.released.get() || host.release.try_lock().is_err())
    }

    /// the containers of `names` that run in a VM, grouped by VM.
    fn group_by_vm(&self, names: &[String]) -> BTreeMap<String, Vec<String>> {
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in names {
            if let Some(vm) = self.vm_of_container(name) {
                groups.entry(vm).or_default().push(name.clone());
            }
        }
        groups
    }
}

fn boot_failure(vm: &str, end: &BootEnd) -> BackendError {
    BackendError::new(boot_failure_text(JOB_VM, vm, end))
}

impl<C: Clock> Backend for TartBackend<C> {
    fn run_command(&self) -> &str {
        RUN_COMMAND
    }

    fn engine_name(&self) -> &'static str {
        "Tart"
    }

    fn engine_poll_interval(&self) -> Duration {
        self.timings.tart_poll
    }

    async fn wait_until_available(&self) {
        loop {
            if self.client.list().await.is_ok() {
                return;
            }
            self.log
                .warn("Tart is not responding yet (not installed, or not answering) - waiting.");
            self.clock.sleep(self.timings.tart_poll).await;
        }
    }

    async fn check_image(&self, _image: &str) -> ImageCheck {
        let budget = self.timings.image_check;
        match tokio::time::timeout(budget, self.client.list()).await {
            Ok(Ok(entries)) => check_golden(&entries),
            Ok(Err(error)) => ImageCheck::Unavailable(format!(
                "Tart did not list the golden VMs ({})",
                text_of(&error)
            )),
            Err(_) => ImageCheck::Unavailable(format!(
                "Tart did not answer the golden VM check within {} s",
                budget.as_secs()
            )),
        }
    }

    /// refuses a `tart` or `softnet` that is missing, a version that does not run and a
    /// Softnet that is not set up or does not answer; a `tart` that does not answer is
    /// not refused, because the slots wait for it.
    async fn check_prerequisites(&self) -> Result<(), String> {
        let tart = match check_tart(&self.client).await.into_result() {
            Ok(running) => Some(running),
            Err(refusal) if refusal.cause == Cause::NotAnswering => None,
            Err(refusal) => return Err(refusal.message),
        };
        let softnet = self
            .check_softnet_once()
            .await
            .into_result()
            .map_err(|refusal| refusal.message)?;
        if let Some((tart, warning)) = tart {
            self.note_in_use(&tart, &softnet.0, warning.into_iter().chain(softnet.1));
        }
        Ok(())
    }

    fn prerequisite_warnings(&self) -> Vec<String> {
        self.in_use.borrow().warnings.clone()
    }

    fn is_own_name(&self, pool: &str, candidate: &str) -> bool {
        parse_job_vm_name(candidate).is_some_and(|job| job.pool == pool)
    }

    fn deletes_leftovers_unexported(&self) -> bool {
        true
    }

    async fn list_pool_containers(&self, pool: &str) -> Result<PoolContainers, BackendError> {
        let entries = self.client.list().await.map_err(|error| {
            BackendError::new(format!("Failed to list VMs: {}", text_of(&error)))
        })?;
        Ok(PoolContainers {
            names: leftover_job_vms(&entries, &[pool])
                .into_iter()
                .map(|entry| entry.name.clone())
                .collect(),
            diagnostic: Vec::new(),
        })
    }

    async fn remove_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> Result<(), BackendError> {
        let removals = names
            .iter()
            .map(|name| self.remove_one(name, budget))
            .collect();
        first_error(join_all(removals).await)
    }

    async fn prepare_volumes(
        &self,
        _image: &str,
        volumes: &[PlannedVolume],
    ) -> Result<(), BackendError> {
        if volumes.is_empty() {
            return Ok(());
        }
        Err(BackendError::new(
            "volumes are not supported in virtual machines",
        ))
    }

    async fn select_cpuset(&self, request: &CpuRequest) -> Result<String, BackendError> {
        let selected = async {
            let vm = slot_of_probe_name(&request.probe_name)
                .and_then(|(pool, slot)| self.host(pool, slot))
                .and_then(|host| host.vm())
                .ok_or_else(|| BackendError::new("CPU discovery: no job VM for the slot."))?;
            self.wait_for_engine(&vm).await?;
            self.probe_cpus(&vm, request).await
        }
        .await;
        if let Err(error) = &selected {
            self.log.warn(&format!("{}: {error}", request.probe_name));
        }
        selected
    }

    async fn prepare_job_host(&self, request: &HostRequest) -> HostPreparation {
        let key = (request.host.pool.clone(), request.host.slot);
        // a VM this slot could not delete earlier is tried again first, so a slot
        // never owns two
        if self.host(&key.0, key.1).is_some()
            && let Err(error) = self.release_job_host(&request.host, RELEASE_WITHIN).await
        {
            return HostPreparation::Failed(error);
        }
        let host = Rc::new(Host::default());
        self.hosts.borrow_mut().insert(key, host.clone());
        // the gate can stay closed for as long as the Mac has no network, so the
        // prerequisites are checked after it, never before
        self.network_gate.wait_until_open().await;
        if let Some(reason) = self.wait_for_prerequisites().await {
            return HostPreparation::Held(reason);
        }
        match self.boot(request, &host).await {
            Ok(()) => HostPreparation::Ready,
            Err(error) => HostPreparation::Failed(error),
        }
    }

    async fn release_job_host(&self, host: &JobHost, budget: Duration) -> Result<(), BackendError> {
        let key = (host.pool.clone(), host.slot);
        let Some(entry) = self.hosts.borrow().get(&key).cloned() else {
            return Ok(());
        };
        self.release_host(&key, &entry, budget).await
    }

    fn host_awaits_finalisation(&self, host: &JobHost) -> bool {
        self.holds_evidence(host)
    }

    fn prepared_hosts(&self) -> Vec<JobHost> {
        self.hosts
            .borrow()
            .keys()
            .map(|(pool, slot)| JobHost {
                pool: pool.clone(),
                slot: *slot,
            })
            .collect()
    }

    fn release_budget(&self) -> Duration {
        self.timings.release
    }

    async fn run_container(&self, spec: &ContainerSpec, jit: &Secret) -> Result<i32, BackendError> {
        let vm = slot_of_container_name(&spec.name)
            .and_then(|(pool, slot)| self.host(pool, slot))
            .and_then(|host| host.vm())
            .ok_or_else(|| BackendError::new("no job VM is prepared for the container"))?;
        self.containers
            .borrow_mut()
            .insert(spec.name.clone(), vm.clone());
        let mut child = Guest::new(&self.client, &vm)
            .launch(&job_run_arguments(spec))
            .map_err(|error| BackendError::new(text_of(&error)))?;
        if let Some(mut input) = child.stdin.take() {
            // a launcher that ends without reading all of it shows in its exit code;
            // closing the pipe is what ends the configuration
            let _ = input.write_all(jit.expose().as_bytes()).await;
        }
        // a diagnostic container is retained instead of running, until a
        // finalisation removes it
        let _job = spec
            .diagnostic
            .as_ref()
            .map(|record| self.retained.started(&spec.name, record));
        let _running = spec
            .diagnostic
            .is_none()
            .then(|| RunningGuard::enter(&self.running, &spec.name));
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let log = &*self.log;
        let (_, _, status) = tokio::join!(
            forward_lines(stdout, log),
            forward_lines(stderr, log),
            child.wait()
        );
        let status = status
            .map_err(|error| BackendError::new(format!("the tart client failed: {error}")))?;
        Ok(status.code().unwrap_or(-1))
    }

    fn running_containers(&self) -> Vec<String> {
        self.running.borrow().iter().cloned().collect()
    }

    async fn stop_containers(&self, request: &StopRequest) -> Result<(), BackendError> {
        let stops = self
            .group_by_vm(&request.names)
            .into_iter()
            .map(|(vm, names)| async move {
                self.guest_docker_checked(
                    &vm,
                    &stop_arguments(request.grace.as_secs(), &names),
                    request.budget,
                    "Failed to stop containers",
                )
                .await
                .map(drop)
            })
            .collect();
        first_error(join_all(stops).await)
    }

    async fn kill_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> Result<(), BackendError> {
        let kills = self
            .group_by_vm(names)
            .into_iter()
            .map(|(vm, names)| async move {
                self.guest_docker_checked(
                    &vm,
                    &kill_arguments(&names),
                    budget,
                    "Failed to end containers",
                )
                .await
                .map(drop)
            })
            .collect();
        first_error(join_all(kills).await)
    }

    fn retained(&self) -> &Retained {
        &self.retained
    }

    /// exports the container's evidence from its VM into its record's sink by
    /// `deadline`, once the pool and sink's admission lock is free: the exporter child
    /// reads it with `tart exec <vm> docker inspect` and `docker cp`. A container
    /// whose VM is gone, or that cannot take the lock by then, fails the export.
    async fn export_diagnostic(
        &self,
        name: &str,
        record: &DiagnosticRecord,
        deadline: Instant,
    ) -> Exported {
        let Some(vm) = self.vm_of_container(name) else {
            return Exported::FAILED;
        };
        let lock = self.exports.admission(record);
        let Ok(admission) = tokio::time::timeout_at(deadline, lock.lock()).await else {
            return Exported::FAILED;
        };
        let request = self
            .exports
            .request(record, Some(name.to_string()), Some(vm), deadline);
        let command = self.exporter(&request);
        self.exports.run(command, deadline, Some(admission)).await
    }

    async fn count_bundles(&self, record: &DiagnosticRecord, deadline: Instant) -> Option<u32> {
        let request = self.exports.request(record, None, None, deadline);
        let command = self.exporter(&request);
        let counted = self.exports.run(command, deadline, None).await;
        counted
            .bundles
            .filter(|_| counted.outcome == ExportOutcome::Complete)
    }
}
