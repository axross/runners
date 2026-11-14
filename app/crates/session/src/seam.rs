//! what the session needs from the world, as traits, so its tests run it against fakes
//! and a clock that jumps: the connection to the agent, the start of the agent, the
//! time, the log, the screen, and for updates the feed, the agent's process and the
//! record kept across an update.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::Launch;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::wire::UpdateEnding;
use marshal_cli::github_status::GithubStatus;
use marshal_view::{Clock, View};
use serde::Serialize;
use tokio::sync::mpsc;

/// what the session asks of a connected agent.
#[derive(Debug)]
pub enum Method {
    /// what a fresh launch sends: the agent starts the runners as
    /// `start_runners_on_launch` says.
    Launch,
    StartAll,
    StopAll,
    Build,
    Quit,
    /// the token travels only here, as a [`Secret`](marshal_cli::runtime::secret::Secret)
    /// whose `Debug` hides it.
    TokenSet {
        repository: String,
        token: marshal_cli::runtime::secret::Secret,
    },
    TokenRemove {
        repository: String,
    },
}

/// why a call got no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// the agent answered `busy`.
    Busy(String),
    /// the token store failed; `detail` is its kind and code, when the agent gave them.
    Store {
        detail: Option<String>,
    },
    /// the agent refused, or failed, in its own words.
    Refused(String),
    /// the connection ended.
    Lost,
    /// the call took longer than it may on a connection that is still open, so the
    /// agent may yet do it.
    TimedOut,
    /// the agent does not serve the method, or does not list the topic.
    NotServed(String),
    /// the agent ended this connection's `github` subscription for want of renewal.
    NotSubscribed,
    Other(String),
}

impl Failure {
    /// the words the page is given, which hold no token.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Self::Busy(_) => "The agent is busy. Try again in a moment.".to_string(),
            Self::Store { .. } => "The token store refused the request.".to_string(),
            Self::Refused(words) | Self::Other(words) => marshal_view::plain(words),
            Self::Lost => "Marshal has lost contact with the agent.".to_string(),
            Self::TimedOut => {
                "The agent has not answered yet. The list shows what it is doing.".to_string()
            }
            Self::NotServed(method) => {
                format!("This agent does not offer {}.", marshal_view::plain(method))
            }
            Self::NotSubscribed => "The agent ended the GitHub subscription.".to_string(),
        }
    }
}

/// why no connection was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialError {
    /// nothing answers at the channel.
    NoAgent,
    /// the channel exists and the agent did not complete the handshake in 5 s.
    NotAnswering,
    /// the agent answered `busy`.
    Busy,
    /// the lock is held and the channel does not answer protocol version 2, or answers
    /// as another user's process.
    LockHeld,
    Failed(String),
}

/// what the agent sends unasked, as the session reads it.
#[derive(Debug)]
pub enum Pushed {
    Snapshot(Box<Snapshot>),
    /// the `github` topic's value, whole.
    Github(Box<GithubStatus>),
    Heartbeat,
    /// the agent is about to end, for this reason (an open set), with what an end for an
    /// update adds.
    Ending {
        reason: String,
        update: Option<UpdateEnding>,
    },
    /// the connection ended.
    Closed,
}

/// one live connection to the agent.
pub trait Link: Send + Sync + 'static {
    fn subscribe(&self) -> impl Future<Output = Result<(), Failure>> + Send;
    /// subscribes this connection to the `github` topic; an agent that does not list the
    /// topic answers [`Failure::NotServed`], and one with no room [`Failure::Busy`].
    fn subscribe_github(&self) -> impl Future<Output = Result<(), Failure>> + Send;
    /// renews this connection's `github` subscription; [`Failure::NotSubscribed`] when
    /// the agent has ended it.
    fn renew_github(&self) -> impl Future<Output = Result<(), Failure>> + Send;
    fn call(&self, method: Method) -> impl Future<Output = Result<(), Failure>> + Send;
    /// asks the agent to drain for an update to `version`; the answer is the agent's
    /// process ID when it gave one.
    fn drain_for_update(
        &self,
        version: &str,
    ) -> impl Future<Output = Result<Option<u32>, Failure>> + Send;
}

