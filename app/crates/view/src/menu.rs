//! the notification-area menu: Windows draws it, so the view says only its labels, its
//! order and which items are available. An unavailable item stays in place.

use serde::Serialize;

use crate::confirm::confirmation;
use crate::control::{Action, Confirmation, availability};
use crate::state::Context;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MenuItem {
    pub action: Action,
    /// in the host's case: sentence case on Windows, title case on the Mac.
    pub label: String,
    pub available: bool,
    /// why the item is unavailable. Windows cannot show it in a menu; the window's
    /// tooltips do.
    pub reason: Option<String>,
    pub confirmation: Option<Confirmation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Menu {
    /// the first line, unavailable like a label.
    pub summary: String,
    pub items: Vec<MenuItem>,
}

/// the items in order; their labels are the host's words.
const ITEMS: [Action; 5] = [
    Action::OpenMarshal,
    Action::StartAll,
    Action::StopAll,
    Action::BuildImage,
    Action::Quit,
];

pub fn menu(context: &Context<'_>, summary: &str) -> Menu {
    Menu {
        summary: summary.to_string(),
        items: ITEMS
            .into_iter()
            .zip(context.input.words.menu)
            .map(|(action, label)| {
                let state = availability(context, &action);
                MenuItem {
                    confirmation: state
                        .available
                        .then(|| confirmation(context, &action))
                        .flatten(),
                    label: label.to_string(),
                    available: state.available,
                    reason: state.reason,
                    action,
                }
            })
            .collect(),
    }
}
