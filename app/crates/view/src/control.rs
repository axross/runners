//! the actions a control or a menu item can run, whether each is available and, when it
//! is not, why: the plan's availability table, in one place so the window, the banners
//! and the menu cannot disagree.

use serde::Serialize;

use crate::state::{Context, Resident};

/// what a control does. Token actions name the repository, `owner/name` in lower case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    OpenMarshal,
    StartAll,
    StopAll,
    BuildImage,
    StopBuild,
    TryAgain,
    Quit,
    SetToken(String),
    ReplaceToken(String),
    RemoveToken(String),
}

/// how a control is drawn: one action is solid at a time, and an unavailable one takes
/// the neutral look and is never dimmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Look {
    Solid,
    Tonal,
    Neutral,
    Destructive,
}

/// a question asked before an action that costs something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Confirmation {
    pub title: String,
    pub body: String,
    pub confirm: String,
    pub cancel: String,
}

/// a button, a pill or a menu item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Control {
    pub action: Action,
    pub label: String,
    pub look: Look,
    pub available: bool,
    /// "Unavailable: …" when the control is not available; the page makes it the
    /// control's description and its tooltip.
    pub reason: Option<String>,
    /// the question to ask first, when the action asks one.
    pub confirmation: Option<Confirmation>,
}

/// whether an action can run now, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    pub available: bool,
    pub reason: Option<String>,
}

impl Availability {
    fn yes() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    fn no(why: &str) -> Self {
        Self {
            available: false,
            reason: Some(format!("Unavailable: {why}")),
        }
    }
}

/// the reason that holds for every action when Marshal does not control the runners or
/// is on its way out, or `None` when the agent's own state decides.
fn not_in_control(resident: &Resident) -> Option<&'static str> {
    match resident {
        Resident::StartingMarshal => Some("Marshal is starting."),
        Resident::Quitting => Some("Marshal is quitting."),
        Resident::LostContact { .. } => Some("Marshal has lost contact with the agent."),
        Resident::NoAgent => Some("Marshal cannot reach its agent."),
        Resident::NotAnswering => Some("the agent is not answering."),
        Resident::LockHeld => Some("another Marshal is running."),
        Resident::Unknown(_) => {
            Some("this version of Marshal does not know what the agent is doing.")
        }
        _ => None,
    }
}

fn while_building(stopping: bool) -> &'static str {
    if stopping {
        "the build is stopping."
    } else {
        "the runner image is being built."
    }
}

pub const BUILD_NEEDS_CONFIGURATION: &str =
    "Build needs a valid configuration, because the image name comes from it.";

/// whether `action` can run in the state of `context`.
pub fn availability(context: &Context<'_>, action: &Action) -> Availability {
    let resident = &context.resident;
    match action {
        Action::OpenMarshal | Action::Quit => match resident {
            Resident::Quitting => Availability::no("Marshal is quitting."),
            _ => Availability::yes(),
        },
        Action::TryAgain => match resident {
            Resident::NoAgent | Resident::NotAnswering | Resident::LostContact { .. } => {
                Availability::yes()
            }
            _ => Availability::no("there is nothing to try again."),
        },
        Action::SetToken(_) | Action::ReplaceToken(_) | Action::RemoveToken(_) => {
            // the agent takes a token in any state it answers in
            not_in_control(resident).map_or_else(Availability::yes, Availability::no)
        }
        Action::StartAll | Action::StopAll | Action::BuildImage | Action::StopBuild => {
            if let Some(why) = not_in_control(resident) {
                return Availability::no(why);
            }
            agent_action(context, action)
        }
    }
}

fn agent_action(context: &Context<'_>, action: &Action) -> Availability {
    match (&context.resident, action) {
        (Resident::Running, Action::StartAll) => {
            Availability::no("the runners are already running.")
        }
        (Resident::Resuming, Action::StartAll) => Availability::no("the runners are starting."),
        (Resident::Stopped(_), Action::StartAll) => Availability::yes(),
        (Resident::Running | Resident::Resuming, Action::StopAll) => Availability::yes(),
        (Resident::Stopped(_), Action::StopAll) => {
            Availability::no("the runners are already stopped.")
        }
        (Resident::Running | Resident::Resuming, Action::BuildImage) => {
            Availability::no("stop the runners first.")
        }
        (Resident::Stopped(stop), Action::BuildImage) if stop.lacks_configuration() => {
            Availability::no(BUILD_NEEDS_CONFIGURATION)
        }
        (Resident::Stopped(_), Action::BuildImage) => Availability::yes(),
        (Resident::Stopping, _) => Availability::no("the runners are stopping."),
        (Resident::Building { stopping }, Action::StopBuild) => {
            if *stopping {
                Availability::no("the build is stopping.")
            } else {
                Availability::yes()
            }
        }
        (Resident::Building { stopping }, _) => Availability::no(while_building(*stopping)),
        (_, Action::StopBuild) => Availability::no("no build is running."),
        _ => Availability::no("the agent is not in a state that allows it."),
    }
}

/// a [`Control`] for `action`, in the look an available or unavailable control takes.
pub fn control(
    context: &Context<'_>,
    action: Action,
    label: &str,
    available_look: Look,
    confirmation: Option<Confirmation>,
) -> Control {
    let Availability { available, reason } = availability(context, &action);
    Control {
        look: if available {
            available_look
        } else {
            Look::Neutral
        },
        action,
        label: label.to_string(),
        available,
        reason,
        confirmation,
    }
}
