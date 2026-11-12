//! the summary: the heading and the line under it, the menu's first line and the icon's
//! tooltip, which say what the runners are doing in the same words.

use serde::Serialize;

use crate::state::{Context, Resident, Stop};
use crate::text::{capitalised, counted, plain};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// "3 of 6 runners up", the count as it is now.
    pub heading: String,
    /// since when and why: "Running since today 03:15 · 3 pools, 1 skipped".
    pub line: String,
    /// the menu's first line, and what the live region says of the summary: the
    /// settled count, with the state's word in front of it while it changes.
    pub menu_line: String,
    /// the icon's tooltip, which is also its accessible name.
    pub tooltip: String,
}

fn count_phrase(up: u32, wanted: u32) -> String {
    if wanted == 0 {
        "no runners up".to_string()
    } else if up == wanted && wanted >= 2 {
        format!("all {wanted} runners up")
    } else {
        format!("{up} of {wanted} runners up")
    }
}

/// `word` ahead of the count, or alone while there is no target to count against.
fn with_count(word: &str, up: u32, wanted: u32) -> String {
    if wanted == 0 {
        word.to_string()
    } else {
        format!("{word}, {}", count_phrase(up, wanted))
    }
}

fn stop_phrase(stop: &Stop) -> String {
    match stop {
        Stop::ByYou => "stopped by you".into(),
        Stop::NoConfiguration => "stopped, there is no configuration".into(),
        Stop::InvalidConfiguration => "stopped, the configuration is not valid".into(),
        Stop::ImageUnusable => "stopped, the runner image cannot run jobs".into(),
        Stop::NoPoolStarted => "stopped, no pool could start".into(),
        Stop::RestartLimit => format!(
            "stopped, the runtime core failed {} times",
            crate::text::RESTART_FAILURES
        ),
        Stop::ImageBuildFailed => "stopped, the image build failed".into(),
        Stop::Other(_) => "runners stopped".into(),
    }
}

/// while the agent drains for an update, what it is waiting for: "updating to 1.4.0:
/// waiting for 2 runners to finish", or "updating to 1.4.0" once it waits for none. The
/// agent's state stays running meanwhile, so a pending stop or quit is shown instead.
fn updating(context: &Context<'_>) -> Option<String> {
    if !matches!(context.resident, Resident::Running) {
        return None;
    }
    let update = context.snapshot?.agent.update.as_ref()?;
    let version = plain(&update.version);
    Some(match update.waiting {
        0 => format!("updating to {version}"),
        waiting => format!(
            "updating to {version}: waiting for {} to finish",
            counted(waiting, "runner", "runners")
        ),
    })
}

/// what the tooltip says after "Marshal: ".
fn phrase(context: &Context<'_>) -> String {
    let (up, settled, wanted) = (context.up, context.up_settled, context.wanted);
    if let Some(updating) = updating(context) {
        return updating;
    }
    match &context.resident {
        Resident::Running if context.restart_due.is_some() => {
            "restarting after a failure".to_string()
        }
        Resident::Running if context.waiting_for_docker() => "waiting for Docker".to_string(),
        Resident::Running => count_phrase(settled, wanted),
        Resident::Resuming => with_count("starting", settled, wanted),
        Resident::Stopping => with_count("stopping", up, wanted),
        Resident::Quitting => with_count("quitting", up, wanted),
        Resident::Building { stopping: false } => "building the runner image".to_string(),
        Resident::Building { stopping: true } => "stopping the build".to_string(),
        Resident::Stopped(stop) => stop_phrase(stop),
        Resident::StartingMarshal => "starting".to_string(),
        Resident::NoAgent => "agent not reachable".to_string(),
        Resident::NotAnswering => "the agent is not answering".to_string(),
        Resident::LockHeld => "another Marshal is running".to_string(),
        Resident::LostContact { .. } => "lost contact with the agent".to_string(),
        Resident::Unknown(_) => "the agent is in a state this version does not know".to_string(),
    }
}

/// "Running since today 03:15", and for the states that run the pools, how many pools
/// there are and how many are skipped; while the agent drains for an update, what it
/// waits for instead.
fn since_line(context: &Context<'_>) -> String {
    if let Some(updating) = updating(context) {
        return capitalised(&updating);
    }
    let clock = &context.input.clock;
    let since = |word: &str| format!("{word} since {}", clock.when_with_day(context.since_ms));
    let pools = || {
        if context.pools.is_empty() {
            return String::new();
        }
        let total = u32::try_from(context.pools.len()).unwrap_or(u32::MAX);
        let skipped = context
            .pools
            .iter()
            .filter(|pool| pool.state == "skipped")
            .count();
        let skipped = u32::try_from(skipped).unwrap_or(u32::MAX);
        let mut text = format!(" · {}", counted(total, "pool", "pools"));
        if skipped > 0 {
            text.push_str(&format!(", {skipped} skipped"));
        }
        text
    };
    match &context.resident {
        Resident::Running => format!("{}{}", since("Running"), pools()),
        Resident::Resuming => format!("{}{}", since("Starting"), pools()),
        Resident::Stopping => since("Stopping"),
        Resident::Quitting => since("Quitting"),
        Resident::Building { stopping: false } => since("Building the runner image"),
        Resident::Building { stopping: true } => since("Stopping the build"),
        Resident::Stopped(Stop::ByYou) => since("Stopped by you"),
        Resident::Stopped(_) => since("Stopped"),
        Resident::LostContact { last_heard_ms, .. } => {
            format!(
                "Last heard from the agent at {}",
                clock.when(*last_heard_ms)
            )
        }
        Resident::Unknown(_) => "The agent's state is not known to this version".to_string(),
        Resident::StartingMarshal
        | Resident::NoAgent
        | Resident::NotAnswering
        | Resident::LockHeld => String::new(),
    }
}

pub fn summary(context: &Context<'_>) -> Summary {
    let phrase = phrase(context);
    let no_target = context.wanted == 0
        && matches!(&context.resident, Resident::Stopped(stop) if stop.lacks_configuration());
    Summary {
        heading: capitalised(&count_phrase(context.up, context.wanted)),
        line: since_line(context),
        menu_line: if no_target {
            "No runners up".to_string()
        } else {
            capitalised(&phrase)
        },
        tooltip: format!("Marshal: {phrase}"),
    }
}
