//! a pool of one repository: what it runs and how it is made ready, following the
//! PowerShell supervisor; `docs/operations/cli.md` describes the preparation.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use tokio::time::Instant;

use crate::affinity::{affinity_count, affinity_offset, slot_offset};
use crate::profile::Profile;
use crate::validate::{Entry, Plan, PlannedVolume, format_cpu_count};

use super::backend::{Backend, DiagnosticRecord, Mount, StopRequest};
use super::board::{Board, PoolState};
use super::diagnostic::{STOP_GRACE, STOP_WITHIN, finalize_claimed, gap_text};
use super::log::Log;
use super::name::{cpu_probe_name, is_own_container_name};
use super::token::read_token;

const STALE_REMOVE_BUDGET: Duration = Duration::from_secs(120);

/// one repository's pool, from a valid axpc plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolSpec {
    /// where the entry sits in the configuration file, such as `repositories[0]`.
    pub path: String,
    /// the entry's name: the start of every container and runner name.
    pub name: String,
    pub owner: String,
    pub repository: String,
    pub slots: u32,
    pub token_path: PathBuf,
    pub labels: Vec<String>,
    /// the CPU quota as text.
    pub cpus: String,
    /// how many CPU IDs each slot takes.
    pub cpu_count: u64,
    /// where the pool's first slot starts among the affinity positions.
    pub cpu_offset: u64,
    pub memory_gb: u32,
    pub volumes: Vec<PlannedVolume>,
    /// where the pool's evidence goes, when its diagnostics are on.
    pub diagnostic: Option<DiagnosticRecord>,
    pub image: String,
    slot_offsets: Vec<u64>,
}

/// one slot of a pool, counting from 1.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotSpec {
    pub pool: String,
    pub owner: String,
    pub repository: String,
    pub number: u32,
    pub token_path: PathBuf,
    pub labels: Vec<String>,
    pub cpus: String,
    pub cpu_count: u64,
    pub cpu_offset: u64,
    pub memory_gb: u32,
    pub mounts: Vec<Mount>,
    pub image: String,
    pub diagnostic: Option<DiagnosticRecord>,
}

/// why a plan cannot become pools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError(&'static str);

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PlanError {}

/// turns a valid axpc plan into pools, in the plan's order, and fails for any other
/// profile.
pub fn pools_from_plan(plan: &Plan) -> Result<Vec<PoolSpec>, PlanError> {
    let (Profile::Axpc, Some(image)) = (plan.profile, plan.image_name.as_ref()) else {
        return Err(PlanError(
            "the container backend runs the axpc profile only",
        ));
    };
    Ok(plan
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| pool_from_entry(plan, index, entry, image))
        .collect())
}

/// the settings of a diagnostics-enabled entry's evidence.
fn diagnostic_record(entry: &Entry) -> Option<DiagnosticRecord> {
    let directory = entry
        .diagnostic_directory
        .as_ref()
        .filter(|_| entry.diagnostics)?;
    Some(DiagnosticRecord {
        pool: entry.name.clone(),
        directory: directory.clone(),
        raw_records: entry.diagnostic_raw_records,
    })
}

/// the evidence settings of every diagnostics-enabled pool of `plan`, in order.
#[must_use]
pub fn diagnostic_records(plan: &Plan) -> Vec<DiagnosticRecord> {
    plan.entries.iter().filter_map(diagnostic_record).collect()
}

fn pool_from_entry(plan: &Plan, index: usize, entry: &Entry, image: &str) -> PoolSpec {
    PoolSpec {
        path: entry.path.clone(),
        name: entry.name.clone(),
        owner: entry.owner.clone(),
        repository: entry.repository.clone(),
        slots: entry.slots,
        token_path: PathBuf::from(&entry.token_path),
        labels: entry.labels.clone(),
        cpus: format_cpu_count(entry.cpus),
        cpu_count: affinity_count(entry),
        cpu_offset: affinity_offset(&plan.entries, index),
        memory_gb: entry.memory_gb,
        volumes: entry.volumes.clone(),
        diagnostic: diagnostic_record(entry),
        image: image.to_string(),
        slot_offsets: (1..=entry.slots)
            .map(|slot| slot_offset(&plan.entries, index, slot))
            .collect(),
    }
}

impl PoolSpec {
    /// the pool and its repository as a warning names them.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{} ({}/{})", self.path, self.owner, self.repository)
    }

    /// the pool's slots, numbered from 1.
    #[must_use]
    pub fn slot_specs(&self) -> Vec<SlotSpec> {
        (1..=self.slots)
            .zip(&self.slot_offsets)
            .map(|(number, offset)| SlotSpec {
                pool: self.name.clone(),
                owner: self.owner.clone(),
                repository: self.repository.clone(),
                number,
                token_path: self.token_path.clone(),
                labels: self.labels.clone(),
                cpus: self.cpus.clone(),
                cpu_count: self.cpu_count,
                cpu_offset: *offset,
                memory_gb: self.memory_gb,
                mounts: self.volumes.iter().map(Mount::from).collect(),
                image: self.image.clone(),
                diagnostic: self.diagnostic.clone(),
            })
            .collect()
    }
}

impl SlotSpec {
    /// the name of the container that reads this slot's allowed CPUs.
    #[must_use]
    pub fn probe_name(&self) -> String {
        cpu_probe_name(&self.pool, self.number)
    }

    /// how the slot is named in a log line.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} slot {}", self.pool, self.number)
    }
}

/// no pool could be started, so there is nothing to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoPoolStartable;

impl fmt::Display for NoPoolStartable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("No repository could be started, so there is nothing to run.")
    }
}

impl std::error::Error for NoPoolStartable {}

