//! what the window, the menu and the notification-area icon show for an agent
//! snapshot; it depends on no operating system, so its tests run everywhere.
//!
//! [`build`] turns an [`Input`], the agent's latest snapshot with the app's own state,
//! into a [`View`]. [`Tracker`] keeps what changes with time: the settled count of
//! runners up and what the live region has said.

mod announce;
mod banner;
mod clock;
mod confirm;
mod control;
mod diagnostics;
mod footer;
mod github;
mod host;
mod icon;
mod input;
mod menu;
mod pools;
mod settled;
mod state;
mod summary;
mod text;
mod tokens;
mod tracker;
mod view;

pub use announce::{Announced, Announcer, SUMMARY_INTERVAL_MS};
pub use banner::{Banner, BannerKind, Tone};
pub use clock::{Clock, elapsed};
pub use control::{Action, Confirmation, Control, Look};
pub use diagnostics::{DiagnosticItem, Diagnostics};
pub use footer::Footer;
pub use github::{GithubNote, HIDDEN_NAMES, OpenRun, job_page};
pub use host::{HOST, HostWords, MACOS, WINDOWS};
pub use icon::{GlyphColour, IconState, Rgb, TaskbarMode};
pub use input::{
    Cause, Connection, GithubFeed, Input, NotSaved, Pending, PendingKind, TokenResult, TokenUi,
};
pub use menu::{Menu, MenuItem};
pub use pools::{Mark, PoolCard, RunnerRow};
pub use settled::{FALL_DELAY_MS, Settled};
pub use summary::Summary;
pub use text::{DEFAULT_CONFIGURATION, LOG_FOLDER, plain};
pub use tokens::{
    TokenForm, TokenRow, TokenState, TokensTab, discarded_announcement, pools_started_by,
};
pub use tracker::{Tracker, Update};
pub use view::{
    Attention, Body, FullWindow, Header, ListWindow, Tab, View, Window, build, icon_state, tooltip,
};
