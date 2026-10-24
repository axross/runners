//! The container backend: it runs the `docker` command line client.
//!
//! Every client starts in its own process group, so a Ctrl+C in the terminal reaches
//! the CLI only, which then stops the containers in order. A client's output is
//! never trusted to be short or valid text: the probe's is capped, and a job
//! container's lines are forwarded in bounded pieces.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::process::Stdio;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::affinity::{MAX_CPU_LIST_BYTES, select_cpus};
use crate::validate::PlannedVolume;

use super::backend::{
    Backend, BackendError, ContainerSpec, CpuRequest, PoolContainers, StopRequest,
};
use super::clock::Clock;
use super::docker_args::{
    JIT_VARIABLE, info_arguments, job_run_arguments, pool_diagnostic_list_arguments,
    pool_list_arguments, probe_find_arguments, probe_run_arguments, remove_arguments,
    stop_arguments, volume_chown_arguments, volume_create_arguments,
};
use super::log::Log;
use super::secret::Secret;

/// How long Docker is given between checks while it is not answering.
pub const DOCKER_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The most bytes of one line of a container's output forwarded as one message.
const MAX_LINE_BYTES: u64 = 64 * 1024;

/// A Windows process creation flag: the client gets its own process group, so a
/// console Ctrl+C is not delivered to it.
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// The time bounds on the clients other than a job container's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockerLimits {
    /// A bound on a listing, a removal, a stop, a volume command or an info call.
    /// The PowerShell supervisor had none.
    pub command_budget: Duration,
    /// The whole budget of one CPU probe, client termination included.
    pub probe_budget: Duration,
    /// The budget of reconciling and removing a leftover probe.
    pub probe_cleanup_budget: Duration,
    /// The time reserved at the end of a budget to end a client that overran it.
    pub client_termination: Duration,
    /// The most bytes of probe output read.
    pub probe_output_bytes: usize,
}

impl Default for DockerLimits {
    fn default() -> Self {
        Self {
            command_budget: Duration::from_secs(120),
            probe_budget: Duration::from_secs(30),
            probe_cleanup_budget: Duration::from_secs(5),
            client_termination: Duration::from_secs(1),
            probe_output_bytes: MAX_CPU_LIST_BYTES,
        }
    }
}

/// How to reach the `docker` client.
#[derive(Debug, Clone)]
pub struct DockerConfig {
    /// The client to run, found on `PATH` when it has no directory.
    pub program: OsString,
    /// Variables to add to every client's environment.
    pub environment: Vec<(OsString, OsString)>,
    pub limits: DockerLimits,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            program: OsString::from("docker"),
            environment: Vec::new(),
            limits: DockerLimits::default(),
        }
    }
}

/// What a finished client left.
struct Captured {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Captured {
    fn succeeded(&self) -> bool {
        self.code == Some(0)
    }

    /// The client's output for an error message, standard output then standard
    /// error, on one line.
    fn output_text(&self) -> String {
        format!("{} {}", self.stdout, self.stderr)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub struct DockerBackend<C> {
    config: DockerConfig,
    clock: C,
    log: Rc<dyn Log>,
    running: RefCell<BTreeSet<String>>,
}

/// Removes a container from the running set when its run ends, however it ends.
struct RunningGuard<'a> {
    set: &'a RefCell<BTreeSet<String>>,
    name: String,
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.set.borrow_mut().remove(&self.name);
    }
}

impl<C: Clock> DockerBackend<C> {
    pub fn new(config: DockerConfig, clock: C, log: Rc<dyn Log>) -> Self {
        Self {
            config,
            clock,
            log,
            running: RefCell::new(BTreeSet::new()),
        }
    }

    /// Builds a client command. The JIT variable is cleared, so only the one `run`
    /// that sets it carries a registration; the client is killed when its future is
    /// dropped, and starts in its own process group.
    fn command(&self, arguments: &[String]) -> Command {
        let mut command = Command::new(&self.config.program);
        command
            .args(arguments)
            .env_remove(JIT_VARIABLE)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        for (name, value) in &self.config.environment {
            command.env(name, value);
        }
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        command
    }

    /// Runs a client to its end within `budget` and returns what it printed.
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

    /// Runs a client with standard error discarded and at most `cap` bytes of
    /// standard output, ending it at `budget` less the time reserved to terminate
    /// it. A client over the cap or the budget is ended and refused.
    async fn bounded(
        &self,
        arguments: &[String],
        budget: Duration,
        cap: usize,
    ) -> Result<(i32, Vec<u8>), BackendError> {
        let unavailable = || {
            BackendError::new("CPU discovery: Docker client unavailable, invalid or over budget.")
        };
        let termination = self.config.limits.client_termination;
        let read_budget = budget
            .checked_sub(termination)
            .filter(|left| !left.is_zero());
        let Some(read_budget) = read_budget else {
            return Err(BackendError::new("CPU discovery: deadline spent."));
        };
        let mut command = self.command(arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| unavailable())?;
        let Some(mut stdout) = child.stdout.take() else {
            end_client(&mut child, termination).await;
            return Err(unavailable());
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
                Err(unavailable())
            }
        }
    }

    /// Removes a leftover CPU probe of this exact name, and only one that carries
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

/// Removes one trailing `\n` or `\r\n`.
fn strip_line_end(text: &str) -> &str {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.strip_suffix('\r').unwrap_or(text)
}

/// Reads to the end, failing when more than `cap` bytes arrive or the read fails.
async fn read_capped<R: AsyncRead + Unpin>(reader: &mut R, cap: usize) -> Option<Vec<u8>> {
    let limit = u64::try_from(cap).ok()?.checked_add(1)?;
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes).await.ok()?;
    if bytes.len() > cap {
        return None;
    }
    Some(bytes)
}

/// Kills a client and waits for it for at most `limit`.
async fn end_client(child: &mut Child, limit: Duration) {
    let _ = tokio::time::timeout(limit, child.kill()).await;
}

/// Forwards each line a container prints to the log, in pieces of at most
/// [`MAX_LINE_BYTES`], until its stream ends.
async fn forward_lines<R: AsyncRead + Unpin>(stream: Option<R>, log: &dyn Log) {
    let Some(stream) = stream else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut reader)
            .take(MAX_LINE_BYTES)
            .read_until(b'\n', &mut line)
            .await
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        log.info(text.trim_end_matches(['\n', '\r']));
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
        let mut command = self.command(&job_run_arguments(spec));
        command
            .env(JIT_VARIABLE, jit.expose())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            BackendError::new(format!("the docker client could not start: {error}"))
        })?;
        self.running.borrow_mut().insert(spec.name.clone());
        let _running = RunningGuard {
            set: &self.running,
            name: spec.name.clone(),
        };
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
}
