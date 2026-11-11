//! the banner that leads the list: what the agent is doing or why it is stopped, in
//! the words of the boards, with the one action that resolves it.

use serde::Serialize;

use crate::clock::elapsed;
use crate::confirm::confirmation;
use crate::control::{Action, Control, Look, control};
use crate::input::Cause;
use crate::state::{Context, ImageStatus, Resident, Stop};
use crate::text::{LOG_FOLDER, counted, plain};
use crate::tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BannerKind {
    StoppedByYou,
    Stopping,
    Starting,
    Building,
    StoppingBuild,
    NoConfiguration,
    InvalidConfiguration,
    ImageCannotRunJobs,
    BuildFailed,
    NoPoolStarted,
    RestartLimit,
    StoppedForAnotherReason,
    Restarting,
    WaitingForDocker,
    LostContact,
    ImageOutOfDate,
    EveryRunnerBacksOff,
    ContainerLeft,
    NoTokens,
    Truncated,
    Quitting,
    UnknownState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Neutral,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Banner {
    pub kind: BannerKind,
    pub tone: Tone,
    pub heading: String,
    /// the lines under the heading, each on its own.
    pub lines: Vec<String>,
    /// the one action that resolves it, solid.
    pub action: Option<Control>,
    /// what the window's live region says when the banner appears.
    pub announcement: String,
}

/// how long before the stop a build may have ended for the stop to be that build's.
const BUILD_STOP_SLACK_MS: u64 = 10_000;

fn banner(
    kind: BannerKind,
    tone: Tone,
    heading: &str,
    lines: Vec<String>,
    announcement: String,
) -> Banner {
    Banner {
        kind,
        tone,
        heading: heading.to_string(),
        lines,
        action: None,
        announcement,
    }
}

/// the heading and the sentence that leads the body, as the live region reads them.
fn spoken(heading: &str, lead: &str) -> String {
    format!("{heading}. {lead}")
}

fn with_action(mut banner: Banner, action: Option<Control>) -> Banner {
    banner.action = action;
    banner
}

fn log_says_why() -> String {
    format!("the log in {LOG_FOLDER} says why")
}

/// the agent's words for a refusal, with "this CLI" in the words Marshal uses.
fn agent_words(message: &str) -> String {
    plain(message).replace("this CLI needs", "Marshal needs")
}

fn build_control(context: &Context<'_>, label: &str) -> Control {
    control(context, Action::BuildImage, label, Look::Solid, None)
}

fn stop_build_control(context: &Context<'_>) -> Control {
    let confirmation = confirmation(context, &Action::StopBuild);
    control(
        context,
        Action::StopBuild,
        "Stop the Build",
        Look::Destructive,
        confirmation,
    )
}

/// when the running build began.
fn build_started(context: &Context<'_>) -> u64 {
    context
        .snapshot
        .and_then(|snapshot| snapshot.agent.build.as_ref())
        .filter(|build| build.outcome == "building")
        .map_or(context.since_ms, |build| build.started)
}

fn stopped_by_you(context: &Context<'_>) -> Banner {
    let mut lines = Vec::new();
    if built_just_before_the_stop(context, "ready") {
        lines.push("The runner image is ready.".to_string());
    }
    lines.push("They stay stopped until you choose Start All.".to_string());
    let mut heading = "Runners stopped by you".to_string();
    if let Some(snapshot) = context.snapshot {
        let names: Vec<&str> = snapshot
            .leftovers
            .iter()
            .map(|left| left.name.as_str())
            .collect();
        if !names.is_empty() {
            heading.push_str("; ");
            heading.push_str(&left_headline(names.len()));
            lines.insert(0, left_line(&names));
        }
    }
    banner(
        BannerKind::StoppedByYou,
        Tone::Neutral,
        &heading,
        lines,
        heading.clone(),
    )
}

/// whether the latest build ended with `outcome` just before the agent stopped.
pub fn built_just_before_the_stop(context: &Context<'_>, outcome: &str) -> bool {
    context
        .snapshot
        .and_then(|snapshot| {
            let build = snapshot.agent.build.as_ref()?;
            let finished = build.finished?;
            (build.outcome == outcome
                && snapshot.agent.since.abs_diff(finished) <= BUILD_STOP_SLACK_MS)
                .then_some(())
        })
        .is_some()
        && matches!(context.resident, Resident::Stopped(Stop::ByYou))
}

fn left_headline(count: usize) -> String {
    if count == 1 {
        "a container could not be removed".to_string()
    } else {
        "some containers could not be removed".to_string()
    }
}

fn left_line(names: &[&str]) -> String {
    let verb = if names.len() == 1 {
        "remains"
    } else {
        "remain"
    };
    format!(
        "{} {verb}; the next start removes {}. {}.",
        crate::text::list_of(&names.iter().map(|name| plain(name)).collect::<Vec<_>>()),
        if names.len() == 1 { "it" } else { "them" },
        crate::text::capitalised(&log_says_why()),
    )
}

fn leftover_banner(context: &Context<'_>) -> Option<Banner> {
    let snapshot = context.snapshot?;
    if snapshot.leftovers.is_empty() || matches!(context.resident, Resident::Stopped(Stop::ByYou)) {
        return None;
    }
    let names: Vec<&str> = snapshot
        .leftovers
        .iter()
        .map(|left| left.name.as_str())
        .collect();
    let heading = crate::text::capitalised(&left_headline(names.len()));
    Some(banner(
        BannerKind::ContainerLeft,
        Tone::Warning,
        &heading,
        vec![left_line(&names)],
        heading.clone(),
    ))
}

fn stopped(context: &Context<'_>, stop: &Stop) -> Banner {
    let path = &context.configuration_path;
    let message = context
        .snapshot
        .and_then(|snapshot| snapshot.agent.reason.as_ref())
        .map(|reason| reason.message.as_str())
        .unwrap_or_default();
    match stop {
        Stop::ByYou => stopped_by_you(context),
        Stop::NoConfiguration => {
            let heading = "Stopped: there is no configuration";
            let lead = format!("{path} does not exist");
            banner(
                BannerKind::NoConfiguration,
                Tone::Error,
                heading,
                vec![
                    lead.clone(),
                    "Create it as Marshal's operations document describes, then choose Start All."
                        .to_string(),
                ],
                spoken(heading, &lead),
            )
        }
        Stop::InvalidConfiguration => {
            let heading = "Stopped: the configuration is not valid";
            let problems = context
                .snapshot
                .map(|snapshot| snapshot.configuration.problems.as_slice())
                .unwrap_or_default();
            let mut lines: Vec<String> = problems.iter().map(|problem| plain(problem)).collect();
            if lines.is_empty() {
                lines.push(plain(message));
            }
            let lead = lines.first().cloned().unwrap_or_default();
            lines.push(format!("Fix {path}, then choose Start All."));
            banner(
                BannerKind::InvalidConfiguration,
                Tone::Error,
                heading,
                lines,
                spoken(heading, &lead),
            )
        }
        Stop::ImageUnusable => {
            let heading = "Stopped: the runner image cannot run jobs";
            let lead = agent_words(message);
            let lines = vec![
                lead.clone(),
                "Build the runner image, then choose Start All.".to_string(),
            ];
            with_action(
                banner(
                    BannerKind::ImageCannotRunJobs,
                    Tone::Error,
                    heading,
                    lines,
                    spoken(heading, &lead),
                ),
                Some(build_control(context, "Build Image")),
            )
        }
        Stop::ImageBuildFailed => build_failed(context),
        Stop::NoPoolStarted => {
            let heading = "Stopped: no pool could start";
            let (lead, lines) = if context.every_pool_lacks_a_token() {
                let lead = "Every pool was skipped because its repository has no token.";
                (
                    lead,
                    vec![format!(
                        "{lead} Set a token for each repository, then choose Start All."
                    )],
                )
            } else {
                let lead = "Each pool below says why.";
                (
                    lead,
                    vec![format!("{lead} Fix them, then choose Start All.")],
                )
            };
            banner(
                BannerKind::NoPoolStarted,
                Tone::Error,
                heading,
                lines,
                spoken(heading, lead),
            )
        }
        Stop::RestartLimit => {
            let heading = "Stopped: the runtime core failed 4 times within 10 minutes";
            let lead = format!("{}.", crate::text::capitalised(&log_says_why()));
            banner(
                BannerKind::RestartLimit,
                Tone::Error,
                heading,
                vec![format!("{lead} Start All tries again.")],
                spoken(heading, &lead),
            )
        }
        Stop::Other(_) => {
            let heading = "Runners stopped";
            let lead = if message.is_empty() {
                "The agent gave no reason.".to_string()
            } else {
                plain(message)
            };
            banner(
                BannerKind::StoppedForAnotherReason,
                Tone::Neutral,
                heading,
                vec![
                    lead.clone(),
                    "Choose Start All to run them again.".to_string(),
                ],
                spoken(heading, &lead),
            )
        }
    }
}

fn build_failed(context: &Context<'_>) -> Banner {
    let heading = "The image build failed";
    let build = context
        .snapshot
        .and_then(|snapshot| snapshot.agent.build.as_ref());
    let mut lines = Vec::new();
    if let Some(finished) = build.and_then(|build| build.finished.map(|end| (build.started, end))) {
        lines.push(format!(
            "The build failed after {}.",
            elapsed(finished.1.saturating_sub(finished.0))
        ));
    }
    if let Some(message) = build.and_then(|build| build.message.as_deref()) {
        lines.push(plain(message));
    }
    lines.push(format!("Docker's output is in the log in {LOG_FOLDER}."));
    let lead = lines.first().cloned().unwrap_or_default();
    with_action(
        banner(
            BannerKind::BuildFailed,
            Tone::Error,
            heading,
            lines,
            spoken(heading, &lead),
        ),
        Some(build_control(context, "Build Again")),
    )
}

fn building(context: &Context<'_>, stopping: bool) -> Banner {
    if stopping {
        let heading = "Stopping the build";
        return with_action(
            banner(
                BannerKind::StoppingBuild,
                Tone::Neutral,
                heading,
                vec!["Up to 30 s.".to_string()],
                heading.to_string(),
            ),
            Some(stop_build_control(context)),
        );
    }
    let heading = "Building the runner image";
    let so_far = format!(
        "{} so far. Docker's output goes to the log in {LOG_FOLDER}.",
        elapsed(
            context
                .input
                .clock
                .now_ms
                .saturating_sub(build_started(context))
        )
    );
    let line = match &context.image_name {
        Some(name) => format!("{name} · {so_far}"),
        None => so_far,
    };
    with_action(
        banner(
            BannerKind::Building,
            Tone::Neutral,
            heading,
            vec![line],
            heading.to_string(),
        ),
        Some(stop_build_control(context)),
    )
}

fn stopping(context: &Context<'_>) -> Banner {
    let heading = "Stopping all runners";
    let line = if context.up > 0 {
        "Up to 30 s. Any job running on them fails."
    } else {
        "Up to 30 s."
    };
    banner(
        BannerKind::Stopping,
        Tone::Neutral,
        heading,
        vec![line.to_string()],
        heading.to_string(),
    )
}

fn quitting(context: &Context<'_>) -> Banner {
    let heading = "Quitting Marshal";
    let mut lines = Vec::new();
    if context.up > 0 {
        lines.push(
            "Stopping all runners first: up to 30 s. Any job running on them fails.".to_string(),
        );
    }
    lines.push("The window closes with the app.".to_string());
    banner(
        BannerKind::Quitting,
        Tone::Neutral,
        heading,
        lines,
        heading.to_string(),
    )
}

fn lost_contact(context: &Context<'_>, last_heard_ms: u64, cause: Cause) -> Banner {
    let heading = "Lost contact with the agent";
    let from = format!(
        "The list below is from {} and may be out of date.",
        context.input.clock.when(last_heard_ms)
    );
    let (line, action) = match cause {
        Cause::Dropped => (
            format!("{from} Reconnecting; Marshal starts the agent once."),
            None,
        ),
        Cause::RestartFailed => (
            format!(
                "{from} Marshal started the agent again and it did not answer; {}.",
                log_says_why()
            ),
            Some(control(
                context,
                Action::TryAgain,
                "Try Again",
                Look::Solid,
                None,
            )),
        ),
        Cause::Busy => (
            format!("{from} The agent is busy; Marshal tries again every 5 s."),
            Some(control(
                context,
                Action::TryAgain,
                "Try Again",
                Look::Solid,
                None,
            )),
        ),
    };
    with_action(
        banner(
            BannerKind::LostContact,
            Tone::Warning,
            heading,
            vec![line],
            heading.to_string(),
        ),
        action,
    )
}

fn running(context: &Context<'_>) -> Option<Banner> {
    if let Some(due) = context.restart_due {
        let heading = format!("Restarting at {}", context.input.clock.when(due));
        let lead = "The runtime core failed; it restarts after 10, 30 or 60 s.";
        return Some(banner(
            BannerKind::Restarting,
            Tone::Warning,
            &heading,
            vec![
                format!("{lead} {}.", crate::text::capitalised(&log_says_why())),
                "No pools are listed until it restarts.".to_string(),
            ],
            spoken(&heading, lead),
        ));
    }
    if context.waiting_for_docker() {
        let heading = "Waiting for Docker";
        let line = "Start Docker Desktop. Marshal checks every 5 s.";
        return Some(banner(
            BannerKind::WaitingForDocker,
            Tone::Warning,
            heading,
            vec![line.to_string()],
            spoken(heading, line),
        ));
    }
    if context.every_runner_backs_off() {
        let heading = "Every runner is backing off";
        let lines = if context.image.is_unusable() {
            let contract = crate::footer::contract(context).unwrap_or_default();
            vec![
                format!("Runner image: {contract}."),
                "The image was replaced outside Marshal. Stop the runners, then choose Build Image."
                    .to_string(),
            ]
        } else {
            vec!["Each runner's reason is on its row below.".to_string()]
        };
        let lead = lines.first().cloned().unwrap_or_default();
        return Some(banner(
            BannerKind::EveryRunnerBacksOff,
            Tone::Error,
            heading,
            lines,
            spoken(heading, &lead),
        ));
    }
    out_of_date(context)
}

/// the banner for an image that runs jobs but should be built again.
fn out_of_date(context: &Context<'_>) -> Option<Banner> {
    if context.image != ImageStatus::OutOfDate {
        return None;
    }
    let image = context.snapshot?.images.first()?;
    let (kind, days) = image.known_warnings().find_map(|(kind, warning)| {
        matches!(
            kind,
            marshal_cli::agent::snapshot::WarningKind::ImageOld
                | marshal_cli::agent::snapshot::WarningKind::PackagesOld
        )
        .then_some((kind, warning.days))
    })?;
    let age = days.map_or_else(String::new, |days| {
        if kind == marshal_cli::agent::snapshot::WarningKind::PackagesOld {
            format!(
                "the image's operating-system packages are {} old and it should be rebuilt",
                counted(u32::try_from(days).unwrap_or(u32::MAX), "day", "days")
            )
        } else {
            format!(
                "the image is {} old and should be rebuilt",
                counted(u32::try_from(days).unwrap_or(u32::MAX), "day", "days")
            )
        }
    });
    let heading = "The runner image is out of date";
    let mut lines = Vec::new();
    if !age.is_empty() {
        lines.push(age);
    }
    lines.push(
        "It still runs jobs. To build it again, stop the runners, then choose Build Image."
            .to_string(),
    );
    let lead = lines.first().cloned().unwrap_or_default();
    Some(banner(
        BannerKind::ImageOutOfDate,
        Tone::Warning,
        heading,
        lines,
        spoken(heading, &lead),
    ))
}

fn unknown_state(state: &str) -> Banner {
    let heading = "Marshal does not know what the agent is doing";
    let line = format!(
        "The agent reports the state \u{201c}{state}\u{201d}, which this version of Marshal does not know."
    );
    banner(
        BannerKind::UnknownState,
        Tone::Warning,
        heading,
        vec![line.clone()],
        spoken(heading, &line),
    )
}

fn no_tokens(context: &Context<'_>) -> Option<Banner> {
    let repositories = tokens::repositories(context);
    if repositories.is_empty() || repositories.iter().any(|repository| repository.has_token) {
        return None;
    }
    if matches!(context.resident, Resident::Stopped(Stop::NoPoolStarted))
        && context.every_pool_lacks_a_token()
    {
        return None;
    }
    let heading = "No repository has a token yet";
    let line = "Set a token for each repository, then choose Start All.";
    Some(banner(
        BannerKind::NoTokens,
        Tone::Warning,
        heading,
        vec![line.to_string()],
        spoken(heading, line),
    ))
}

fn truncated(context: &Context<'_>) -> Option<Banner> {
    let omitted = &context.snapshot?.omitted;
    let parts: Vec<String> = [
        (omitted.runners, "runner", "runners"),
        (omitted.pools, "pool", "pools"),
        (
            omitted.problems,
            "configuration problem",
            "configuration problems",
        ),
        (omitted.repositories, "repository", "repositories"),
        (
            omitted.leftovers,
            "leftover container",
            "leftover containers",
        ),
        (omitted.images, "image", "images"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, one, many)| counted(count, one, many))
    .collect();
    if parts.is_empty() {
        return None;
    }
    let heading = "Part of the list is left out";
    let line = format!(
        "The agent's list was too large to send whole. Not shown: {}.",
        crate::text::list_of(&parts)
    );
    Some(banner(
        BannerKind::Truncated,
        Tone::Warning,
        heading,
        vec![line.clone()],
        spoken(heading, &line),
    ))
}

/// the banners of the list, first the one that says what the agent is doing.
pub fn banners(context: &Context<'_>) -> Vec<Banner> {
    let state = match &context.resident {
        Resident::Running => running(context),
        Resident::Resuming => Some(banner(
            BannerKind::Starting,
            Tone::Neutral,
            "Starting all runners",
            vec!["Reading the configuration and checking the image.".to_string()],
            "Starting all runners".to_string(),
        )),
        Resident::Stopping => Some(stopping(context)),
        Resident::Building { stopping } => Some(building(context, *stopping)),
        Resident::Stopped(stop) => Some(stopped(context, stop)),
        Resident::Quitting => Some(quitting(context)),
        Resident::LostContact {
            last_heard_ms,
            cause,
        } => Some(lost_contact(context, *last_heard_ms, *cause)),
        Resident::Unknown(state) => Some(unknown_state(state)),
        Resident::StartingMarshal
        | Resident::NoAgent
        | Resident::NotAnswering
        | Resident::LockHeld => None,
    };
    state
        .into_iter()
        .chain(leftover_banner(context))
        .chain(no_tokens(context))
        .chain(truncated(context))
        .collect()
}
