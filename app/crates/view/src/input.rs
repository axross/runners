//! what the view is computed from: the latest snapshot from the agent and the app's
//! own state, which no snapshot carries.

use std::collections::BTreeMap;

use marshal_cli::agent::snapshot::Snapshot;

use crate::clock::Clock;

/// why the app lost contact with a running agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// the connection dropped, and Marshal is starting the agent again, once.
    Dropped,
    /// Marshal started the agent again and it did not answer.
    RestartFailed,
    /// the agent answered `busy`; Marshal tries again every 5 s.
    Busy,
}

/// the app's connection to its agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// Marshal is starting, or connecting to its agent.
    StartingMarshal,
    /// the snapshot is the agent's latest.
    Connected,
    /// the connection ended; the last snapshot stays, with when it was last heard.
    LostContact { last_heard_ms: u64, cause: Cause },
    /// no agent started, or none came back after the one restart.
    NoAgent,
    /// the channel exists, and the agent did not complete the handshake in 5 s.
    NotAnswering,
    /// the lock is held and the user's channel does not answer protocol version 2.
    LockHeld,
    /// the app is ending.
    Quitting,
}

/// an action the app sent and the snapshot does not show yet, so the view shows its
/// transition at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKind {
    StopAll,
    StartAll,
    Build,
    StopBuild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    pub kind: PendingKind,
    /// when the user chose it, in milliseconds since the Unix epoch.
    pub since_ms: u64,
}

/// why a token was not saved; none of these holds a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotSaved {
    /// the token was over 1,280 UTF-16 code units.
    TooLong,
    /// the store refused it; `detail` is the platform's own words, when it gave any.
    Store { detail: Option<String> },
    /// the agent refused it, in its own words.
    Refused(String),
    /// contact with the agent was lost.
    LostContact,
}

/// what the latest save or removal of a repository's token came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenResult {
    /// saved; `started` names the pools the save started, see
    /// [`crate::tokens::pools_started_by`].
    Saved {
        started: Vec<String>,
    },
    Removed,
    NotSaved(NotSaved),
    /// a removal that failed, with why; the token is still stored.
    NotRemoved(String),
    /// a save or a removal the agent did not answer within the call's limit; it may
    /// still do it, so the snapshot says where the token stands.
    NotAnswered {
        removal: bool,
    },
}

/// the token forms as the page has them open. The view never holds a token or what is
/// typed: the page keeps that in its field and clears it when it submits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUi {
    /// the repository whose field is open.
    pub editing: Option<String>,
    /// the repository whose removal asks for confirmation.
    pub removing: Option<String>,
    /// the latest result for each repository.
    pub results: BTreeMap<String, TokenResult>,
}

/// everything one view is computed from.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub connection: Connection,
    /// the agent's latest snapshot, kept while contact is lost.
    pub snapshot: Option<&'a Snapshot>,
    pub pending: Option<Pending>,
    pub clock: Clock,
    /// the settled count of runners up, from [`crate::Settled`]; without it the
    /// snapshot's count is shown.
    pub settled_up: Option<u32>,
    pub tokens: Option<&'a TokenUi>,
}

impl<'a> Input<'a> {
    #[must_use]
    pub fn new(connection: Connection, snapshot: Option<&'a Snapshot>, clock: Clock) -> Self {
        Self {
            connection,
            snapshot,
            pending: None,
            clock,
            settled_up: None,
            tokens: None,
        }
    }

    #[must_use]
    pub fn pending(mut self, pending: Option<Pending>) -> Self {
        self.pending = pending;
        self
    }

    #[must_use]
    pub fn settled(mut self, up: u32) -> Self {
        self.settled_up = Some(up);
        self
    }

    #[must_use]
    pub fn tokens(mut self, tokens: &'a TokenUi) -> Self {
        self.tokens = Some(tokens);
        self
    }
}