fn skip(log: &dyn Log, board: &Board, pool: &PoolSpec, reason: &str) {
    log.warn(&format!(
        "{} is not started and gets no slots: {reason}",
        pool.describe()
    ));
    board.set_pool(&pool.name, PoolState::Skipped(reason.to_string()));
}

/// the first step of preparing the pools, needing no Docker: returns the pools
/// whose token file reads, skipping the others with a warning; every token read is
/// masked in `log`.
#[must_use]
pub fn admit_pools(pools: Vec<PoolSpec>, log: &dyn Log, board: &Board) -> Vec<PoolSpec> {
    let mut admitted = Vec::new();
    for pool in pools {
        match read_token(&pool.token_path) {
            Err(error) => skip(log, board, &pool, &format!("{}: {error}.", pool.path)),
            Ok(token) => {
                log.mask(&token);
                board.set_pool(&pool.name, PoolState::WaitingForDocker);
                admitted.push(pool);
            }
        }
    }
    admitted
}

/// the second step, once Docker answers: removes each admitted pool's stale
/// containers, prepares its volumes, and returns the pools that can run; a pool whose
/// stale containers cannot be listed, whose labelled one remains, or whose volumes
/// cannot be prepared is skipped with a warning, and it fails when none is left.
pub async fn prepare_admitted<B: Backend>(
    backend: &B,
    admitted: Vec<PoolSpec>,
    log: &dyn Log,
    board: &Board,
) -> Result<Vec<PoolSpec>, NoPoolStartable> {
    let mut prepared = Vec::new();
    for pool in admitted {
        board.set_pool(&pool.name, PoolState::Preparing);
        let outcome = match clear_stale_containers(backend, &pool, log).await {
            Ok(()) => backend
                .prepare_volumes(&pool.image, &pool.volumes)
                .await
                .map_err(|error| error.to_string()),
            Err(reason) => Err(reason),
        };
        match outcome {
            Ok(()) => {
                board.set_pool(&pool.name, PoolState::Running);
                prepared.push(pool);
            }
            Err(reason) => skip(log, board, &pool, &reason),
        }
    }
    if prepared.is_empty() {
        return Err(NoPoolStartable);
    }
    Ok(prepared)
}

/// removes the pool's own leftover containers; a labelled one the backend already
/// retains keeps its record and is left alone, and any other is never removed
/// without its export, so it is finalised, or with diagnostics off stopped and the
/// pool refused.
async fn clear_stale_containers<B: Backend>(
    backend: &B,
    pool: &PoolSpec,
    log: &dyn Log,
) -> Result<(), String> {
    let listing = backend
        .list_pool_containers(&pool.name)
        .await
        .map_err(|error| error.to_string())?;
    let mut left = Vec::new();
    for name in listing
        .names
        .iter()
        .filter(|name| is_own_container_name(&pool.name, name))
    {
        if listing.diagnostic.contains(name) {
            if !backend.retained().contains(name) {
                left.push(name.clone());
            }
            continue;
        }
        log.warn(&format!(
            "Removing stale container '{name}' left over from an earlier run."
        ));
        if let Err(error) = backend
            .remove_containers(std::slice::from_ref(name), STALE_REMOVE_BUDGET)
            .await
        {
            log.warn(&format!(
                "Failed to remove stale container '{name}': {error}"
            ));
        }
    }
    match &pool.diagnostic {
        Some(record) => {
            finalize_left(backend, log, record, &left).await;
            backend.retained().set_unmanaged(&pool.name, Vec::new());
            Ok(())
        }
        None => refuse_left(backend, log, pool, left).await,
    }
}

/// retains the labelled containers an earlier process left with the pool's current
/// settings, and finalises them on one clock, reporting each one.
async fn finalize_left<B: Backend>(
    backend: &B,
    log: &dyn Log,
    record: &DiagnosticRecord,
    left: &[String],
) {
    if left.is_empty() {
        return;
    }
    let retained = backend.retained();
    for name in left {
        retained.adopt(name, record);
    }
    let claims = left
        .iter()
        .filter_map(|name| retained.claim(name))
        .collect();
    for acted in finalize_claimed(backend, log, claims, Instant::now()).await {
        let removed = if acted.finalized.removed {
            "removed"
        } else {
            "not removed"
        };
        // the gap itself is already warned by the finalisation
        let export = if gap_text(acted.finalized.export).is_none() {
            "exported"
        } else {
            "export gap"
        };
        log.info(&format!(
            "Diagnostic container {} that an earlier run left: {export}, {removed}.",
            acted.name
        ));
    }
}

/// gives the labelled containers of a pool with diagnostics off a bounded stop and
/// refuses the pool, leaving them to the manual recovery.
async fn refuse_left<B: Backend>(
    backend: &B,
    log: &dyn Log,
    pool: &PoolSpec,
    left: Vec<String>,
) -> Result<(), String> {
    backend.retained().set_unmanaged(&pool.name, left.clone());
    if left.is_empty() {
        return Ok(());
    }
    let stop = StopRequest {
        names: left.clone(),
        grace: STOP_GRACE,
        budget: STOP_WITHIN,
    };
    if let Err(error) = backend.stop_containers(&stop).await {
        log.warn(&format!(
            "Stopping the labelled containers of {} failed: {error}",
            pool.describe()
        ));
    }
    for name in &left {
        log.warn(&format!(
            "Diagnostic evidence retained after disabling; deletion refused for '{name}'. \
             Explicit private recovery and cleanup are required before this entry can start."
        ));
    }
    Err(
        "Marked diagnostic containers remain after disabling; entry startup refused. \
         Recover them by hand as \"Recover before disabling diagnostics\" in the Windows \
         Runner Host document describes."
            .to_string(),
    )
}
