//! The seam between the runtime and whatever runs containers.
//!
//! The runtime asks a backend to wait until it is available, list and remove a
//! pool's leftover containers, prepare volumes, choose a slot's CPUs, run a
//! container under the run contract and report its exit, and stop or remove a set
//! of containers within a budget. The container backend runs `docker`; a later
//! backend can run the same containers inside a virtual machine.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use crate::validate::PlannedVolume;

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

    /// Lists the containers whose names begin with `pool` and a hyphen, and those
    /// of them that carry the diagnostics label.
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

    /// The containers this backend has started and not seen end.
    fn running_containers(&self) -> Vec<String>;

    /// Stops the containers, in one call to the container engine.
    fn stop_containers(
        &self,
        request: &StopRequest,
    ) -> impl Future<Output = Result<(), BackendError>>;
}
