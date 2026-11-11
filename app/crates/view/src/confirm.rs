//! the questions asked before an action that costs something. They name the
//! consequence, and mention running jobs only while a runner is up.

use crate::control::{Action, Confirmation};
use crate::state::{Context, Resident};

const CANCEL: &str = "Cancel";

fn question(title: &str, body: &str, confirm: &str) -> Confirmation {
    Confirmation {
        title: title.to_string(),
        body: body.to_string(),
        confirm: confirm.to_string(),
        cancel: CANCEL.to_string(),
    }
}

/// the question to ask before `action`, or `None` when it asks none. Removing a token
/// asks its own question, which needs the repository's pools: see
/// [`crate::tokens`].
pub fn confirmation(context: &Context<'_>, action: &Action) -> Option<Confirmation> {
    let jobs_run = context.up > 0;
    match action {
        Action::StopAll => Some(question(
            "Stop all runners?",
            if jobs_run {
                "Any job running on them fails."
            } else {
                "They stay stopped until you choose Start All."
            },
            "Stop",
        )),
        Action::StopBuild => Some(question(
            "Stop the build?",
            "A build in progress is discarded. If it has already finished, the new image stays.",
            "Stop",
        )),
        Action::Quit => Some(match &context.resident {
            Resident::Running | Resident::Resuming | Resident::Stopping if jobs_run => question(
                "Quit Marshal?",
                "Its runners stop, and any job running on them fails.",
                "Quit",
            ),
            Resident::Running | Resident::Resuming | Resident::Stopping => question(
                "Quit Marshal?",
                "Its runners stop, and Marshal ends.",
                "Quit",
            ),
            Resident::Building { .. } => question(
                "Quit Marshal?",
                "The image build is discarded, and Marshal ends.",
                "Quit",
            ),
            Resident::Stopped(_) => question("Quit Marshal?", "Marshal and its agent end.", "Quit"),
            _ => question(
                "Quit Marshal?",
                "Marshal cannot reach its agent, so any runners keep running. This ends only the app.",
                "Quit",
            ),
        }),
        _ => None,
    }
}
