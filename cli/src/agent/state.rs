//! where the agent is and what it knows about its configuration, for the snapshot.

use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::host_config::{Loaded, Migration, MigrationFailure};
use crate::image::check::ImageCheck;
use crate::validate::Plan;

use super::api::Method;
use super::snapshot::{BuildInfo, Reason as WireReason};

/// why the agent is stopped; the codes are an open set on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub code: &'static str,
    pub message: String,
}

impl Reason {
    pub const BY_YOU: &'static str = "by_you";
    pub const NO_CONFIGURATION: &'static str = "no_configuration";
    pub const INVALID_CONFIGURATION: &'static str = "invalid_configuration";
    pub const IMAGE_UNUSABLE: &'static str = "image_unusable";
    pub const NO_POOL_STARTED: &'static str = "no_pool_started";
    pub const RESTART_LIMIT: &'static str = "restart_limit";
    pub const IMAGE_BUILD_FAILED: &'static str = "image_build_failed";

    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn by_you() -> Self {
        Self::new(Self::BY_YOU, "stopped by you")
    }

    pub(crate) fn wire(&self) -> WireReason {
        WireReason {
            code: self.code.to_string(),
            message: self.message.clone(),
        }
    }
}

/// where the agent is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Running,
    Stopping,
    Resuming,
    Building,
    Quitting,
    Stopped(Reason),
}

impl Phase {
    /// the word the snapshot and a refusal use.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Resuming => "resuming",
            Self::Building => "building",
            Self::Quitting => "quitting",
            Self::Stopped(_) => "stopped",
        }
    }
}

/// what a request that changes the agent's state does in a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// begins the transition the method asks for.
    Begin,
    /// stops the runners, or abandons or cancels what is under way, and ends stopped.
    Stop,
    /// does what [`Action::Stop`] does and then ends the agent.
    Quit,
    /// the agent is already where the method asks it to be.
    NoChange,
    /// another transition is under way.
    Busy,
    /// the state does not allow the method.
    Refuse(&'static str),
}

/// why `build` is refused while the runners run.
pub const BUILD_WHILE_RUNNING: &str =
    "the runners are running; stop them before building the image";

impl Phase {
    /// what the lifecycle method `method` does here: the one table every handler of
    /// a phase follows, so they cannot differ.
    #[must_use]
    pub fn action(&self, method: &Method) -> Action {
        match (self, method) {
            (Self::Stopped(_), Method::StartAll | Method::Build) => Action::Begin,
            (Self::Stopped(_) | Self::Stopping | Self::Quitting, Method::StopAll)
            | (Self::Running, Method::StartAll) => Action::NoChange,
            (Self::Running, Method::Build) => Action::Refuse(BUILD_WHILE_RUNNING),
            (_, Method::StartAll | Method::Build) => Action::Busy,
            (_, Method::StopAll) => Action::Stop,
            (_, Method::Quit) => Action::Quit,
            _ => Action::Refuse("the method is not available"),
        }
    }
}

/// what the agent keeps between its transitions.
#[derive(Debug)]
pub struct Inner {
    pub phase: Phase,
    pub since: SystemTime,
    /// the latest load of the configuration, `None` until the first.
    pub loaded: Option<Loaded>,
    /// why each pool was skipped when the runners last stopped.
    pub skips: BTreeMap<String, String>,
    pub build: Option<BuildInfo>,
    pub restart_due: Option<SystemTime>,
    /// the image the latest check named, and what the check found.
    pub image: Option<(String, Option<ImageCheck>)>,
}

impl Inner {
    #[must_use]
    pub fn new(phase: Phase) -> Self {
        Self {
            phase,
            since: SystemTime::now(),
            loaded: None,
            skips: BTreeMap::new(),
            build: None,
            restart_due: None,
            image: None,
        }
    }
}

impl Inner {
    /// the configuration in use, if the agent holds one.
    #[must_use]
    pub fn plan(&self) -> Option<&Plan> {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.in_use.as_ref())
            .map(|in_use| &in_use.plan)
    }

    /// the problems the snapshot lists: those of `config.toml`, or of `config.json`
    /// when its migration was refused for them.
    #[must_use]
    pub fn problems(&self) -> &[String] {
        match &self.loaded {
            Some(Loaded {
                migration: Migration::Failed(MigrationFailure::JsonRejected(problems)),
                ..
            }) => problems,
            Some(loaded) => &loaded.problems,
            None => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the table of the resident states in `docs/operations/cli.md`, row by row, in the
    /// order stop all, start all, build, quit.
    #[test]
    fn each_phase_answers_the_lifecycle_methods_as_the_documented_table_says() {
        use Action::{Begin, Busy, NoChange, Quit, Refuse, Stop};
        let rows = [
            (
                Phase::Running,
                [Stop, NoChange, Refuse(BUILD_WHILE_RUNNING), Quit],
            ),
            (
                Phase::Stopped(Reason::by_you()),
                [NoChange, Begin, Begin, Quit],
            ),
            (Phase::Resuming, [Stop, Busy, Busy, Quit]),
            (Phase::Building, [Stop, Busy, Busy, Quit]),
            (Phase::Stopping, [NoChange, Busy, Busy, Quit]),
            (Phase::Quitting, [NoChange, Busy, Busy, Quit]),
        ];
        for (phase, expected) in rows {
            let methods = [
                Method::StopAll,
                Method::StartAll,
                Method::Build,
                Method::Quit,
            ];
            for (method, expected) in methods.iter().zip(expected) {
                assert_eq!(phase.action(method), expected, "{phase:?} {method:?}");
            }
        }
    }
}
