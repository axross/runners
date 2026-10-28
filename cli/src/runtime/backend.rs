//! the seam between the runtime and whatever runs containers; the container backend
//! runs `docker`, and a later one can run the same containers in a virtual machine.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

use crate::image::check::ImageCheck;
use crate::validate::PlannedVolume;

use super::retained::Retained;
use super::secret::Secret;

/// why a backend operation failed; the text never holds a secret.
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

/// a named volume mounted into a job container.
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

/// what `run_container` needs under the run contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSpec {
    /// the container name, which is also the runner's name.
    pub name: String,
    /// the CPU quota as text, as `format_cpu_count` writes it.
    pub cpus: String,
    /// the CPU IDs the container may use, such as `2,5-6`.
    pub cpuset: String,
    pub memory_gb: u32,
    pub mounts: Vec<Mount>,
    pub image: String,
    /// the command the launcher runs after verifying the CPU set.
    pub run_command: String,
    /// where a diagnostics-enabled pool's evidence goes; the container runs the
    /// diagnostic variant.
    pub diagnostic: Option<DiagnosticRecord>,
}

/// the settings a diagnostic container ran with, which every finalisation of it
/// uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    /// the pool's name, which starts each of its bundles' names.
    pub pool: String,
    /// the sink, as the configuration names it.
    pub directory: String,
    pub raw_records: bool,
}

/// the export deadline and the whole finalisation's deadline, removal included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadlines {
    pub export: Instant,
    pub removal: Instant,
}

/// from a finalisation's clock start to the end of its export.
pub const EXPORT_WITHIN: Duration = Duration::from_secs(18);

/// from a finalisation's clock start to the end of its removal.
pub const REMOVAL_WITHIN: Duration = Duration::from_secs(24);

impl Deadlines {
    /// the deadlines of a clock that started at `start`.
    #[must_use]
    pub fn from(start: Instant) -> Self {
        Self {
            export: start + EXPORT_WITHIN,
            removal: start + REMOVAL_WITHIN,
        }
    }
}

/// how a diagnostic container's export went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportOutcome {
    /// the bundle is complete.
    Complete,
    /// the bundle is complete, but its final observation is missing.
    Gap,
    /// the sink already holds ten of the pool's bundles.
    SinkFull,
    /// the export failed, was unsafe, or ran out of time.
    Failed,
    /// the clock left no time to begin it.
    Skipped,
    /// an earlier finalisation's bundle stands, so none was made.
    AlreadyExported,
}

/// what finalising a diagnostic container did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finalized {
    pub export: ExportOutcome,
    /// the container is gone, and no longer retained.
    pub removed: bool,
    /// a forced stop ended the finalisation before its removal, and the container is
    /// kept.
    pub kept: bool,
    /// the pool's bundles in the sink after the export, as the exporter counted
    /// them.
    pub bundles: Option<u32>,
}

/// what one export of a diagnostic container did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exported {
    pub outcome: ExportOutcome,
    /// a bundle was created for the container, so no later finalisation exports
    /// it again.
    pub admitted: bool,
    /// the pool's bundles in the sink, as the export counted them.
    pub bundles: Option<u32>,
}

impl Exported {
    pub const FAILED: Self = Self {
        outcome: ExportOutcome::Failed,
        admitted: false,
        bundles: None,
    };
}

/// a request for the CPUs of one slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuRequest {
    /// the name of the short container that reads the CPUs it may use.
    pub probe_name: String,
    pub image: String,
    /// how many CPU IDs the slot takes.
    pub count: u64,
    /// where the slot starts among the allowed CPUs, which wraps.
    pub offset: u64,
}

/// a pool's containers a name filter found, and which of them carry the
/// diagnostics label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolContainers {
    pub names: Vec<String>,
    pub diagnostic: Vec<String>,
}

/// a request to stop containers: each gets `grace` to end before it is killed, and
/// the whole request is abandoned at `budget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopRequest {
    pub names: Vec<String>,
    pub grace: Duration,
    pub budget: Duration,
}

pub trait Backend {
    /// completes once the backend can take commands, retrying until then.
    fn wait_until_available(&self) -> impl Future<Output = ()>;

    /// classifies the image `image` names, as `docker run` resolves it, within the
    /// image-check bound; anything not a clear answer is [`ImageCheck::Unavailable`],
    /// never a mismatch.
    fn check_image(&self, image: &str) -> impl Future<Output = ImageCheck>;

    /// lists the containers whose names contain `pool` and a hyphen, which Docker's
    /// filter matches anywhere in a name, and those carrying the diagnostics label;
    /// callers keep only the names `is_own_container_name` accepts.
    fn list_pool_containers(
        &self,
        pool: &str,
    ) -> impl Future<Output = Result<PoolContainers, BackendError>>;

    /// force-removes the containers, within `budget`.
    fn remove_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// creates the volumes and gives the runner user ownership of their mount
    /// points.
    fn prepare_volumes(
        &self,
        image: &str,
        volumes: &[PlannedVolume],
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// chooses the slot's CPU set, as `2,5-6`; any doubt is an error, never an
    /// unrestricted set.
    fn select_cpuset(
        &self,
        request: &CpuRequest,
    ) -> impl Future<Output = Result<String, BackendError>>;

    /// runs one job container to its end and returns its exit code; the JIT
    /// configuration reaches it through the environment of the one client that
    /// starts it, never through an argument.
    fn run_container(
        &self,
        spec: &ContainerSpec,
        jit: &Secret,
    ) -> impl Future<Output = Result<i32, BackendError>>;

    /// the containers this backend has started and not seen end; dropping a
    /// `run_container` future removes its container from the list though it may live
    /// on, so a stop sequence reads it before cancelling the workers.
    fn running_containers(&self) -> Vec<String>;

    /// stops the containers, in one call to the container engine.
    fn stop_containers(
        &self,
        request: &StopRequest,
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// ends the containers at once and keeps them, in one call, within `budget`.
    fn kill_containers(
        &self,
        names: &[String],
        budget: Duration,
    ) -> impl Future<Output = Result<(), BackendError>>;

    /// the diagnostic containers this backend retains; only a successful removal by a
    /// finalisation forgets one, a panic included.
    fn retained(&self) -> &Retained;

    /// exports a diagnostic container into its record's sink by `deadline`; the
    /// finalisation removes it.
    fn export_diagnostic(
        &self,
        name: &str,
        record: &DiagnosticRecord,
        deadline: Instant,
    ) -> impl Future<Output = Exported>;

    /// counts a pool's bundles in its sink, by `deadline`, or `None` when the sink
    /// cannot be read.
    fn count_bundles(
        &self,
        record: &DiagnosticRecord,
        deadline: Instant,
    ) -> impl Future<Output = Option<u32>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_gives_the_export_eighteen_seconds_and_the_removal_twenty_four() {
        let start = Instant::now();
        assert_eq!(
            Deadlines::from(start),
            Deadlines {
                export: start + Duration::from_secs(18),
                removal: start + Duration::from_secs(24),
            }
        );
    }
}
