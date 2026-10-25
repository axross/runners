//! The seam between the runtime and whatever runs containers.
//!
//! The runtime asks a backend to wait until it is available, read the configured
//! image's labels, list and remove a pool's leftover containers, prepare volumes,
//! choose a slot's CPUs, run a container under the run contract and report its
//! exit, stop, kill or remove a set of containers within a budget, and finalise a
//! diagnostic container under a clock. The container backend runs `docker`; a
//! later backend can run the same containers inside a virtual machine.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

use crate::image::check::ImageCheck;
use crate::validate::PlannedVolume;

use super::retained::{Claim, Retained};
use super::secret::Secret;

/// Why a backend operation failed. The text never holds a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError(String);

impl BackendError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BackendError {}

/// A named volume mounted into a job container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub volume: String,
    pub target: String,
}

impl From<&PlannedVolume> for Mount {
    fn from(volume: &PlannedVolume) -> Self {
        Self {
            volume: volume.name.clone(),
            target: volume.mount_path.clone(),
        }
    }
}

/// What `run_container` needs under the run contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSpec {
    /// The container name, which is also the runner's name.
    pub name: String,
    /// The CPU quota as text, as `format_cpu_count` writes it.
    pub cpus: String,
    /// The CPU IDs the container may use, such as `2,5-6`.
    pub cpuset: String,
    pub memory_gb: u32,
    pub mounts: Vec<Mount>,
    pub image: String,
    /// The command the launcher runs after verifying the CPU set.
    pub run_command: String,
    /// Where a diagnostics-enabled pool's evidence goes; such a container runs the
    /// diagnostic variant and is retained until it is finalised.
    pub diagnostic: Option<DiagnosticRecord>,
}

/// The settings a diagnostic container ran with, which every finalisation of it
/// uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    /// The pool's name, which starts each of its bundles' names.
    pub pool: String,
    /// The sink, as the configuration names it.
    pub directory: String,
    pub raw_records: bool,
}

/// The time a diagnostic container's export has, and the time its whole
/// finalisation has, its removal included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadlines {
    pub export: Instant,
    pub removal: Instant,
}

/// From a finalisation's clock start to the end of its export.
pub const EXPORT_WITHIN: Duration = Duration::from_secs(18);

/// From a finalisation's clock start to the end of its removal.
pub const REMOVAL_WITHIN: Duration = Duration::from_secs(24);

impl Deadlines {
    /// The deadlines of a clock that started at `start`.
    #[must_use]
    pub fn from(start: Instant) -> Self {
        Self {
            export: start + EXPORT_WITHIN,
            removal: start + REMOVAL_WITHIN,
        }
    }
}

/// How a diagnostic container's export went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportOutcome {
    /// The bundle is complete.
    Complete,
    /// The bundle is complete, but its final observation is missing.
    Gap,
    /// The sink already holds ten of the pool's bundles.
    SinkFull,
    /// The export failed, was unsafe, or ran out of time.
    Failed,
    /// The clock left no time to begin it.
    Skipped,
    /// An earlier finalisation's bundle stands, so none was made.
    AlreadyExported,
}

/// What finalising a diagnostic container did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finalized {
    pub export: ExportOutcome,
    /// The container is gone, and no longer retained.
    pub removed: bool,
    /// A forced stop ended the finalisation before its removal, and the container is
    /// kept.
    pub kept: bool,
    /// The pool's bundles in the sink after the export, as the exporter counted
    /// them.
    pub bundles: Option<u32>,
}

/// A request for the CPUs of one slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuRequest {
    /// The name of the short container that reads the CPUs it may use.
    pub probe_name: String,
    pub image: String,
    /// How many CPU IDs the slot takes.
    pub count: u64,
    /// Where the slot starts among the allowed CPUs, which wraps.
    pub offset: u64,
}

/// A pool's containers a name filter found, and which of them carry the
/// diagnostics label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolContainers {
    pub names: Vec<String>,
    pub diagnostic: Vec<String>,
}

/// A request to stop containers: each gets `grace` to end before it is killed, and
/// the whole request is abandoned at `budget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopRequest {
    pub names: Vec<String>,
    pub grace: Duration,
    pub budget: Duration,
}

pub trait Backend {
    /// Completes once the backend can take commands, retrying until then.
    fn wait_until_available(&self) -> impl Future<Output = ()>;

    /// Reads the labels of the image `image` names, as `docker run` resolves the
    /// name, and sorts them into the four outcomes, within the image-check bound.
    /// Anything that is not a clear answer is [`ImageCheck::Unavailable`], never a
    /// mismatch.
    fn check_image(&self, image: &str) -> impl Future<Output = ImageCheck>;

    /// Lists the containers whose names contain `pool` and a hyphen, as Docker's
    /// name filter matches them anywhere in a name, and those of them that carry
    /// the diagnostics label. A caller keeps only the names `is_own_container_name`
    /// accepts.
    fn list_pool_containers(
        &self,
        pool: &str,
    ) -> impl Future<Output = Result<PoolContainers, BackendError>>;

    /// Force-removes the containers, within `budget`.
    fn remove_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// Creates the volumes and gives the runner user ownership of their mount
    /// points.
    fn prepare_volumes(
        &self,
        image: &str,
        volumes: &[PlannedVolume],
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// Chooses the slot's CPU set, as `2,5-6`. It never returns an unrestricted
    /// set: any doubt is an error.
    fn select_cpuset(
        &self,
        request: &CpuRequest,
    ) -> impl Future<Output = Result<String, BackendError>>;

    /// Runs one job container to its end and returns its exit code. The JIT
    /// configuration reaches the container through the environment of the one
    /// client that starts it, never through an argument.
    fn run_container(
        &self,
        spec: &ContainerSpec,
        jit: &Secret,
    ) -> impl Future<Output = Result<i32, BackendError>>;

    /// The containers this backend has started and not seen end. Dropping a
    /// `run_container` future removes its container from this list, though the
    /// container itself may live on in the engine, so a stop sequence reads this
    /// list before it cancels the workers that hold those futures.
    fn running_containers(&self) -> Vec<String>;

    /// Stops the containers, in one call to the container engine.
    fn stop_containers(
        &self,
        request: &StopRequest,
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// Ends the containers at once and keeps them, in one call, within `budget`.
    fn kill_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// The diagnostic containers this backend retains. A `run_container` of a
    /// diagnostic container retains it; a successful removal by a finalisation
    /// forgets it, and nothing else does, a panic included.
    fn retained(&self) -> &Retained;

    /// Finalises a claimed diagnostic container with its record: exports it, unless
    /// a bundle was already admitted for it, by `deadlines.export`, then removes it
    /// whatever the export did, by `deadlines.removal`. A forced stop, asked for
    /// through [`Retained::force`], ends the export and keeps the container.
    fn finalize_diagnostic(
        &self,
        claim: &Claim<'_>,
        deadlines: Deadlines,
    ) -> impl Future<Output = Finalized>;

    /// Counts a pool's bundles in its sink, by `deadline`, or `None` when the sink
    /// cannot be read.
    fn count_bundles(
        &self,
        record: &DiagnosticRecord,
        deadline: Instant,
    ) -> impl Future<Output = Option<u32>>;
}
