//! what choosing a menu item does: run it, ask first, or open the window. The question
//! is asked in the main window while it is shown, and otherwise in a window of its own.

use marshal_view::{Action, Confirmation, MenuItem};

use crate::session::Act;

/// what a yes to a question does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    Act(Act),
    Quit,
}

/// the acts whose question the main window's page shows, and answers by calling the
/// command that runs the act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmable {
    StopAll,
    StopBuild,
}

impl Confirmable {
    fn of(then: Then) -> Option<Self> {
        match then {
            Then::Act(Act::StopAll) => Some(Self::StopAll),
            Then::Act(Act::StopBuild) => Some(Self::StopBuild),
            Then::Act(Act::StartAll | Act::Build) | Then::Quit => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chosen {
    /// bring the main window forward.
    Open,
    Run(Act),
    TryAgain,
    Quit,
    /// ask in the main window, which is shown and has the page that asks it.
    AskInMainWindow {
        question: Confirmation,
        then: Confirmable,
    },
    /// ask in a small window of its own.
    AskInOwnWindow {
        question: Confirmation,
        then: Then,
    },
    /// the item is not available, or is not one the menu offers.
    Nothing,
}

/// the result of choosing `item`.
///
/// Quit is always asked in its own window: only the menu quits, and the main window's
/// capability grants no command that answers a quit.
#[must_use]
pub fn choose(item: &MenuItem, main_window_shown: bool) -> Chosen {
    if !item.available {
        return Chosen::Nothing;
    }
    let then = match &item.action {
        Action::OpenMarshal => return Chosen::Open,
        Action::TryAgain => return Chosen::TryAgain,
        Action::StartAll => Then::Act(Act::StartAll),
        Action::StopAll => Then::Act(Act::StopAll),
        Action::BuildImage => Then::Act(Act::Build),
        Action::StopBuild => Then::Act(Act::StopBuild),
        Action::Quit => Then::Quit,
        Action::SetToken(_) | Action::ReplaceToken(_) | Action::RemoveToken(_) => {
            return Chosen::Nothing;
        }
    };
    let Some(question) = item.confirmation.clone() else {
        return match then {
            Then::Act(act) => Chosen::Run(act),
            Then::Quit => Chosen::Quit,
        };
    };
    if main_window_shown && let Some(then) = Confirmable::of(then) {
        return Chosen::AskInMainWindow { question, then };
    }
    Chosen::AskInOwnWindow { question, then }
}
