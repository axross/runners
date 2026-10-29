//! the container backend, which runs the `docker` command line client and never
//! trusts a client's output to be short or valid text; `docs/operations/cli.md`
//! describes the argument lists, budgets and the shim.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Stdio;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::affinity::{MAX_CPU_LIST_BYTES, select_cpus};
use crate::diagnostics::{ADMITTED, BUNDLES, ExportRequest, outcome_of_exit};
use crate::image::check::{
    IMAGE_CHECK_BUDGET, ImageCheck, ImageLabels, MAX_INSPECT_BYTES, checked_reference,
};
use crate::validate::PlannedVolume;

use super::backend::{
    Backend, BackendError, ContainerSpec, CpuRequest, DiagnosticRecord, ExportOutcome, Exported,
    PoolContainers, StopRequest,
};
use super::clock::Clock;
use super::docker_args::{
    JIT_VARIABLE, image_inspect_arguments, image_list_arguments, info_arguments, job_run_arguments,
    kill_arguments, pool_diagnostic_list_arguments, pool_list_arguments, probe_find_arguments,
    probe_run_arguments, remove_arguments, stop_arguments, volume_chown_arguments,
    volume_create_arguments,
};
use super::log::Log;
use super::retained::Retained;
use super::scrub::scrub_error_text;
use super::secret::Secret;

/// how long Docker is given between checks while it is not answering.
pub const DOCKER_POLL_INTERVAL: Duration = Duration::from_secs(5);

const MAX_LINE_BYTES: u64 = 64 * 1024;

/// the Windows flag that gives a client its own process group, so a console Ctrl+C
/// does not reach it.
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// how long an exporter gets to end after its liveness pipe closes at the export
/// deadline, before it is killed.
pub const EXPORTER_TERMINATION: Duration = Duration::from_secs(1);

const MAX_EXPORTER_LINE_BYTES: u64 = 64;

/// the time bounds on the clients other than a job container's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockerLimits {
    /// a bound on a listing, a volume command or an info call; a removal and a stop
    /// take the budget their caller passes instead.
    pub command_budget: Duration,
    /// the whole budget of one CPU probe, client termination included.
    pub probe_budget: Duration,
    /// the budget of reconciling and removing a leftover probe.
    pub probe_cleanup_budget: Duration,
    /// the budget of one image check, both its clients together.
    pub image_check_budget: Duration,
    /// the time reserved at the end of a budget to end a client that overran it.
    pub client_termination: Duration,
    /// the most bytes of probe output read.
    pub probe_output_bytes: usize,
}

impl Default for DockerLimits {
    fn default() -> Self {
        Self {
            command_budget: Duration::from_secs(120),
            probe_budget: Duration::from_secs(30),
            probe_cleanup_budget: Duration::from_secs(5),
            image_check_budget: IMAGE_CHECK_BUDGET,
            client_termination: Duration::from_secs(1),
            probe_output_bytes: MAX_CPU_LIST_BYTES,
        }
    }
}

/// how a job container's `docker run` client is started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum JobLauncher {
    /// as a child of this process, killed when the run is dropped.
    #[default]
    Direct,
    /// by the shim, which cleans up the container when its liveness pipe closes,
    /// however this process ended.
    #[cfg(unix)]
    Shim(PathBuf),
}

/// the client a release build always runs, found by the operating system's search.
pub const DOCKER_PROGRAM: &str = "docker";

/// resolves the client `start` runs: a release build always returns
/// [`DOCKER_PROGRAM`], so a release binary cannot be made to run another program,
/// and a debug build runs `requested` for the tests (on Windows a bare name is
/// found in this program's directory and the system directories before `PATH`, so a
/// fake `docker` placed first on `PATH` loses to a real one in `System32`).
#[must_use]
pub fn resolve_program(requested: Option<&OsStr>) -> OsString {
    #[cfg(debug_assertions)]
    if let Some(requested) = requested {
        return requested.to_os_string();
    }
    #[cfg(not(debug_assertions))]
    let _ = requested;
    OsString::from(DOCKER_PROGRAM)
}

