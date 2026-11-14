//! the Settings tab: the five `[lifecycle]` settings in two groups, each a checkbox with
//! its description, its state against its Windows default, and a note when something
//! about it needs saying. The agent writes a change; the view shows what it came to.

use marshal_cli::agent::StopReason;
use marshal_cli::agent::machine::{RESUME_NO_GATEWAY, RESUME_WAITING};
use marshal_cli::agent::snapshot::LifecycleInfo;
use marshal_cli::lifecycle_settings::Setting;
use serde::Serialize;

use crate::input::SettingResult;
use crate::state::{Context, Resident, Stop};
use crate::text::{capitalised, plain};

const INTRO: &str = "Saved in config.toml as soon as you change it. An edit made in the file \
takes effect at the next Start All.";

const INVALID: &str = "Settings cannot be changed while config.toml has problems. Fix the file, then choose Start All.";

/// the chip the Settings tab carries while something there needs the user.
pub const NEEDS_ATTENTION: &str = "Needs attention";

/// each setting with its group, label and description, in the order the window shows
/// them.
const SETTINGS: [(&str, Setting, &str, &str); 5] = [
    (
        "Starting Marshal",
        Setting::StartAtLogin,
        "Start Marshal when I sign in",
        "Marshal starts with only its icon in the notification area.",
    ),
    (
        "Starting Marshal",
        Setting::StartRunnersOnLaunch,
        "Start the runners when Marshal starts",
        "Applies when you launch Marshal or sign in, not after an update.",
    ),
    (
        "Sleep and network",
        Setting::KeepAwake,
        "Keep the computer awake while runners run",
        "Prevents idle sleep only. Closing the lid or choosing Sleep still sleeps.",
    ),
    (
        "Sleep and network",
        Setting::StopOnNetworkChange,
        "Stop the runners when the network changes",
        "A sleep or wake always stops them.",
    ),
    (
        "Sleep and network",
        Setting::ResumeAfterStop,
        "Start the runners again after a sleep or network stop",
        "Only on the network they stopped on.",
    ),
];

/// the label of the setting `key` names, as the window shows it.
#[must_use]
pub fn label(key: &str) -> Option<&'static str> {
    SETTINGS
        .into_iter()
        .find(|(_, setting, _, _)| setting.name() == key)
        .map(|(_, _, label, _)| label)
}

/// how a note is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteTone {
    Info,
    Warning,
    Error,
}

/// what the window says under one setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingNote {
    pub text: String,
    pub tone: NoteTone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingRow {
    /// the setting's key in `[lifecycle]`.
    pub key: String,
    pub label: String,
    pub description: String,
    /// "On (default).", "Off (default: on).", or why the value is not known.
    pub state: String,
    /// the checkbox's value, or `None` while it is not known.
    pub checked: Option<bool>,
    pub note: Option<SettingNote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingGroup {
    pub title: String,
    pub rows: Vec<SettingRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsTab {
    pub intro: String,
    /// why no setting can be changed now; the page makes it every checkbox's
    /// description, and the checkboxes stay focusable and do not toggle.
    pub unavailable: Option<String>,
    pub groups: Vec<SettingGroup>,
    /// [`NEEDS_ATTENTION`] while something on the tab needs the user.
    pub chip: Option<String>,
}

fn lifecycle<'a>(context: &Context<'a>) -> Option<&'a LifecycleInfo> {
    context
        .snapshot
        .and_then(|snapshot| snapshot.lifecycle.as_ref())
}

