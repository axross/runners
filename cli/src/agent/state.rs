//! where the agent is and what it knows about its configuration, for the snapshot.

use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::host_config::{Loaded, Migration, MigrationFailure};
use crate::image::check::ImageCheck;
use crate::validate::{Entry, Plan};

use super::api::Method;
use super::config::Effect;
use super::snapshot::{ApplyInfo, BuildInfo, Reason as WireReason};

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
    /// the computer slept, or woke, which stops every runner.
    pub const SLEPT: &'static str = "slept";
    /// the primary network changed, which stops every runner.
    pub const NETWORK_CHANGED: &'static str = "network_changed";
    /// a signal stop that came after too many resumes within an hour, so it arms none.
    pub const RESUMED_TOO_OFTEN: &'static str = "resumed_too_often";

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

    /// whether a signal stopped the runners rather than the user or an error, which an
    /// update keeps and a restart of the agent does not undo.
    #[must_use]
    pub fn is_signal(&self) -> bool {
        [Self::SLEPT, Self::NETWORK_CHANGED, Self::RESUMED_TOO_OFTEN].contains(&self.code)
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

/// why a lifecycle method is refused during a drain for an update.
pub const DRAINING: &str = "the agent is draining for an update";

/// why a drain is refused outside `running` and `stopped`.
pub const DRAIN_NOT_NOW: &str = "a drain for an update needs the agent running or stopped";

/// why `build` is refused while the runners run.
pub const BUILD_WHILE_RUNNING: &str =
    "the runners are running; stop them before building the image";

impl Phase {
    /// what the lifecycle method `method` does here, `draining` while a drain for an
    /// update is under way: the one table every handler of a phase follows, so they
    /// cannot differ.
    #[must_use]
    pub fn action(&self, method: &Method, draining: bool) -> Action {
        if draining {
            return match method {
                Method::StopAll => Action::Stop,
                Method::Quit => Action::Quit,
                _ => Action::Refuse(DRAINING),
            };
        }
        match (self, method) {
            (Self::Running | Self::Stopped(_), Method::DrainForUpdate { .. }) => Action::Begin,
            (_, Method::DrainForUpdate { .. }) => Action::Refuse(DRAIN_NOT_NOW),
            (Self::Stopped(_), Method::StartAll | Method::Launch | Method::Build) => Action::Begin,
            (Self::Stopped(_) | Self::Stopping | Self::Quitting, Method::StopAll)
            | (Self::Running, Method::StartAll | Method::Launch) => Action::NoChange,
            (Self::Running, Method::Build) => Action::Refuse(BUILD_WHILE_RUNNING),
            (_, Method::StartAll | Method::Launch | Method::Build) => Action::Busy,
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
    /// the drain for an update under way.
    pub drain: Option<Drain>,
    /// the apply of an edit that runs now.
    pub applying: Option<Applying>,
    /// the pools an apply removed, which the snapshot lists while a slot of theirs is
    /// still leaving, as one whose stop ran late or that keeps its evidence does after
    /// the apply has ended.
    pub leaving: Vec<Entry>,
    /// how the latest apply of an edit ended, or that it runs.
    pub last_apply: Option<ApplyInfo>,
}

/// a drain for an update under way, as the snapshot shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drain {
    pub version: String,
    pub waiting: u32,
    pub deadline: SystemTime,
}

/// an apply that runs: its effect on each pool.
#[derive(Debug)]
pub struct Applying {
    pub effects: Vec<Effect>,
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
            drain: None,
            applying: None,
            leaving: Vec::new(),
            last_apply: None,
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
                assert_eq!(
                    phase.action(method, false),
                    expected,
                    "{phase:?} {method:?}"
                );
            }
        }
    }

    fn drain() -> Method {
        Method::DrainForUpdate {
            version: "1.4.0".to_string(),
            requester: None,
        }
    }

    /// the table of The Update Drain in `docs/operations/cli.md`: a drain begins from running or
    /// stopped and is refused elsewhere, and while one is under way start all, build
    /// and another drain are refused while stop all and quit are taken.
    #[test]
    fn a_drain_begins_only_from_running_or_stopped_and_refuses_starts_while_under_way() {
        use Action::{Begin, Quit, Refuse, Stop};
        for phase in [Phase::Running, Phase::Stopped(Reason::by_you())] {
            assert_eq!(phase.action(&drain(), false), Begin, "{phase:?}");
        }
        for phase in [
            Phase::Resuming,
            Phase::Building,
            Phase::Stopping,
            Phase::Quitting,
        ] {
            assert_eq!(
                phase.action(&drain(), false),
                Refuse(DRAIN_NOT_NOW),
                "{phase:?}"
            );
        }
        let running = Phase::Running;
        assert_eq!(running.action(&Method::StopAll, true), Stop);
        assert_eq!(running.action(&Method::Quit, true), Quit);
        for method in [Method::StartAll, Method::Build, drain()] {
            assert_eq!(
                running.action(&method, true),
                Refuse(DRAINING),
                "{method:?}"
            );
        }
    }
}
