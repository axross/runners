//! where Marshal is, as one value the rest of the view reads: the agent's state from
//! the snapshot, the app's own connection, and an action not yet in the snapshot.

use marshal_cli::agent::StopReason;
use marshal_cli::agent::snapshot::{
    ImageInfo, PoolInfo, Reason, RunnerState, Snapshot, WarningKind,
};
use marshal_cli::agent::tokens::NO_TOKEN;

use crate::input::{Cause, Connection, Input, PendingKind};
use crate::text::plain;

/// why the agent is stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    ByYou,
    NoConfiguration,
    InvalidConfiguration,
    ImageUnusable,
    NoPoolStarted,
    RestartLimit,
    ImageBuildFailed,
    /// a code this reader does not know, or none.
    Other(String),
}

impl Stop {
    fn of(reason: Option<&Reason>) -> Self {
        let Some(reason) = reason else {
            return Self::Other(String::new());
        };
        match reason.code.as_str() {
            StopReason::BY_YOU => Self::ByYou,
            StopReason::NO_CONFIGURATION => Self::NoConfiguration,
            StopReason::INVALID_CONFIGURATION => Self::InvalidConfiguration,
            StopReason::IMAGE_UNUSABLE => Self::ImageUnusable,
            StopReason::NO_POOL_STARTED => Self::NoPoolStarted,
            StopReason::RESTART_LIMIT => Self::RestartLimit,
            StopReason::IMAGE_BUILD_FAILED => Self::ImageBuildFailed,
            other => Self::Other(other.to_string()),
        }
    }

    /// whether the stop means something went wrong; an unknown code is not taken for
    /// an error.
    pub fn is_error(&self) -> bool {
        !matches!(self, Self::ByYou | Self::Other(_))
    }

    /// whether the configuration cannot be used, so no image name is known.
    pub fn lacks_configuration(&self) -> bool {
        matches!(self, Self::NoConfiguration | Self::InvalidConfiguration)
    }
}

/// the states of the plan's availability table, with the app's own beside the agent's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resident {
    StartingMarshal,
    NoAgent,
    NotAnswering,
    LockHeld,
    Quitting,
    LostContact {
        last_heard_ms: u64,
        cause: Cause,
    },
    Running,
    Resuming,
    Stopping,
    Building {
        stopping: bool,
    },
    Stopped(Stop),
    /// an agent state this reader does not know.
    Unknown(String),
}

/// what the image check last found, and whether the image should be built again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageStatus {
    Missing,
    Mismatched,
    OutOfDate,
    Current,
    Unchecked,
}

const CONTRACT_LINE: &str = "image-contract";

impl ImageStatus {
    fn of(image: Option<&ImageInfo>) -> Self {
        let Some(image) = image else {
            return Self::Unchecked;
        };
        let contract = image
            .lines
            .iter()
            .find(|line| line.name == CONTRACT_LINE)
            .map_or("", |line| line.value.as_str());
        match contract {
            "missing" => Self::Missing,
            "not checked" | "" => Self::Unchecked,
            word if word.starts_with("mismatch") => Self::Mismatched,
            _ if image.known_warnings().any(|(kind, _)| {
                matches!(kind, WarningKind::ImageOld | WarningKind::PackagesOld)
            }) =>
            {
                Self::OutOfDate
            }
            _ => Self::Current,
        }
    }

    /// whether the image cannot run jobs.
    pub fn is_unusable(self) -> bool {
        matches!(self, Self::Missing | Self::Mismatched)
    }
}

/// the 401 GitHub answers a registration with when it does not accept the token.
pub const REJECTED_MARK: &str = "status code does not indicate success: 401";

/// whether a runner's reason for backing off is GitHub not accepting the token.
pub fn is_rejection(reason: &str) -> bool {
    reason.contains(REJECTED_MARK)
}

pub struct Context<'a> {
    pub input: &'a Input<'a>,
    pub snapshot: Option<&'a Snapshot>,
    pub resident: Resident,
    /// when the state began, in milliseconds since the Unix epoch.
    pub since_ms: u64,
    pub up: u32,
    pub wanted: u32,
    /// the count the icon, tooltip, menu and announcement show.
    pub up_settled: u32,
    pub configuration_path: String,
    pub image: ImageStatus,
    pub image_name: Option<String>,
    pub restart_due: Option<u64>,
    pub pools: &'a [PoolInfo],
}