/// whether the configuration file cannot be edited: missing, unreadable or with problems.
fn file_unusable(context: &Context<'_>) -> Option<&'static str> {
    match &context.resident {
        Resident::Stopped(Stop::NoConfiguration) => {
            return Some("Settings cannot be changed until config.toml exists.");
        }
        Resident::Stopped(Stop::InvalidConfiguration) => return Some(INVALID),
        _ => {}
    }
    let status = context
        .snapshot
        .map_or("", |snapshot| snapshot.configuration.status.as_str());
    match status {
        "missing" => Some("Settings cannot be changed until config.toml exists."),
        "invalid" | "unreadable" => Some(INVALID),
        _ => None,
    }
}

fn unavailable(context: &Context<'_>) -> Option<String> {
    let why = match &context.resident {
        Resident::LostContact { .. } => {
            Some("Settings cannot be changed while Marshal's agent does not answer.")
        }
        Resident::Quitting => Some("Settings cannot be changed while Marshal is quitting."),
        Resident::Unknown(_) => Some(
            "Settings cannot be changed while this version of Marshal does not know what the agent is doing.",
        ),
        _ => None,
    }
    .or_else(|| file_unusable(context))
    .or_else(|| {
        context
            .input
            .settings
            .and_then(|ui| ui.writing.as_ref())
            .map(|_| "Settings cannot be changed while another setting is being saved.")
    })
    .or_else(|| {
        lifecycle(context)
            .is_none()
            .then_some("Settings cannot be changed: the agent did not report them.")
    });
    why.map(str::to_string)
}

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

/// what the latest change of a setting says under it, as the live region says it too.
fn result_note(result: &SettingResult) -> Option<SettingNote> {
    let (text, tone) = match result {
        SettingResult::Saved { .. } => return None,
        SettingResult::Refused { code, .. } if code == "config_changed" => (
            "Not saved: config.toml changed since Marshal read it. Your edits in the file were \
             kept. Choose Start All to read them, then change the setting again."
                .to_string(),
            NoteTone::Error,
        ),
        SettingResult::Refused { code, .. } if code == "busy" => (
            "Not saved: another change to config.toml was being applied. Change the setting again."
                .to_string(),
            NoteTone::Error,
        ),
        SettingResult::Refused { words, .. } => (
            format!("Not saved: {}.", plain(words).trim_end_matches('.')),
            NoteTone::Error,
        ),
        SettingResult::LostContact => (
            "Not saved: Marshal has lost contact with the agent.".to_string(),
            NoteTone::Error,
        ),
        SettingResult::NotAnswered => (
            "The agent has not answered yet and may still save it. This shows the setting as \
             the agent last reported it."
                .to_string(),
            NoteTone::Info,
        ),
    };
    Some(SettingNote { text, tone })
}

impl SettingResult {
    /// what the live region says of the result; `label` is the setting's.
    #[must_use]
    pub fn announcement(&self, label: &str) -> String {
        match self {
            Self::Saved { on } => {
                format!("Saved: \"{label}\" is {}.", if *on { "on" } else { "off" })
            }
            other => result_note(other).map_or_else(String::new, |note| note.text),
        }
    }
}

/// what the agent's state says under `setting`, when anything.
fn state_note(
    context: &Context<'_>,
    lifecycle: Option<&LifecycleInfo>,
    setting: Setting,
) -> Option<SettingNote> {
    let lifecycle = lifecycle?;
    let note = |text: String, tone| Some(SettingNote { text, tone });
    match setting {
        Setting::StartAtLogin => match lifecycle.sign_in.as_deref() {
            Some("turned_off") => note(
                "Windows' Startup apps setting keeps Marshal from starting at sign-in. Turn \
                 Marshal on in Windows Settings\u{a0}>\u{a0}Apps\u{a0}>\u{a0}Startup."
                    .to_string(),
                NoteTone::Warning,
            ),
            Some("failed") => note(
                format!(
                    "Marshal could not register itself to start at sign-in ({}). It tries again \
                     at the next launch or Start All.",
                    plain(
                        lifecycle
                            .sign_in_error
                            .as_deref()
                            .unwrap_or("no reason given")
                    )
                ),
                NoteTone::Error,
            ),
            _ => None,
        },
        Setting::KeepAwake => lifecycle.keep_awake_warning.as_deref().and_then(|warning| {
            note(
                format!("{}.", capitalised(plain(warning).trim_end_matches('.'))),
                NoteTone::Warning,
            )
        }),
        Setting::ResumeAfterStop => {
            if matches!(
                &context.resident,
                Resident::Stopped(Stop::Signal(code)) if code == StopReason::RESUMED_TOO_OFTEN
            ) {
                return note(
                    "Not resumed: the runners stopped and started again too often within an \
                     hour. Choose Start All to run them again."
                        .to_string(),
                    NoteTone::Warning,
                );
            }
            match lifecycle.resume.as_deref() {
                Some(RESUME_WAITING) => note(
                    "Waiting for the network the runners stopped on.".to_string(),
                    NoteTone::Info,
                ),
                Some(RESUME_NO_GATEWAY) => note(
                    "Not resuming: the network the runners stopped on has no gateway hardware \
                     address to recognise it by."
                        .to_string(),
                    NoteTone::Info,
                ),
                _ => None,
            }
        }
        Setting::StartRunnersOnLaunch | Setting::StopOnNetworkChange => None,
    }
}

/// whether the configuration's problems name `setting`, so its value cannot be read.
fn unreadable(context: &Context<'_>, setting: Setting) -> bool {
    let named = format!("lifecycle.{}", setting.name());
    context.snapshot.is_some_and(|snapshot| {
        snapshot
            .configuration
            .problems
            .iter()
            .any(|problem| problem.contains(&named))
    })
}

fn row(
    context: &Context<'_>,
    lifecycle: Option<&LifecycleInfo>,
    (setting, label, description): (Setting, &str, &str),
) -> (SettingRow, bool) {
    let key = setting.name();
    let value = lifecycle
        .filter(|_| !unreadable(context, setting))
        .and_then(|lifecycle| lifecycle.settings.get(key));
    let state = match value {
        Some(value) if value.on == setting.windows_default() => {
            format!("{} (default).", on_off(value.on))
        }
        Some(value) => format!(
            "{} (default: {}).",
            on_off(value.on),
            on_off(setting.windows_default()).to_lowercase()
        ),
        None if lifecycle.is_some() => "Not readable in config.toml.".to_string(),
        None => "Not known while config.toml cannot be read.".to_string(),
    };
    let result = context
        .input
        .settings
        .and_then(|ui| ui.results.get(key))
        .and_then(result_note);
    let refused = result
        .as_ref()
        .is_some_and(|note| note.tone == NoteTone::Error);
    let startup_off = lifecycle.and_then(|l| l.sign_in.as_deref()) == Some("turned_off")
        && setting == Setting::StartAtLogin;
    let row = SettingRow {
        key: key.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        state,
        checked: value.map(|value| value.on),
        note: result.or_else(|| state_note(context, lifecycle, setting)),
    };
    (row, refused || startup_off)
}

pub fn tab(context: &Context<'_>) -> SettingsTab {
    let lifecycle = lifecycle(context);
    let unavailable = unavailable(context);
    let mut attention = file_unusable(context) == Some(INVALID);
    let mut groups: Vec<SettingGroup> = Vec::new();
    for (group, setting, label, description) in SETTINGS {
        let (row, needs) = row(context, lifecycle, (setting, label, description));
        attention |= needs;
        match groups.last_mut().filter(|last| last.title == group) {
            Some(last) => last.rows.push(row),
            None => groups.push(SettingGroup {
                title: group.to_string(),
                rows: vec![row],
            }),
        }
    }
    SettingsTab {
        intro: if file_unusable(context) == Some(INVALID) {
            INVALID.to_string()
        } else {
            INTRO.to_string()
        },
        unavailable,
        groups,
        chip: attention.then(|| NEEDS_ATTENTION.to_string()),
    }
}