/// how to reach the `docker` client.
#[derive(Debug, Clone)]
pub struct DockerConfig {
    /// the client to run, found on `PATH` when it has no directory.
    pub program: OsString,
    /// variables to add to every client's environment.
    pub environment: Vec<(OsString, OsString)>,
    pub limits: DockerLimits,
    pub launcher: JobLauncher,
    /// the program run as the exporter child: this CLI; without one, every export
    /// fails, and its container is still removed.
    pub exporter: Option<PathBuf>,
    /// the checkout every sink must lie outside, when one was found.
    pub checkout: Option<PathBuf>,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            program: OsString::from(DOCKER_PROGRAM),
            environment: Vec::new(),
            limits: DockerLimits::default(),
            launcher: JobLauncher::Direct,
            exporter: None,
            checkout: None,
        }
    }
}

struct Captured {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Captured {
    fn succeeded(&self) -> bool {
        self.code == Some(0)
    }

    /// the client's output for an error message, on one line and cut as a registration
    /// error is.
    fn output_text(&self) -> String {
        scrub_error_text(&format!("{} {}", self.stdout, self.stderr), "")
    }
}

/// builds a client command with the JIT variable cleared, so only the one `run`
/// that sets it carries a registration; the client is killed when its future is
/// dropped and starts in its own process group, so a Ctrl+C reaches only the CLI.
pub(crate) fn client_command(
    program: &OsStr,
    environment: &[(OsString, OsString)],
    arguments: &[String],
) -> Command {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env_remove(JIT_VARIABLE)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    for (name, value) in environment {
        command.env(name, value);
    }
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    command
}

/// the admission lock of one pool and sink, held by an export until its child has
/// created the bundle.
type Admission = Rc<tokio::sync::Mutex<()>>;

pub struct DockerBackend<C> {
    config: DockerConfig,
    clock: C,
    log: Rc<dyn Log>,
    running: RefCell<BTreeSet<String>>,
    retained: Retained,
    admissions: RefCell<BTreeMap<(String, String), Admission>>,
}

/// removes a container from the running set when its run ends, a dropped future
/// included; a panic leaves the name, so whatever supervises the code can still read
/// and remove the container.
struct RunningGuard<'a> {
    set: &'a RefCell<BTreeSet<String>>,
    name: String,
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.set.borrow_mut().remove(&self.name);
        }
    }
}

impl<C: Clock> DockerBackend<C> {
    pub fn new(config: DockerConfig, clock: C, log: Rc<dyn Log>) -> Self {
        Self {
            config,
            clock,
            log,
            running: RefCell::new(BTreeSet::new()),
            retained: Retained::default(),
            admissions: RefCell::new(BTreeMap::new()),
        }
    }

    fn command(&self, arguments: &[String]) -> Command {
        client_command(&self.config.program, &self.config.environment, arguments)
    }

