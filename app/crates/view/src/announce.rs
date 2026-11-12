//! what the window's one polite live region says: a banner or a full-window state when
//! it appears, anything that needs the user's eye when it starts, and the settled
//! summary at most once per 5 s. A pure state machine over the time passed in.

use std::collections::BTreeSet;

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
}

/// the banners or the full-window state `view` shows, keyed so that a banner whose
/// words change with time, like a build's elapsed time, is still one banner.
fn spoken(view: &View) -> Vec<(String, String)> {
    match &view.window.body {
        Body::Full(full) => vec![(format!("full:{}", full.heading), full.announcement.clone())],
        Body::List(list) => list
            .banners
            .iter()
            .map(|banner| {
                (
                    format!("{:?}:{}", banner.kind, banner.heading),
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
            self.summary = Some(view.summary.menu_line.clone());
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
        let summary = &view.summary.menu_line;
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
        Announced {
            messages,
            recheck_at_ms,
        }
    }
}
