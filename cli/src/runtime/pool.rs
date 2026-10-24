//! A pool of one repository: what it runs and how it is made ready.
//!
//! Preparation follows the PowerShell supervisor: a pool with a missing or blank
//! token file is skipped, Docker is waited for, and then each pool's stale
//! containers are removed and its volumes prepared. A pool that cannot be prepared
//! is skipped with a warning naming it, so it does not take the others down.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::affinity::{affinity_count, affinity_offset, slot_offset};
use crate::profile::Profile;
use crate::validate::{Entry, Plan, PlannedVolume, format_cpu_count};

use super::backend::{Backend, Mount};
use super::log::Log;
use super::name::{cpu_probe_name, is_own_container_name};
use super::token::read_token;

/// The bound on removing one stale container. The PowerShell supervisor had none.
const STALE_REMOVE_BUDGET: Duration = Duration::from_secs(120);

/// One repository's pool, from a valid axpc plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolSpec {
    /// Where the entry sits in the configuration file, such as `repositories[0]`.
    pub path: String,
    /// The entry's name: the start of every container and runner name.
    pub name: String,
    pub owner: String,
    pub repository: String,
    pub slots: u32,
    pub token_path: PathBuf,
    pub labels: Vec<String>,
    /// The CPU quota as text.
    pub cpus: String,
    /// How many CPU IDs each slot takes.
    pub cpu_count: u64,
    /// Where the pool's first slot starts among the affinity positions.
    pub cpu_offset: u64,
    pub memory_gb: u32,
    pub volumes: Vec<PlannedVolume>,
    pub diagnostics: bool,
    pub image: String,
    slot_offsets: Vec<u64>,
}

/// One slot of a pool, counting from 1.
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
}

/// Why a plan cannot become pools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError(&'static str);

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PlanError {}

/// Turns a valid axpc plan into pools, in the plan's order.
///
/// # Errors
///
/// The container backend runs the axpc profile only.
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
        diagnostics: entry.diagnostics,
        image: image.to_string(),
        slot_offsets: (1..=entry.slots)
            .map(|slot| slot_offset(&plan.entries, index, slot))
            .collect(),
    }
}

impl PoolSpec {
    /// The pool and its repository as a warning names them.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{} ({}/{})", self.path, self.owner, self.repository)
    }

    /// The pool's slots, numbered from 1.
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
            })
            .collect()
    }
}

impl SlotSpec {
    /// The name of the container that reads this slot's allowed CPUs.
    #[must_use]
    pub fn probe_name(&self) -> String {
        cpu_probe_name(&self.pool, self.number)
    }

    /// How the slot is named in a log line.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} slot {}", self.pool, self.number)
    }
}

/// No pool could be started, so there is nothing to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoPoolStartable;

impl fmt::Display for NoPoolStartable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("No repository could be started, so there is nothing to run.")
    }
}

impl std::error::Error for NoPoolStartable {}

fn warn_skipped(log: &dyn Log, pool: &PoolSpec, reason: &str) {
    log.warn(&format!(
        "{} is not started and gets no slots: {reason}",
        pool.describe()
    ));
}

/// Prepares the pools and returns those that can run, in order.
///
/// A pool is skipped, with a warning that names it and the reason, when its token
/// file is missing or blank, when its diagnostics are on (they come in a later
/// phase), when its stale containers cannot be listed or a labelled one remains,
/// or when its volumes cannot be prepared. Docker is waited for once, before the
/// first pool that needs it.
///
/// # Errors
///
/// Fails when no pool is left.
pub async fn prepare_pools<B: Backend>(
    backend: &B,
    pools: Vec<PoolSpec>,
    log: &dyn Log,
) -> Result<Vec<PoolSpec>, NoPoolStartable> {
    let mut startable = Vec::new();
    for pool in pools {
        if let Err(error) = read_token(&pool.token_path) {
            warn_skipped(log, &pool, &format!("{}: {error}.", pool.path));
        } else if pool.diagnostics {
            warn_skipped(
                log,
                &pool,
                "diagnostics are not supported by this runtime yet, so the pool is skipped.",
            );
        } else {
            startable.push(pool);
        }
    }
    if !startable.is_empty() {
        backend.wait_until_available().await;
    }
    let mut prepared = Vec::new();
    for pool in startable {
        let outcome = match clear_stale_containers(backend, &pool, log).await {
            Ok(()) => backend
                .prepare_volumes(&pool.image, &pool.volumes)
                .await
                .map_err(|error| error.to_string()),
            Err(reason) => Err(reason),
        };
        match outcome {
            Ok(()) => prepared.push(pool),
            Err(reason) => warn_skipped(log, &pool, &reason),
        }
    }
    if prepared.is_empty() {
        return Err(NoPoolStartable);
    }
    Ok(prepared)
}

/// Removes the pool's own leftover containers: those named like its slots' and not
/// labelled as diagnostics evidence. A labelled one is never removed; with
/// diagnostics off, one that remains refuses the pool.
async fn clear_stale_containers<B: Backend>(
    backend: &B,
    pool: &PoolSpec,
    log: &dyn Log,
) -> Result<(), String> {
    let listing = backend
        .list_pool_containers(&pool.name)
        .await
        .map_err(|error| error.to_string())?;
    let mut retained = false;
    for name in listing
        .names
        .iter()
        .filter(|name| is_own_container_name(&pool.name, name))
    {
        if listing.diagnostic.contains(name) {
            log.warn(&format!(
                "Diagnostic evidence retained after disabling; deletion refused for '{name}'. \
                 Explicit private recovery and cleanup are required before this entry can start."
            ));
            retained = true;
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
    if retained {
        return Err(
            "Marked diagnostic containers remain after disabling; entry startup refused."
                .to_string(),
        );
    }
    Ok(())
}