impl<'a> Context<'a> {
    pub fn new(input: &'a Input<'a>) -> Self {
        let snapshot = input.snapshot;
        let mut resident = resident_of(input);
        let mut since_ms = snapshot.map_or(0, |snapshot| snapshot.agent.since);
        if let Some(pending) = input.pending {
            let next = match (pending.kind, &resident) {
                (PendingKind::StopAll, Resident::Running | Resident::Resuming) => {
                    Some(Resident::Stopping)
                }
                (PendingKind::StartAll, Resident::Stopped(_)) => Some(Resident::Resuming),
                (PendingKind::Build, Resident::Stopped(_)) => {
                    Some(Resident::Building { stopping: false })
                }
                (PendingKind::StopBuild, Resident::Building { .. }) => {
                    Some(Resident::Building { stopping: true })
                }
                _ => None,
            };
            if let Some(next) = next {
                resident = next;
                if pending.kind != PendingKind::StopBuild {
                    since_ms = pending.since_ms;
                }
            }
        }
        let up = snapshot.map_or(0, |snapshot| snapshot.runners_up);
        let image_info = snapshot.and_then(|snapshot| snapshot.images.first());
        Self {
            input,
            snapshot,
            resident,
            since_ms,
            up,
            wanted: snapshot.map_or(0, |snapshot| snapshot.runners_wanted),
            up_settled: input.settled_up.unwrap_or(up),
            configuration_path: snapshot
                .map(|snapshot| plain(&snapshot.configuration.path))
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| crate::text::DEFAULT_CONFIGURATION.to_string()),
            image: ImageStatus::of(image_info),
            image_name: image_info
                .map(|image| plain(&image.name))
                .filter(|name| !name.is_empty()),
            restart_due: snapshot.and_then(|snapshot| snapshot.agent.restart_due),
            pools: snapshot.map_or(&[], |snapshot| snapshot.pools.as_slice()),
        }
    }

    /// whether the agent runs the pools, the time the count is settled.
    pub fn agent_runs(&self) -> bool {
        matches!(self.resident, Resident::Running | Resident::Resuming)
    }

    /// whether the window shows a list of pools, as against the whole window saying why
    /// Marshal does not control the runners.
    pub fn has_list(&self) -> bool {
        self.snapshot.is_some()
            && !matches!(
                self.resident,
                Resident::StartingMarshal
                    | Resident::NoAgent
                    | Resident::NotAnswering
                    | Resident::LockHeld
            )
    }

    /// whether the list shown is the last one heard, not a live one.
    pub fn is_stale(&self) -> bool {
        matches!(self.resident, Resident::LostContact { .. })
    }

    /// whether every pool waits for Docker.
    pub fn waiting_for_docker(&self) -> bool {
        !self.pools.is_empty()
            && self
                .pools
                .iter()
                .all(|pool| pool.state == "waiting_for_docker")
    }

    /// whether the runners are listed and every one backs off.
    pub fn every_runner_backs_off(&self) -> bool {
        let mut runners = self.pools.iter().flat_map(|pool| &pool.runners).peekable();
        runners.peek().is_some() && runners.all(|runner| runner.kind() == RunnerState::BackingOff)
    }

    /// whether every pool is skipped for want of a token.
    pub fn every_pool_lacks_a_token(&self) -> bool {
        !self.pools.is_empty()
            && self
                .pools
                .iter()
                .all(|pool| pool.state != "running" && pool.reason.as_deref() == Some(NO_TOKEN))
    }
}

fn resident_of(input: &Input<'_>) -> Resident {
    match input.connection {
        Connection::StartingMarshal => Resident::StartingMarshal,
        Connection::NoAgent => Resident::NoAgent,
        Connection::NotAnswering => Resident::NotAnswering,
        Connection::LockHeld => Resident::LockHeld,
        Connection::Quitting => Resident::Quitting,
        Connection::LostContact {
            last_heard_ms,
            cause,
        } => Resident::LostContact {
            last_heard_ms,
            cause,
        },
        Connection::Connected => input
            .snapshot
            .map_or(Resident::StartingMarshal, agent_state),
    }
}

fn agent_state(snapshot: &Snapshot) -> Resident {
    match snapshot.agent.state.as_str() {
        "running" => Resident::Running,
        "resuming" => Resident::Resuming,
        "stopping" => Resident::Stopping,
        "building" => Resident::Building { stopping: false },
        "quitting" => Resident::Quitting,
        "stopped" => Resident::Stopped(Stop::of(snapshot.agent.reason.as_ref())),
        other => Resident::Unknown(plain(other)),
    }
}
