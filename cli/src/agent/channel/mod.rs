//! the agent's side of the control channel: today's pipe or socket, with today's
//! access rules, serving protocol version 2 on the agent's own runtime; the pipe's
//! descriptor grants only the current user and rejects remote clients, and the
//! socket is `0600` in a `0700` directory with each peer's user ID checked;
//! `docs/operations/cli.md` describes both.

mod client;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};

use crate::instance::channel::ChannelError;
use crate::instance::lock::InstanceLock;
use crate::instance::roots::Roots;

use super::api::Call;
use super::server::{Counts, Limits, Shared, Watchers, serve};
use super::snapshot::Snapshot;
use super::wire::{EndingNotice, UpdateEnding};
use crate::github_status::GithubStatus;

/// the bare stream to the agent, for the tests of the channel's limits.
#[doc(hidden)]
pub use client::platform::{Stream as RawStream, connect as connect_raw};
pub use client::{CallError, Client, ConnectError, Push, Welcome, connect};

#[cfg(unix)]
use unix::{Listener, split as platform_split};
#[cfg(windows)]
use windows::{Listener, split as platform_split};

/// the open channel; dropping it closes it at once.
pub struct AgentChannel {
    accepting: JoinHandle<()>,
    ending: watch::Sender<Option<EndingNotice>>,
    counts: Arc<Counts>,
    address: String,
    github: Option<GithubFeed>,
}

/// what the collector of GitHub status needs from the channel: where it publishes
/// the `github` topic's value, and whether a renewed subscription wants it.
pub struct GithubFeed {
    pub published: watch::Sender<Arc<GithubStatus>>,
    pub active: watch::Receiver<bool>,
}

impl Drop for AgentChannel {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

impl AgentChannel {
    /// opens the channel, holding the lock, and serves every connection on the
    /// current runtime; every error is a startup failure.
    pub fn open(
        _lock: &InstanceLock,
        roots: &Roots,
        calls: mpsc::UnboundedSender<Call>,
        snapshots: watch::Receiver<Arc<Snapshot>>,
        limits: Limits,
    ) -> Result<Self, ChannelError> {
        Self::start(Listener::open(roots)?, calls, snapshots, limits)
    }

    /// like [`open`](Self::open), but accepts only peers whose user ID is `peer_uid`,
    /// to show that a peer of another user is turned away; a debug build only.
    #[cfg(all(unix, debug_assertions))]
    #[doc(hidden)]
    pub fn open_for_peer(
        _lock: &InstanceLock,
        roots: &Roots,
        calls: mpsc::UnboundedSender<Call>,
        snapshots: watch::Receiver<Arc<Snapshot>>,
        limits: Limits,
        peer_uid: u32,
    ) -> Result<Self, ChannelError> {
        Self::start(
            Listener::open_accepting(roots, peer_uid)?,
            calls,
            snapshots,
            limits,
        )
    }

    fn start(
        listener: Listener,
        calls: mpsc::UnboundedSender<Call>,
        snapshots: watch::Receiver<Arc<Snapshot>>,
        limits: Limits,
    ) -> Result<Self, ChannelError> {
        let address = listener.address();
        let (ending, ending_receiver) = watch::channel(None);
        let counts = Arc::new(Counts::default());
        let (published, github) = watch::channel(Arc::new(GithubStatus::default()));
        let (watchers, active) = Watchers::new();
        let shared = Arc::new(Shared {
            calls,
            snapshots,
            github,
            watchers,
            ending: ending_receiver,
            limits,
            counts: Arc::clone(&counts),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
        });
        let accepting = tokio::spawn(accept(listener, shared));
        Ok(Self {
            accepting,
            ending,
            counts,
            address,
            github: Some(GithubFeed { published, active }),
        })
    }

    /// the feed of GitHub status, once.
    pub fn github_feed(&mut self) -> Option<GithubFeed> {
        self.github.take()
    }

    /// where clients reach the channel.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// tells every connection the agent is ending for `reason`, with `update` when it
    /// ends for an update, stops accepting, and
    /// waits up to `within` for the connections to end, so the message reaches a
    /// client that reads; a connection that has not ended then is dropped, so a
    /// client that never reads cannot hold the agent.
    pub async fn close(mut self, reason: &str, update: Option<UpdateEnding>, within: Duration) {
        self.ending.send_replace(Some(EndingNotice {
            reason: reason.to_string(),
            update,
        }));
        let deadline = tokio::time::Instant::now() + within;
        while self.counts.all() > 0 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // the listener goes with its task, and the socket file with the listener
        self.accepting.abort();
        let _ = (&mut self.accepting).await;
    }
}

/// how long the accept loop waits after the operating system refuses a connection.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

async fn accept(mut listener: Listener, shared: Arc<Shared>) {
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        let stream = match listener.accept().await {
            Ok(stream) => stream,
            Err(_) => {
                tokio::time::sleep(ACCEPT_BACKOFF).await;
                continue;
            }
        };
        let Some(admission) = shared.counts.admit(&shared.limits) else {
            continue;
        };
        let (read, write) = platform_split(stream);
        connections.spawn(serve(read, write, Arc::clone(&shared), admission));
    }
}
