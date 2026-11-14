//! the view state: everything the window, the menu and the notification-area icon show,
//! computed from the agent's latest snapshot and the app's own state. The page sets the
//! text of every field as text and never as markup.

use marshal_cli::agent::snapshot::Snapshot;
use serde::Serialize;

use crate::banner::{Banner, banners};
use crate::clock::Clock;
use crate::confirm::confirmation;
use crate::control::{Action, Control, Look, control};
use crate::footer::{Footer, footer};
use crate::icon::{IconState, shape};
use crate::input::{Cause, Connection, Input};
use crate::menu::{Menu, menu};
use crate::pools::{PoolCard, cards};
use crate::settings::{self, SettingsTab};
use crate::state::{Context, ImageStatus, Resident, Stop};
use crate::summary::{Summary, summary};
use crate::text::LOG_FOLDER;
use crate::tokens::{self, TokensTab};

/// the tab the window opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    Pools,
    Tokens,
    Settings,
}

/// the whole window saying why Marshal does not control the runners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FullWindow {
    pub heading: String,
    pub lines: Vec<String>,
    pub action: Option<Control>,
    pub announcement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Header {
    pub heading: String,
    pub line: String,
    /// Start All, Stop All and, while the image needs building, Build Image.
    pub controls: Vec<Control>,
    /// the tab the window opens on: Tokens while no repository has a token.
    pub opens_on: Tab,
    /// the chip on the Tokens tab.
    pub tokens_chip: Option<String>,
    /// the chip on the Settings tab.
    pub settings_chip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListWindow {
    pub header: Header,
    /// the first items of the scrolling list, over both tabs.
    pub banners: Vec<Banner>,
    pub pools: Vec<PoolCard>,
    /// what stands where the pools would be when there are none.
    pub pools_note: Option<String>,
    pub tokens: TokensTab,
    pub settings: SettingsTab,
    /// whether the list is the last one heard: its words take the low-emphasis colour.
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    Full(FullWindow),
    List(ListWindow),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Window {
    pub body: Body,
    pub footer: Footer,
}

/// something that needs the user's eye, which the live region says when it appears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attention {
    pub key: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct View {
    pub icon: IconState,
    pub tooltip: String,
    pub summary: Summary,
    pub menu: Menu,
    pub window: Window,
    pub attention: Vec<Attention>,
    /// GitHub status's one-time announcements, which the live region says once each,
    /// in order, at least 5 s apart.
    pub github_attention: Vec<Attention>,
    /// the request budget's level, while the window shows GitHub status.
    pub github_level: Option<String>,
}

const LOCK_HELD: &str = "Another Marshal process holds the runner lock, and Marshal cannot tell which: one that is starting or ending, an older version, or another Windows user's. Marshal checks again every 5 s. If it stays, look in Task Manager for marshal.exe or marshal-app.exe, or sign the other user out.";

fn full_window(context: &Context<'_>) -> FullWindow {
    let try_again = || control(context, Action::TryAgain, "Try Again", Look::Solid, None);
    let (heading, lines, action): (&str, Vec<String>, Option<Control>) = match &context.resident {
        Resident::NoAgent
        | Resident::LostContact {
            cause: Cause::RestartFailed,
            ..
        } => (
            "Marshal cannot reach its agent",
            vec![format!(
                "The agent did not start, or stopped answering and did not come back after one restart. The log in {LOG_FOLDER} says why."
            )],
            Some(try_again()),
        ),
        Resident::LostContact {
            cause: Cause::Busy, ..
        } => (
            "Marshal cannot reach its agent",
            vec!["The agent is busy. Marshal tries again every 5 s.".to_string()],
            Some(try_again()),
        ),
        Resident::NotAnswering => (
            "The agent is not answering",
            vec![
                "Marshal's agent did not answer within 5 s, and its runners may still be running."
                    .to_string(),
                "End marshal-app.exe in Task Manager, then open Marshal again; the next start removes the containers it left."
                    .to_string(),
            ],
            Some(try_again()),
        ),
        Resident::LockHeld => ("Another Marshal is running", vec![LOCK_HELD.to_string()], None),
        Resident::Quitting => (
            "Quitting Marshal",
            vec!["The window closes with the app.".to_string()],
            None,
        ),
        _ => (
            "Starting Marshal",
            vec!["Connecting to the agent.".to_string()],
            None,
        ),
    };
    let lead = lines.first().cloned().unwrap_or_default();
    FullWindow {
        heading: heading.to_string(),
        announcement: format!("{heading}. {lead}"),
        lines,
        action,
    }
}

/// whether the header offers Build Image: the image is missing, being built, failed or
/// out of date.
fn header_offers_build(context: &Context<'_>) -> bool {
    matches!(context.resident, Resident::Building { .. })
        || matches!(context.resident, Resident::Stopped(Stop::ImageBuildFailed))
        || matches!(
            context.image,
            ImageStatus::Missing | ImageStatus::Mismatched | ImageStatus::OutOfDate
        )
}

fn header(
    context: &Context<'_>,
    summary: &Summary,
    banners: &[Banner],
    tokens: &TokensTab,
    settings: &SettingsTab,
) -> Header {
    let banner_leads = banners.iter().any(|banner| {
        banner.action.as_ref().is_some_and(|action| {
            action.available && matches!(action.action, Action::BuildImage | Action::TryAgain)
        })
    });
    let start_look = if banner_leads {
        Look::Neutral
    } else {
        Look::Solid
    };
    let mut controls = vec![
        control(context, Action::StartAll, "Start All", start_look, None),
        control(
            context,
            Action::StopAll,
            "Stop All",
            Look::Destructive,
            confirmation(context, &Action::StopAll),
        ),
    ];
    if header_offers_build(context) {
        controls.push(control(
            context,
            Action::BuildImage,
            "Build Image",
            Look::Tonal,
            None,
        ));
    }
    Header {
        heading: summary.heading.clone(),
        line: summary.line.clone(),
        controls,
        opens_on: if tokens::none_has_a_token(context) {
            Tab::Tokens
        } else {
            Tab::Pools
        },
        tokens_chip: tokens.chip.clone(),
        settings_chip: settings.chip.clone(),
    }
}

fn pools_note(context: &Context<'_>) -> Option<String> {
    if !context.pools.is_empty() {
        return None;
    }
    match &context.resident {
        Resident::Stopped(Stop::NoConfiguration) => {
            Some("No pools are listed while there is no configuration.".to_string())
        }
        Resident::Stopped(Stop::InvalidConfiguration) => {
            Some("No pools are listed while the configuration cannot be read.".to_string())
        }
        _ => None,
    }
}

fn attention(cards: &[PoolCard]) -> Vec<Attention> {
    let mut found = Vec::new();
    for card in cards {
        let skipped = card.state == "Skipped" && card.mark.is_some();
        let rejected = card
            .pill
            .as_ref()
            .is_some_and(|pill| matches!(pill.action, Action::ReplaceToken(_)));
        if let Some(note) = card.note.as_ref().filter(|_| skipped || rejected) {
            found.push(Attention {
                key: format!("pool:{}:{note}", card.name),
                text: format!("Pool {}: {note}", card.name),
            });
        }
        for item in card
            .diagnostics
            .iter()
            .flat_map(|section| &section.items)
            .filter(|item| item.problem)
        {
            found.push(Attention {
                key: format!("diagnostics:{}:{}:{}", card.name, item.container, item.text),
                text: format!("Pool {}: {}: {}", card.name, item.container, item.text),
            });
        }
    }
    found
}

fn github_attention(context: &Context<'_>, cards: &[PoolCard]) -> Vec<Attention> {
    let mut found: Vec<Attention> = crate::github::feed_attention(context).into_iter().collect();
    for item in cards
        .iter()
        .flat_map(|card| &card.github_notes)
        .filter_map(|note| note.attention.clone())
    {
        if !found.iter().any(|each| each.key == item.key) {
            found.push(item);
        }
    }
    found
}

/// the view for `input`.
#[must_use]
pub fn build(input: &Input<'_>) -> View {
    let context = Context::new(input);
    let summary = summary(&context);
    let footer = footer(&context);
    let menu = menu(&context, &summary.menu_line);
    let icon = shape(&context);
    let tooltip = summary.tooltip.clone();
    if !context.has_list() {
        return View {
            icon,
            tooltip,
            menu,
            window: Window {
                body: Body::Full(full_window(&context)),
                footer,
            },
            summary,
            attention: Vec::new(),
            github_attention: Vec::new(),
            github_level: None,
        };
    }
    let banners = banners(&context);
    let tokens = tokens::tab(&context);
    let settings = settings::tab(&context);
    let header = header(&context, &summary, &banners, &tokens, &settings);
    let pools = cards(&context);
    View {
        icon,
        tooltip,
        menu,
        attention: attention(&pools),
        github_attention: github_attention(&context, &pools),
        github_level: crate::github::level(&context),
        window: Window {
            body: Body::List(ListWindow {
                header,
                banners,
                pools_note: pools_note(&context),
                pools,
                tokens,
                settings,
                stale: context.is_stale(),
            }),
            footer,
        },
        summary,
    }
}

impl View {
    /// the banners of the list, or none while the whole window speaks.
    #[must_use]
    pub fn banners(&self) -> &[Banner] {
        match &self.window.body {
            Body::List(list) => &list.banners,
            Body::Full(_) => &[],
        }
    }

    /// the list of the window, when it shows one.
    #[must_use]
    pub fn list(&self) -> Option<&ListWindow> {
        match &self.window.body {
            Body::List(list) => Some(list),
            Body::Full(_) => None,
        }
    }

    /// the full-window state, when the whole window speaks.
    #[must_use]
    pub fn full(&self) -> Option<&FullWindow> {
        match &self.window.body {
            Body::Full(full) => Some(full),
            Body::List(_) => None,
        }
    }
}

/// the view of a snapshot alone, as the agent's latest and connected: for a caller that
/// has no more than a snapshot to show.
fn of_snapshot(snapshot: Option<&Snapshot>) -> View {
    let connection = if snapshot.is_some() {
        Connection::Connected
    } else {
        Connection::StartingMarshal
    };
    build(&Input::new(connection, snapshot, Clock::new(0, 0)))
}

/// the icon's state for a snapshot alone.
#[must_use]
pub fn icon_state(snapshot: Option<&Snapshot>) -> IconState {
    of_snapshot(snapshot).icon
}

/// the icon's tooltip for a snapshot alone.
#[must_use]
pub fn tooltip(snapshot: Option<&Snapshot>) -> String {
    of_snapshot(snapshot).tooltip
}
