//! the one step the window names after `config.toml` is fixed, or after an edit made in
//! the file is to be read: the footer's warnings, the Settings tab and its notes, the
//! live-region announcement of a refused setting and the banners of a stop for the
//! file all take it from here.

use crate::input::Input;
use crate::state::Context;

/// what the operator does to make Marshal read the file again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    /// a migration of `config.json` waits for the next start, or was rejected.
    QuitAndLaunch,
    /// the runners run or resume, and the agent offers the editor.
    Reload,
    /// the runners run or resume, and the editor cannot be used.
    StopAndStart,
    /// every other state.
    StartAll,
}

/// whether the snapshot says the migration of `config.json` is in `state`.
pub(crate) fn migration_is(context: &Context<'_>, state: &str) -> bool {
    context
        .snapshot
        .and_then(|snapshot| snapshot.configuration.migration.as_ref())
        .is_some_and(|migration| migration.state == state)
}

/// whether `config.json` was refused at its migration: the one failure that leaves no
/// configuration in use, as the others run `config.json`'s.
pub(crate) fn json_refused(context: &Context<'_>) -> bool {
    migration_is(context, "failed")
        && context
            .snapshot
            .is_some_and(|snapshot| snapshot.configuration.in_use.is_none())
}

impl NextStep {
    /// the one step the window names for the state `context` describes.
    pub(crate) fn of(context: &Context<'_>) -> Self {
        if migration_is(context, "pending") || json_refused(context) {
            Self::QuitAndLaunch
        } else if !context.agent_runs() {
            Self::StartAll
        } else if context.input.editor && !migration_is(context, "failed") {
            Self::Reload
        } else {
            Self::StopAndStart
        }
    }

    /// the step as an instruction: "choose Start All".
    #[must_use]
    pub fn choose(self) -> &'static str {
        match self {
            Self::QuitAndLaunch => "quit Marshal and launch it again",
            Self::Reload => "choose Reload config.toml",
            Self::StopAndStart => "choose Stop All and Start All",
            Self::StartAll => "choose Start All",
        }
    }

    /// when an edit made in the file takes effect.
    pub(crate) fn takes_effect(self) -> &'static str {
        match self {
            Self::QuitAndLaunch => "when you quit Marshal and launch it again",
            Self::Reload => "when you choose Reload config.toml",
            Self::StopAndStart => "when you choose Stop All and Start All",
            Self::StartAll => "at the next Start All",
        }
    }
}

/// the step the window names for the state `input` describes.
#[must_use]
pub fn next_step(input: &Input<'_>) -> NextStep {
    NextStep::of(&Context::new(input))
}