    /// the command that starts a job container, the client itself or the shim,
    /// which is not killed when the run is dropped because closing its liveness pipe
    /// makes it clean up, and which writes the client's standard error to its standard
    /// output.
    fn job_command(&self, spec: &ContainerSpec) -> Command {
        let arguments = job_run_arguments(spec);
        match &self.config.launcher {
            JobLauncher::Direct => {
                let mut command = self.command(&arguments);
                command.stderr(Stdio::piped());
                command
            }
            #[cfg(unix)]
            JobLauncher::Shim(shim) => {
                let mut command = Command::new(shim);
                command
                    .arg(crate::shim::SHIM_COMMAND)
                    .arg("--name")
                    .arg(&spec.name);
                if spec.diagnostic.is_some() {
                    command.arg(crate::shim::DIAGNOSTIC_FLAG);
                }
                command
                    .arg("--")
                    .arg(&self.config.program)
                    .args(&arguments)
                    .stdin(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .kill_on_drop(false)
                    .process_group(0);
                for (name, value) in &self.config.environment {
                    command.env(name, value);
                }
                command
            }
        }
    }

    /// runs a client to its end within `budget` and returns what it printed.
    async fn capture(
        &self,
        arguments: &[String],
        budget: Duration,
    ) -> Result<Captured, BackendError> {
        let mut command = self.command(arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = command.spawn().map_err(|error| {
            BackendError::new(format!("the docker client could not start: {error}"))
        })?;
        let output = tokio::time::timeout(budget, child.wait_with_output())
            .await
            .map_err(|_| {
                BackendError::new(format!(
                    "the docker client did not finish within {} s",
                    budget.as_secs()
                ))
            })?
            .map_err(|error| BackendError::new(format!("the docker client failed: {error}")))?;
        Ok(Captured {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    async fn capture_checked(
        &self,
        arguments: &[String],
        failure: &str,
    ) -> Result<Captured, BackendError> {
        let captured = self
            .capture(arguments, self.config.limits.command_budget)
            .await?;
        if captured.succeeded() {
            return Ok(captured);
        }
        Err(BackendError::new(format!(
            "{failure}: {}",
            captured.output_text()
        )))
    }

    /// runs a client with standard error discarded and at most `cap` bytes of
    /// standard output, ending it at `budget` less the time reserved to terminate
    /// it; a client over the cap or the budget is ended and refused.
    async fn run_capped(
        &self,
        arguments: &[String],
        budget: Duration,
        cap: usize,
    ) -> Result<(i32, Vec<u8>), CappedFailure> {
        let termination = self.config.limits.client_termination;
        let read_budget = budget
            .checked_sub(termination)
            .filter(|left| !left.is_zero())
            .ok_or(CappedFailure::DeadlineSpent)?;
        let mut command = self.command(arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| CappedFailure::Unavailable)?;
        let Some(mut stdout) = child.stdout.take() else {
            end_client(&mut child, termination).await;
            return Err(CappedFailure::Unavailable);
        };
        let outcome = tokio::time::timeout(read_budget, async {
            let bytes = read_capped(&mut stdout, cap).await?;
            let status = child.wait().await.ok()?;
            Some((status.code().unwrap_or(-1), bytes))
        })
        .await;
        match outcome {
            Ok(Some(result)) => Ok(result),
            Ok(None) | Err(_) => {
                end_client(&mut child, termination).await;
                Err(CappedFailure::Unavailable)
            }
        }
    }

    /// [`run_capped`](Self::run_capped) for the CPU probe, whose failures read as
    /// CPU discovery errors.
    async fn bounded(
        &self,
        arguments: &[String],
        budget: Duration,
        cap: usize,
    ) -> Result<(i32, Vec<u8>), BackendError> {
        self.run_capped(arguments, budget, cap)
            .await
            .map_err(|failure| match failure {
                CappedFailure::DeadlineSpent => BackendError::new("CPU discovery: deadline spent."),
                CappedFailure::Unavailable => BackendError::new(
                    "CPU discovery: Docker client unavailable, invalid or over budget.",
                ),
            })
    }

    /// removes a leftover CPU probe of this exact name, and only one that carries
    /// the probe label.
    async fn remove_probe(&self, probe: &str) -> Result<(), BackendError> {
        let limits = self.config.limits;
        let started = Instant::now();
        let (code, bytes) = self
            .bounded(
                &probe_find_arguments(probe),
                limits.probe_cleanup_budget,
                limits.probe_output_bytes,
            )
            .await?;
        if code != 0 {
            return Err(BackendError::new(
                "CPU discovery: probe reconciliation unavailable.",
            ));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let found = String::from_utf8_lossy(&bytes);
        let found = strip_line_end(&found);
        if found != probe {
            return Err(BackendError::new(
                "CPU discovery: unexpected probe identity.",
            ));
        }
        let remaining = limits
            .probe_cleanup_budget
            .saturating_sub(started.elapsed());
        let (code, _) = self
            .bounded(
                &remove_arguments(&[probe.to_string()]),
                remaining,
                limits.probe_output_bytes,
            )
            .await?;
        if code != 0 {
            return Err(BackendError::new(
                "CPU discovery: probe removal unconfirmed.",
            ));
        }
        Ok(())
    }

    async fn read_allowed_cpus(&self, request: &CpuRequest) -> Result<String, BackendError> {
        let limits = self.config.limits;
        let started = Instant::now();
        self.remove_probe(&request.probe_name).await?;
        let remaining = limits.probe_budget.saturating_sub(started.elapsed());
        let (code, bytes) = self
            .bounded(
                &probe_run_arguments(&request.probe_name, &request.image),
                remaining,
                limits.probe_output_bytes,
            )
            .await?;
        if code != 0 {
            return Err(BackendError::new(
                "CPU discovery: allowed CPU probe failed.",
            ));
        }
        let text = String::from_utf8(bytes)
            .ok()
            .filter(|text| text.is_ascii())
            .ok_or_else(|| BackendError::new("CPU discovery: invalid allowed CPU list."))?;
        select_cpus(strip_line_end(&text), request.count, request.offset)
            .map_err(|error| BackendError::new(error.to_string()))
    }
}

impl<C: Clock> DockerBackend<C> {
    fn admission(&self, record: &DiagnosticRecord) -> Admission {
        self.admissions
            .borrow_mut()
            .entry((record.pool.clone(), record.directory.clone()))
            .or_default()
            .clone()
    }

    /// the exporter request for `record`'s sink; no `container` only counts its
    /// bundles.
    fn export_request(
        &self,
        record: &DiagnosticRecord,
        container: Option<String>,
        deadline: tokio::time::Instant,
    ) -> ExportRequest {
        ExportRequest {
            pool: record.pool.clone(),
            container,
            directory: record.directory.clone(),
            raw_records: record.raw_records,
            checkout: self.config.checkout.clone(),
            deadline: deadline.saturating_duration_since(tokio::time::Instant::now()),
        }
    }

    /// starts the exporter child for `request` with a liveness pipe as its standard
    /// input, closing which ends it, so it can kill its own clients first; its
    /// standard error is this process's, and it writes only a panic's location there.
    fn exporter(&self, request: &ExportRequest) -> Option<Child> {
        let program = self.config.exporter.as_ref()?;
        let mut command = Command::new(program);
        command
            .args(request.arguments(&self.config.program))
            .env_remove(JIT_VARIABLE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(false);
        for (name, value) in &self.config.environment {
            command.env(name, value);
        }
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        command.spawn().ok()
    }

    /// runs the exporter child for `request` until it ends or `deadline` passes;
    /// `admission` is released once the child has created the bundle or ended.
    async fn run_exporter(
        &self,
        request: &ExportRequest,
        deadline: tokio::time::Instant,
        mut admission: Option<tokio::sync::MutexGuard<'_, ()>>,
    ) -> Exported {
        let Some(mut child) = self.exporter(request) else {
            return Exported::FAILED;
        };
        let liveness = child.stdin.take();
        let stdout = child.stdout.take();
        let (admitted, bundles) = (Cell::new(false), Cell::new(None));
        let reading = async {
            if let Some(stdout) = stdout {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = Vec::new();
                    let read = (&mut reader)
                        .take(MAX_EXPORTER_LINE_BYTES)
                        .read_until(b'\n', &mut line)
                        .await
                        .unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end();
                    if text == ADMITTED {
                        admitted.set(true);
                        admission.take();
                    } else if let Some(count) = text.strip_prefix(BUNDLES) {
                        bundles.set(count.parse().ok());
                    }
                }
            }
            admission.take();
            child.wait().await.ok()
        };
        let outcome = match tokio::time::timeout_at(deadline, reading).await {
            Ok(Some(status)) => outcome_of_exit(status.code()),
            Ok(None) => ExportOutcome::Failed,
            Err(_) => {
                drop(liveness);
                if tokio::time::timeout(EXPORTER_TERMINATION, child.wait())
                    .await
                    .is_err()
                {
                    let _ = child.kill().await;
                }
                ExportOutcome::Failed
            }
        };
        Exported {
            outcome,
            admitted: admitted.get(),
            bundles: bundles.get(),
        }
    }
}

enum CappedFailure {
    /// the budget left no time to run it.
    DeadlineSpent,
    /// the client could not start, overran its cap or its budget, or its output could
    /// not be read.
    Unavailable,
}

fn strip_line_end(text: &str) -> &str {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.strip_suffix('\r').unwrap_or(text)
}

/// reads to the end, failing when more than `cap` bytes arrive or the read fails.
async fn read_capped<R: AsyncRead + Unpin>(reader: &mut R, cap: usize) -> Option<Vec<u8>> {
    let limit = u64::try_from(cap).ok()?.checked_add(1)?;
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes).await.ok()?;
    if bytes.len() > cap {
        return None;
    }
    Some(bytes)
}

async fn end_client(child: &mut Child, limit: Duration) {
    let _ = tokio::time::timeout(limit, child.kill()).await;
}

/// forwards each line a container prints to the log until its stream ends; a line
/// longer than [`MAX_LINE_BYTES`] goes in pieces, each cut where it splits no masked
/// secret.
async fn forward_lines<R: AsyncRead + Unpin>(stream: Option<R>, log: &dyn Log) {
    let Some(stream) = stream else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut piece = Vec::new();
    loop {
        let read = (&mut reader)
            .take(MAX_LINE_BYTES)
            .read_until(b'\n', &mut piece)
            .await
            .unwrap_or(0);
        let line_ended = read == 0 || piece.ends_with(b"\n");
        let held = if line_ended { 0 } else { log.held_back(&piece) };
        let next = piece.split_off(piece.len() - held);
        if !piece.is_empty() {
            let text = String::from_utf8_lossy(&piece);
            log.info(text.trim_end_matches(['\n', '\r']));
        }
        if read == 0 {
            return;
        }
        piece = next;
    }
}

impl<C: Clock> Backend for DockerBackend<C> {
    async fn wait_until_available(&self) {
        loop {
            let answered = self
                .capture(&info_arguments(), self.config.limits.command_budget)
                .await
                .is_ok_and(|captured| captured.succeeded());
            if answered {
                return;
            }
            self.log.warn(
                "Docker Desktop is not responding yet (still starting, or not running) - waiting.",
            );
            self.clock.sleep(DOCKER_POLL_INTERVAL).await;
        }
    }

    async fn check_image(&self, image: &str) -> ImageCheck {
        let budget = self.config.limits.image_check_budget;
        let started = Instant::now();
        let unanswered = || {
            ImageCheck::Unavailable(format!(
                "Docker did not answer the image check within {} s",
                budget.as_secs()
            ))
        };
        let reference = checked_reference(image);
        let inspected = self
            .run_capped(
                &image_inspect_arguments(&reference),
                budget,
                MAX_INSPECT_BYTES,
            )
            .await;
        match inspected {
            Ok((0, output)) => {
                // an inspect that succeeds but prints something else is an image
                // whose labels cannot be read, so its contract is malformed
                return ImageLabels::from_inspect(&output).map_or_else(
                    || ImageCheck::Mismatched(ImageLabels::default()),
                    ImageCheck::of_labels,
                );
            }
            Ok(_) => {}
            Err(_) => return unanswered(),
        }
        let remaining = budget.saturating_sub(started.elapsed());
        match self
            .run_capped(
                &image_list_arguments(&reference),
                remaining,
                MAX_INSPECT_BYTES,
            )
            .await
        {
            Ok((0, output)) if output.iter().all(u8::is_ascii_whitespace) => ImageCheck::Missing,
            Ok((0, _)) => ImageCheck::Unavailable(
                "docker image inspect failed for an image that exists".to_string(),
            ),
            Ok(_) | Err(_) => unanswered(),
        }
    }

    async fn list_pool_containers(&self, pool: &str) -> Result<PoolContainers, BackendError> {
        let all = self
            .capture_checked(&pool_list_arguments(pool), "Failed to list containers")
            .await?;
        let diagnostic = self
            .capture(
                &pool_diagnostic_list_arguments(pool),
                self.config.limits.command_budget,
            )
            .await
            .ok()
            .filter(Captured::succeeded)
            .ok_or_else(|| {
                BackendError::new(
                    "Diagnostic lifecycle identification unavailable; stale cleanup refused.",
                )
            })?;
        let lines = |captured: &Captured| -> Vec<String> {
            captured
                .stdout
                .lines()
                .map(|line| line.trim_end_matches('\r').to_string())
                .collect()
        };
        Ok(PoolContainers {
            names: lines(&all),
            diagnostic: lines(&diagnostic),
        })
    }

    async fn remove_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> Result<(), BackendError> {
        if names.is_empty() {
            return Ok(());
        }
        let captured = self.capture(&remove_arguments(names), budget).await?;
        if captured.succeeded() {
            let mut running = self.running.borrow_mut();
            for name in names {
                running.remove(name);
            }
            return Ok(());
        }
        Err(BackendError::new(format!(
            "Failed to remove containers: {}",
            captured.output_text()
        )))
    }

    async fn prepare_volumes(
        &self,
        image: &str,
        volumes: &[PlannedVolume],
    ) -> Result<(), BackendError> {
        for volume in volumes {
            self.capture_checked(
                &volume_create_arguments(volume),
                &format!("Failed to create Docker volume '{}'", volume.name),
            )
            .await?;
        }
        if volumes.is_empty() {
            return Ok(());
        }
        self.capture_checked(
            &volume_chown_arguments(image, volumes),
            "Failed to set ownership of the cache volumes",
        )
        .await?;
        Ok(())
    }

    async fn select_cpuset(&self, request: &CpuRequest) -> Result<String, BackendError> {
        let selection = self.read_allowed_cpus(request).await;
        // the cleanup runs whatever the probe did, and its failure outranks the
        // probe's result, as in the PowerShell `finally`.
        self.remove_probe(&request.probe_name).await?;
        selection
    }

    async fn run_container(&self, spec: &ContainerSpec, jit: &Secret) -> Result<i32, BackendError> {
        let mut command = self.job_command(spec);
        command
            .env(JIT_VARIABLE, jit.expose())
            .stdout(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            BackendError::new(format!("the docker client could not start: {error}"))
        })?;
        // the shim's liveness pipe, closed when this run ends or is dropped
        let _liveness = child.stdin.take();
        // a diagnostic container is retained instead of running, until a
        // finalisation removes it
        let _job = spec
            .diagnostic
            .as_ref()
            .map(|record| self.retained.started(&spec.name, record));
        let _running = spec.diagnostic.is_none().then(|| {
            self.running.borrow_mut().insert(spec.name.clone());
            RunningGuard {
                set: &self.running,
                name: spec.name.clone(),
            }
        });
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let log = &*self.log;
        let (_, _, status) = tokio::join!(
            forward_lines(stdout, log),
            forward_lines(stderr, log),
            child.wait()
        );
        let status = status
            .map_err(|error| BackendError::new(format!("the docker client failed: {error}")))?;
        Ok(status.code().unwrap_or(-1))
    }

    fn running_containers(&self) -> Vec<String> {
        self.running.borrow().iter().cloned().collect()
    }

    async fn stop_containers(&self, request: &StopRequest) -> Result<(), BackendError> {
        if request.names.is_empty() {
            return Ok(());
        }
        let captured = self
            .capture(
                &stop_arguments(request.grace.as_secs(), &request.names),
                request.budget,
            )
            .await?;
        if captured.succeeded() {
            return Ok(());
        }
        Err(BackendError::new(format!(
            "Failed to stop containers: {}",
            captured.output_text()
        )))
    }

    async fn kill_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> Result<(), BackendError> {
        if names.is_empty() {
            return Ok(());
        }
        let captured = self.capture(&kill_arguments(names), budget).await?;
        if captured.succeeded() {
            return Ok(());
        }
        Err(BackendError::new(format!(
            "Failed to end containers: {}",
            captured.output_text()
        )))
    }

    fn retained(&self) -> &Retained {
        &self.retained
    }

    /// exports the container into its record's sink by `deadline`, once the pool and
    /// sink's admission lock is free; an export that cannot take it by then fails.
    async fn export_diagnostic(
        &self,
        name: &str,
        record: &DiagnosticRecord,
        deadline: tokio::time::Instant,
    ) -> Exported {
        let lock = self.admission(record);
        let Ok(admission) = tokio::time::timeout_at(deadline, lock.lock()).await else {
            return Exported::FAILED;
        };
        let request = self.export_request(record, Some(name.to_string()), deadline);
        self.run_exporter(&request, deadline, Some(admission)).await
    }

    async fn count_bundles(
        &self,
        record: &DiagnosticRecord,
        deadline: tokio::time::Instant,
    ) -> Option<u32> {
        let request = self.export_request(record, None, deadline);
        let counted = self.run_exporter(&request, deadline, None).await;
        counted
            .bundles
            .filter(|_| counted.outcome == ExportOutcome::Complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::fake::MemoryLog;

    #[tokio::test(flavor = "current_thread")]
    async fn a_secret_across_two_pieces_of_a_long_line_is_masked() {
        let log = MemoryLog::default();
        log.mask(&Secret::new("jit-0123456789".to_string()));
        let before = usize::try_from(MAX_LINE_BYTES).unwrap() - 4;
        let mut output = "a".repeat(before);
        output.push_str("jit-0123456789 and on\nnext\n");
        forward_lines(Some(output.as_bytes()), &log).await;
        let lines = log.lines();
        assert_eq!(lines.len(), 3, "the long line goes in two pieces");
        let texts: String = lines
            .iter()
            .map(|line| line.strip_prefix("INFO: ").unwrap())
            .collect();
        assert_eq!(texts, format!("{}<masked> and onnext", "a".repeat(before)));
    }
}
