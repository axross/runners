//! what the window's one polite live region says: a banner or a full-window state when
//! it appears, anything that needs the user's eye when it starts, the settled summary at
//! most once per 5 s, and GitHub status's one-time announcements, queued and written one
//! at a time at least 5 s after the last write. A pure state machine over the time
//! passed in.

use std::collections::{BTreeSet, VecDeque};

use crate::view::{Attention, Body, View};

/// the least time between two announcements of the summary.
pub const SUMMARY_INTERVAL_MS: u64 = 5_000;

/// what to write to the live region now, and when to look again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Announced {
    pub messages: Vec<String>,
    /// when a held summary may be announced or the settled count may fall, in
    /// milliseconds since the Unix epoch; the caller updates again then.
    pub recheck_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Announcer {
    started: bool,
    /// the banners and full-window states announced, so one is announced once while it
    /// shows.
    shown: BTreeSet<String>,
    attention: BTreeSet<String>,
    summary: Option<String>,
    summary_at_ms: Option<u64>,
    /// GitHub's one-time announcements made, by key, so each is made once while it shows.
    github: BTreeSet<String>,
    /// the request budget's level last seen.
    level: Option<String>,
    /// GitHub's announcements waiting their turn, in order; none is dropped or merged.
    queue: VecDeque<String>,
    /// when the region was last written to.
    written_at_ms: Option<u64>,
}

/// what the live region says when the budget's level changes to `level` from `before`.
fn level_change(before: Option<&str>, level: Option<&str>) -> Option<String> {
    match (before, level?) {
        (Some("slowed" | "paused"), "normal") => {
            Some("GitHub requests: checked every 15 s".to_string())
        }
        (before, "slowed") if before != Some("slowed") => {
            Some("GitHub requests: checks slowed to once a minute to save the budget".to_string())
        }
        // a pause is said by its banner
        _ => None,
    }
}

/// the banners or the full-window state `view` shows, keyed by what the live region
/// says of them, so a banner whose lines change with time, like a build's elapsed time,
/// is still one banner, and one whose cause changes under the same heading is a new one.
fn spoken(view: &View) -> Vec<(String, String)> {
    match &view.window.body {
        Body::Full(full) => vec![(
            format!("full:{}", full.announcement),
            full.announcement.clone(),
        )],
        Body::List(list) => list
            .banners
            .iter()
            .map(|banner| {
                (
                    format!(
                        "{:?}:{}:{}",
                        banner.kind, banner.heading, banner.announcement
                    ),
                    banner.announcement.clone(),
                )
            })
            .collect(),
    }
}

impl Announcer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// notes the view at `now_ms` and returns what to announce. The first view is the
    /// baseline: what it shows is read with the page, not announced.
    pub fn update(&mut self, view: &View, now_ms: u64) -> Announced {
        let spoken = spoken(view);
        let attention: Vec<&Attention> = view.attention.iter().collect();
        if !self.started {
            self.started = true;
            self.shown = spoken.into_iter().map(|(key, _)| key).collect();
            self.attention = attention.iter().map(|item| item.key.clone()).collect();
            self.summary = Some(view.summary.spoken.clone());
            self.github = view
                .github_attention
                .iter()
                .map(|item| item.key.clone())
                .collect();
            self.level = view.github_level.clone();
            return Announced::default();
        }
        let mut messages = Vec::new();
        let now_showing: BTreeSet<String> = spoken.iter().map(|(key, _)| key.clone()).collect();
        for (key, text) in &spoken {
            if !self.shown.contains(key) {
                messages.push(text.clone());
            }
        }
        self.shown = now_showing;
        let now_attending: BTreeSet<String> =
            attention.iter().map(|item| item.key.clone()).collect();
        for item in &attention {
            if !self.attention.contains(&item.key) {
                messages.push(item.text.clone());
            }
        }
        self.attention = now_attending;
        let spoke = !messages.is_empty();
        let summary = &view.summary.spoken;
        let mut recheck_at_ms = None;
        if self.summary.as_ref() != Some(summary) {
            let due = self
                .summary_at_ms
                .map_or(now_ms, |at| at.saturating_add(SUMMARY_INTERVAL_MS));
            if spoke {
                // the banner already says it, so the summary is not said twice
                self.summary = Some(summary.clone());
                self.summary_at_ms = Some(now_ms);
            } else if now_ms >= due {
                messages.push(summary.clone());
                self.summary = Some(summary.clone());
                self.summary_at_ms = Some(now_ms);
            } else {
                recheck_at_ms = Some(due);
            }
        }
        self.queue_github(view);
        if !messages.is_empty() {
            self.written_at_ms = Some(now_ms);
        }
        if !self.queue.is_empty() {
            let due = self
                .written_at_ms
                .map_or(now_ms, |at| at.saturating_add(SUMMARY_INTERVAL_MS));
            let next_at = if now_ms >= due {
                messages.extend(self.queue.pop_front());
                self.written_at_ms = Some(now_ms);
                (!self.queue.is_empty()).then(|| now_ms.saturating_add(SUMMARY_INTERVAL_MS))
            } else {
                Some(due)
            };
            if let Some(at) = next_at {
                recheck_at_ms = Some(recheck_at_ms.map_or(at, |due: u64| due.min(at)));
            }
        }
        Announced {
            messages,
            recheck_at_ms,
        }
    }

    /// puts GitHub's new one-time announcements, and a change of level, in the queue.
    fn queue_github(&mut self, view: &View) {
        let level = view.github_level.clone();
        if level.is_some() && level != self.level {
            self.queue
                .extend(level_change(self.level.as_deref(), level.as_deref()));
        }
        // while the window shows no GitHub status the level last seen stays
        if level.is_some() {
            self.level = level;
        }
        let now_showing: BTreeSet<String> = view
            .github_attention
            .iter()
            .map(|item| item.key.clone())
            .collect();
        for item in &view.github_attention {
            if !self.github.contains(&item.key) {
                self.queue.push_back(item.text.clone());
            }
        }
        self.github = now_showing;
    }
}
