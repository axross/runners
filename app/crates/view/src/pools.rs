//! each pool as a card: its state, its runners, its diagnostics, and what its
//! repository's token needs.

use marshal_cli::agent::snapshot::{PoolInfo, RunnerInfo, RunnerState};
use serde::Serialize;

use crate::control::Control;
use crate::diagnostics::{Diagnostics, section};
use crate::state::{Context, Resident};
use crate::text::{humanised, plain};
use crate::tokens::{Repository, pool_note, repositories};

/// the mark that goes with a state's words; the state is always written out too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    /// up: a check.
    Check,
    /// registering: a ring.
    Ring,
    /// backing off, or held: the history mark, as both try again later.
    History,
    Stop,
    /// a skipped pool's flag.
    Flag,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerRow {
    pub slot: u32,
    /// "Runner 1".
    pub name: String,
    /// "Container running", "Registering", "Backing off", "Held", or for a state this
    /// reader does not know, the runner's own reason.
    pub state: String,
    /// removed while the list is the last one heard.
    pub mark: Option<Mark>,
    /// the container's name, while there is one.
    pub container: Option<String>,
    /// "since 03:15".
    pub since: Option<String>,
    /// why a runner backs off or is held, in the agent's words.
    pub detail: Option<String>,
    /// what Narrator reads: the pool, the runner and its state.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PoolCard {
    pub name: String,
    pub repository: String,
    /// "Running", "Skipped", "Waiting for Docker".
    pub state: String,
    pub mark: Option<Mark>,
    /// "2 of 2 up".
    pub count: String,
    /// "diagnostics on".
    pub tags: Vec<String>,
    /// "from 03:52", while the list is the last one heard.
    pub from: Option<String>,
    /// the whole line as the boards write it: "Running · 1 of 2 up · diagnostics on".
    pub line: String,
    /// why the pool is skipped, or what its repository's token needs.
    pub note: Option<String>,
    /// Set Token or Replace Token, which opens the Tokens tab on the repository's row.
    pub pill: Option<Control>,
    pub runners: Vec<RunnerRow>,
    pub diagnostics: Option<Diagnostics>,
}

fn pool_state(pool: &PoolInfo) -> (String, Option<Mark>) {
    let every_runner_backs_off = !pool.runners.is_empty()
        && pool
            .runners
            .iter()
            .all(|runner| runner.kind() == RunnerState::BackingOff);
    match pool.state.as_str() {
        "waiting_for_docker" => ("Waiting for Docker".to_string(), None),
        "preparing" => ("Starting".to_string(), None),
        "running" if every_runner_backs_off => ("Backing off".to_string(), Some(Mark::History)),
        "running" => ("Running".to_string(), None),
        "skipped" => ("Skipped".to_string(), Some(Mark::Flag)),
        // a pool that was skipped keeps its reason while it is listed stopped
        "stopped" if pool.reason.is_some() => ("Skipped".to_string(), Some(Mark::Flag)),
        "stopped" => ("Stopped".to_string(), None),
        other => (humanised(&plain(other)), None),
    }
}

fn runner_row(context: &Context<'_>, pool: &PoolInfo, runner: &RunnerInfo) -> RunnerRow {
    let saving = runner.workload.as_deref().is_some_and(|name| {
        pool.diagnostics.as_ref().is_some_and(|info| {
            info.containers
                .iter()
                .any(|container| container.name == name && container.state == "finalising")
        })
    });
    let (state, mark) = match runner.kind() {
        RunnerState::Running if saving => ("Saving diagnostics".to_string(), Mark::Check),
        RunnerState::Running => ("Container running".to_string(), Mark::Check),
        RunnerState::Registering => ("Registering".to_string(), Mark::Ring),
        RunnerState::BackingOff => ("Backing off".to_string(), Mark::History),
        RunnerState::Held => ("Held".to_string(), Mark::History),
        RunnerState::Stopped => ("Stopped".to_string(), Mark::Stop),
        RunnerState::Unknown(word) => (
            runner
                .unknown_state_text()
                .map_or_else(|| humanised(&plain(&word)), plain),
            Mark::Unknown,
        ),
        _ => (humanised(&plain(&runner.state)), Mark::Unknown),
    };
    let known = !matches!(runner.kind(), RunnerState::Unknown(_));
    let name = format!("Runner {}", runner.slot);
    let lowered = state.chars().next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(state.chars().skip(1)).collect()
    });
    RunnerRow {
        slot: runner.slot,
        label: format!("{}, {name}, {lowered}", plain(&pool.name)),
        name,
        mark: (!context.is_stale()).then_some(mark),
        container: runner.workload.as_deref().map(plain),
        since: (runner.kind() != RunnerState::Stopped)
            .then(|| format!("since {}", context.input.clock.when(runner.since))),
        detail: known.then(|| runner.reason.as_deref().map(plain)).flatten(),
        state,
    }
}

fn card(context: &Context<'_>, repositories: &[Repository], pool: &PoolInfo) -> PoolCard {
    let (state, mark) = pool_state(pool);
    let diagnostics = section(pool);
    let count = format!("{} of {} up", pool.runners_up, pool.slots);
    let mut tags = Vec::new();
    if diagnostics.as_ref().is_some_and(|section| section.on) {
        tags.push("diagnostics on".to_string());
    }
    let from = match context.resident {
        Resident::LostContact { last_heard_ms, .. } => {
            Some(format!("from {}", context.input.clock.when(last_heard_ms)))
        }
        _ => None,
    };
    let mut line = vec![state.clone(), count.clone()];
    line.extend(tags.iter().cloned());
    line.extend(from.iter().cloned());
    let (note, pill) = match pool_note(context, repositories, pool) {
        Some((text, pill)) => (Some(text), Some(pill)),
        None => (pool.reason.as_deref().map(plain), None),
    };
    let every_runner_stopped = pool
        .runners
        .iter()
        .all(|runner| runner.kind() == RunnerState::Stopped);
    PoolCard {
        name: plain(&pool.name),
        repository: plain(&pool.repository),
        state,
        mark: (!context.is_stale()).then_some(mark).flatten(),
        count,
        tags,
        from,
        line: line.join(" · "),
        note,
        pill,
        runners: if every_runner_stopped {
            Vec::new()
        } else {
            pool.runners
                .iter()
                .map(|runner| runner_row(context, pool, runner))
                .collect()
        },
        diagnostics,
    }
}

/// every pool of the snapshot, in the agent's order.
pub fn cards(context: &Context<'_>) -> Vec<PoolCard> {
    let repositories = repositories(context);
    context
        .pools
        .iter()
        .map(|pool| card(context, &repositories, pool))
        .collect()
}