/// what the agent said of itself in `welcome`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentInfo {
    /// the agent's own version.
    pub version: String,
    /// whether it lists `drain_for_update`.
    pub drains: bool,
}

/// a connection and what the agent sends on it.
pub struct Connected<L> {
    pub link: L,
    pub pushes: mpsc::UnboundedReceiver<Pushed>,
    pub agent: AgentInfo,
}

/// makes connections to this user's agent.
pub trait Dial: Send + Sync + 'static {
    type Link: Link;

    /// connects and completes the handshake; the library's client gives up after 5 s.
    fn dial(&self) -> impl Future<Output = Result<Connected<Self::Link>, DialError>> + Send;
}

/// starts the agent from the app's own executable.
pub trait Launcher: Send + Sync + 'static {
    /// starts the agent as a detached process with no console window, or says why not:
    /// a missing or changed executable is never started.
    fn start_agent(&self, launch: Launch) -> Result<(), String>;
}

/// the time, wall-clock and local.
pub trait TimeSource: Send + Sync + 'static {
    fn clock(&self) -> Clock;

    /// the time alone in milliseconds since the Unix epoch, for a caller that needs no
    /// local offset.
    fn now_ms(&self) -> u64 {
        self.clock().now_ms
    }
}

/// the app's log; no line ever holds a token.
pub trait Log: Send + Sync + 'static {
    fn info(&self, line: &str);
    fn warn(&self, line: &str);
}

/// what the windows, the menu and the icon are shown: the view, what the live region
/// says with it, and the view's revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shown {
    pub view: View,
    pub announce: Vec<String>,
    /// grows by one with each view the session shows, so the page can tell a view that
    /// is older than the one it has when the two reach it by different routes.
    pub revision: u64,
}

/// the screen: called from the session's task whenever what is shown changes.
pub trait Present: Send + Sync + 'static {
    fn present(&self, shown: &Shown);
}

/// a future a seam returns, which a trait object can name.
pub type Later<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// the update feed: what a check found, after its download verified.
pub trait Updater: Send + Sync + 'static {
    /// reads the feed and, for a release the app takes, downloads its installer and
    /// verifies it; `None` when there is no newer release. A check never installs.
    fn check(&self) -> Later<Result<Option<Box<dyn Verified>>, String>>;
}

/// a downloaded update whose signature verified, held until it is installed or dropped.
pub trait Verified: Send + 'static {
    /// the version it installs.
    fn version(&self) -> &str;

    /// starts its installer, which replaces Marshal and asks, through `AUTOLAUNCHAPP`,
    /// for the new app to start, which only an MSI that honours it does. On Windows the
    /// app's process ends here when the installer started, so `Ok` returns only to a
    /// test.
    fn install(self: Box<Self>) -> Result<(), String>;
}

/// the agent's process, which the app waits on before it installs.
pub trait Processes: Send + Sync + 'static {
    /// a handle to the process `pid`, or `None` when no such process runs.
    fn open(&self, pid: u32) -> Result<Option<Arc<dyn Process>>, String>;
}

/// a handle to a process, which stays the same process while the handle is open.
pub trait Process: Send + Sync + 'static {
    fn has_exited(&self) -> bool;

    /// waits at most `limit` for the process to exit; `true` when it has.
    fn wait(self: Arc<Self>, limit: Duration) -> Later<bool>;
}

/// where the app does not update itself: no process is watched and no record is kept.
pub struct NoUpdates;

impl Processes for NoUpdates {
    fn open(&self, _pid: u32) -> Result<Option<Arc<dyn Process>>, String> {
        Err("this build does not watch processes".to_string())
    }
}

impl Record for NoUpdates {
    fn write(&self, _version: &str, _at_ms: u64, _stopped: bool) -> Result<(), String> {
        Ok(())
    }

    fn remove(&self) {}
}

/// the one-shot record of how the runners stood when the agent ended for an update.
pub trait Record: Send + Sync + 'static {
    /// writes whether the runners stay stopped after the update to `version`: stopped
    /// by you or by a signal, rather than running.
    fn write(&self, version: &str, at_ms: u64, stopped: bool) -> Result<(), String>;
    fn remove(&self);
}
