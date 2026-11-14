//! the session: one task that owns everything the app knows about its agent: the
//! connection and what it is doing, the latest snapshot, the actions and token
//! results the snapshot does not show yet, and the view computed from them. The
//! windows and the menu act on it through a [`Handle`], and it shows what it knows
//! through [`Present`](crate::seam::Present).
//!
//! The session connects at launch, starts the agent when none answers, keeps the last
//! list under lost contact, starts the agent once more, tries again on `busy` and on a
//! held lock every 5 s, and ends when the agent ends for `quit` or
//! `executable_changed`. Its [`update`] module checks the update feed and installs an
//! update once the agent has drained.
//!
//! While the main window is shown and not minimized it also holds the `github`
//! subscription, on a second connection of its own that it renews every 30 s and closes
//! when the window is hidden or minimized, so the agent asks GitHub only then.

mod update;

pub use update::{CHECK_EVERY, EXIT_WITHIN};

use std::collections::BTreeMap;
use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::wire::ENDING_UPDATE;
use marshal_cli::agent::{EXECUTABLE_CHANGED, Launch, QUIT, StopReason};
use marshal_cli::github_status::GithubStatus;
use marshal_cli::token_store::{TokenKey, TokenRefusal, accepted_token};
use marshal_view::{
    Cause, Connection, GithubFeed, Input, NotSaved, Pending, PendingKind, SettingResult,
    SettingsUi, TokenResult, TokenUi, Tracker, Update, View, job_page, pools_started_by,
    setting_label,
};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};

use crate::connect::{Attempt, Found, RETRY_EVERY, attempt};
use crate::seam::{
    AgentInfo, Connected, Dial, DialError, Failure, Launcher, Link, Log, Method, Present,
    Processes, Pushed, Record, Shown, TimeSource, Updater, Verified,
};
use update::Drain;

/// how often the view is computed again with no news, so a build's elapsed time and the
/// settled count move.
const TICK: Duration = Duration::from_secs(1);

/// the longest a stop, a start or a build call waits for its answer: the stop sequence
/// takes at most 30 s.
const CALL_WITHIN: Duration = Duration::from_secs(40);

/// the longest a token call waits for the Credential Manager.
const TOKEN_WITHIN: Duration = Duration::from_secs(10);

/// the longest `lifecycle_set` waits: one write of config.toml.
const SETTING_WITHIN: Duration = Duration::from_secs(10);

/// the longest `quit` waits: the stop sequence, then the agent ending.
const QUIT_WITHIN: Duration = Duration::from_secs(45);

/// how long the app waits for the agent's connection to end after `quit` answered, so
/// the agent is gone when the app is.
const QUIT_LINGER: Duration = Duration::from_secs(3);

/// an answered action whose snapshot never shows it stops being shown after this.
const PENDING_FOR: Duration = Duration::from_secs(5);

/// how often the `github` subscription is renewed; the agent ends one not renewed for
/// 90 s.
pub const RENEW_EVERY: Duration = Duration::from_secs(30);

/// how long a pool says its Open run found the job ended.
const ENDED_FOR_MS: u64 = 60_000;

/// how often a `github` subscription the agent had no room for is asked for again.
pub const GITHUB_ASK_AGAIN: Duration = Duration::from_secs(60);

/// the longest the `github` subscription or its renewal waits for its answer.
const GITHUB_WITHIN: Duration = Duration::from_secs(5);

/// why the session ended, and so the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// the user quit and the agent ended with the app.
    Quit,
    /// the agent ended for this reason: `quit` or `executable_changed`.
    AgentEnded(String),
    /// the user quit with no agent to quit: only the app ends.
    AppOnly,
    /// the agent ended for the update to this version and its installer started; on
    /// Windows the process has already ended by then, so only a test sees this.
    Installing(String),
}

/// the token forms the page has open, which only the page knows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Forms {
    pub editing: Option<String>,
    pub removing: Option<String>,
}

/// what an action of the window does to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    StartAll,
    StopAll,
    Build,
    StopBuild,
}

impl Act {
    fn pending(self) -> PendingKind {
        match self {
            Self::StartAll => PendingKind::StartAll,
            Self::StopAll => PendingKind::StopAll,
            Self::Build => PendingKind::Build,
            Self::StopBuild => PendingKind::StopBuild,
        }
    }

    /// stopping a build is the agent's `stop_all`, which cancels it.
    fn method(self) -> Method {
        match self {
            Self::StartAll => Method::StartAll,
            Self::StopAll | Self::StopBuild => Method::StopAll,
            Self::Build => Method::Build,
        }
    }
}

type Reply<T> = oneshot::Sender<Result<T, String>>;

enum Command {
    Read {
        forms: Option<Forms>,
        reply: oneshot::Sender<Shown>,
    },
    Act {
        act: Act,
        reply: Reply<()>,
    },
    TryAgain {
        reply: Reply<()>,
    },
    SetToken {
        repository: String,
        token: String,
        reply: Reply<()>,
    },
    RemoveToken {
        repository: String,
        reply: Reply<()>,
    },
    SetSetting {
        key: String,
        on: bool,
        reply: Reply<()>,
    },
    WindowShown(bool),
    OpenJob {
        pool: String,
        runner: String,
        reply: Reply<Option<String>>,
    },
    Quit,
}

/// what the windows and the menu ask of the session.
#[derive(Clone)]
pub struct Handle {
    commands: mpsc::UnboundedSender<Command>,
}

const GONE: &str = "Marshal is closing.";

impl Handle {
    async fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(command(reply))
            .map_err(|_| GONE.to_string())?;
        answer.await.map_err(|_| GONE.to_string())?
    }

    /// the view now. `forms`, when given, replaces which token forms the page has open.
    pub async fn read_view(&self, forms: Option<Forms>) -> Result<Shown, String> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(Command::Read { forms, reply })
            .map_err(|_| GONE.to_string())?;
        answer.await.map_err(|_| GONE.to_string())
    }

    pub async fn act(&self, act: Act) -> Result<(), String> {
        self.ask(|reply| Command::Act { act, reply }).await
    }

    pub async fn try_again(&self) -> Result<(), String> {
        self.ask(|reply| Command::TryAgain { reply }).await
    }

    /// stores a token entered in the window. The token is trimmed and checked here,
    /// sent once and dropped; what came of it is in the view.
    pub async fn set_token(&self, repository: String, token: String) -> Result<(), String> {
        self.ask(|reply| Command::SetToken {
            repository,
            token,
            reply,
        })
        .await
    }

    pub async fn remove_token(&self, repository: String) -> Result<(), String> {
        self.ask(|reply| Command::RemoveToken { repository, reply })
            .await
    }

    /// writes one `[lifecycle]` setting through the agent; what came of it is in the
    /// view.
    pub async fn set_setting(&self, key: String, on: bool) -> Result<(), String> {
        self.ask(|reply| Command::SetSetting { key, on, reply })
            .await
    }

    /// says whether the main window is shown and not minimized, which is when the
    /// session holds the `github` subscription.
    pub fn window_shown(&self, shown: bool) {
        let _ = self.commands.send(Command::WindowShown(shown));
    }

    /// the address of the job page of `pool`'s runner named `runner`, built by the
    /// session; `None` when that job has ended, which the pool's card then says. A pool
    /// the agent does not list is refused.
    pub async fn open_job(&self, pool: String, runner: String) -> Result<Option<String>, String> {
        self.ask(|reply| Command::OpenJob {
            pool,
            runner,
            reply,
        })
        .await
    }

    /// quits: the runners stop, then the agent and the app end; with no agent to reach
    /// only the app ends.
    pub fn quit(&self) {
        let _ = self.commands.send(Command::Quit);
    }
}

/// what the session's own tasks report back to it.
enum Internal<L> {
    Attempted {
        generation: u64,
        attempt: Attempt<L>,
    },
    Acted {
        since_ms: u64,
        outcome: Result<(), Failure>,
    },
    Token {
        repository: String,
        removal: bool,
        started: Vec<String>,
        outcome: Result<(), Failure>,
        reply: Reply<()>,
    },
    Setting {
        key: String,
        on: bool,
        outcome: Result<(), Failure>,
        reply: Reply<()>,
    },
    QuitAnswered(Result<(), Failure>),
    /// what a check of the update feed found.
    Checked(Result<Option<Box<dyn Verified>>, String>),
    /// the agent's answer to the drain this connection asked for: its process ID.
    DrainAnswered(Result<Option<u32>, Failure>),
    /// whether the agent's process exited after it ended for the update.
    AgentGone(bool),
    GithubAttempted {
        generation: u64,
        found: Result<Connected<L>, Failure>,
    },
    GithubRenewed {
        generation: u64,
        /// the agent had ended the subscription, and it was taken again.
        resubscribed: bool,
        outcome: Result<(), Failure>,
    },
}

/// where the `github` subscription stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Feed {
    /// the window is hidden or minimized.
    Off,
    /// the agent does not list the topic; so until the app reaches another agent.
    Unavailable,
    /// the agent had no room; asked again every 60 s.
    ForNow,
    /// held, or being asked for, since `opened_ms`.
    Live { opened_ms: u64 },
}

/// the second connection, which holds the `github` subscription.
struct Github<L> {
    feed: Feed,
    live: Option<Live<L>>,
    attempt: Option<JoinHandle<()>>,
    /// tells the reports of an attempt or a renewal that was given up from current ones.
    generation: u64,
    retry_at: Option<Instant>,
    renew_at: Option<Instant>,
    status: Option<GithubStatus>,
}

impl<L> Github<L> {
    fn new() -> Self {
        Self {
            feed: Feed::Off,
            live: None,
            attempt: None,
            generation: 0,
            retry_at: None,
            renew_at: None,
            status: None,
        }
    }

    /// closes the connection, which ends the subscription, and gives up what is under way.
    fn close(&mut self) {
        if let Some(task) = self.attempt.take() {
            task.abort();
        }
        self.live = None;
        self.generation += 1;
        self.retry_at = None;
        self.renew_at = None;
    }
}

enum Stage {
    /// an attempt to reach the agent is under way.
    Attempting,
    Live,
    /// asking again at `retry_at`: the agent was busy, or the lock is held.
    Waiting,
    /// nothing more happens until the user tries again.
    Parked,
    Quitting,
    /// the agent ended for this app's update; the app waits for its process to exit,
    /// then installs.
    Ending,
}

struct Live<L> {
    link: Arc<L>,
    pushes: mpsc::UnboundedReceiver<Pushed>,
}

pub struct Seams<D> {
    pub dial: D,
    pub launcher: Arc<dyn Launcher>,
    pub present: Arc<dyn Present>,
    pub time: Arc<dyn TimeSource>,
    pub log: Arc<dyn Log>,
    /// the app's own version.
    pub version: String,
    /// the version of the agent this app starts, which the library it links gives; an
    /// agent older than it is replaced.
    pub agent_version: String,
    /// the update feed; `None` where the app does not update itself.
    pub updater: Option<Arc<dyn Updater>>,
    pub processes: Arc<dyn Processes>,
    pub record: Arc<dyn Record>,
    /// how a launch that starts the agent starts it: as the record kept across an update
    /// says, or stopped for a fresh launch.
    pub launch: Launch,
    /// a fresh launch: an agent the first attempt starts is sent `launch`, which starts
    /// the runners as `start_runners_on_launch` says.
    pub fresh: bool,
}

pub struct Session<D: Dial> {
    dial: Arc<D>,
    launcher: Arc<dyn Launcher>,
    present: Arc<dyn Present>,
    time: Arc<dyn TimeSource>,
    log: Arc<dyn Log>,
    commands: mpsc::UnboundedReceiver<Command>,
    internal: mpsc::UnboundedReceiver<Internal<D::Link>>,
    report: mpsc::UnboundedSender<Internal<D::Link>>,
    stage: Stage,
    live: Option<Live<D::Link>>,
    attempt: Option<JoinHandle<()>>,
    generation: u64,
    /// how many times the agent may still be started: once at launch, once after a
    /// drop, once for each Try Again.
    starts_left: u32,
    retry_at: Option<Instant>,
    end_by: Option<Instant>,
    /// the user quit while an attempt to reach the agent was under way. The attempt may
    /// have started the agent, so it is quit once the attempt connects, and with none to
    /// reach the app ends alone.
    quit_when_reached: bool,
    next_tick: Instant,
    recheck: Option<Instant>,
    ended: Option<Ended>,
    connection: Connection,
    snapshot: Option<Snapshot>,
    last_heard_ms: Option<u64>,
    pending: Option<Pending>,
    answered: Option<Instant>,
    tokens: TokenUi,
    settings: SettingsUi,
    announce: Vec<String>,
    tracker: Tracker,
    shown: Option<View>,
    /// the revision of `shown`, which counts the views the session has shown.
    revision: u64,
    version: String,
    agent_version: String,
    updater: Option<Arc<dyn Updater>>,
    processes: Arc<dyn Processes>,
    update_record: Arc<dyn Record>,
    /// the mode the next start of the agent uses instead of the one the last state
    /// gives: the launch's own, or the replaced older agent's.
    next_launch: Option<Launch>,
    /// whether an agent this launch starts is sent `launch` once connected.
    send_launch: bool,
    /// whether the agent this fresh launch started still waits for `launch`.
    launch_owed: bool,
    /// what the connected agent said of itself.
    agent: AgentInfo,
    next_check: Option<Instant>,
    checking: bool,
    /// the update downloaded and verified, until it installs or is dropped.
    verified: Option<Box<dyn Verified>>,
    /// the drain this app asked for.
    drain: Option<Drain>,
    /// when a drain the agent did not take is asked for again.
    drain_again_at: Option<Instant>,
    /// an agent that does not drain was logged once.
    told_no_drain: bool,
    /// whether the main window is shown and not minimized.
    window_shown: bool,
    github: Github<D::Link>,
    /// the pools whose Open run found its job ended, with when.
    ended_jobs: BTreeMap<String, u64>,
}

impl<D: Dial> Session<D> {
    #[must_use]
    pub fn new(seams: Seams<D>) -> (Self, Handle) {
        let (commands_sender, commands) = mpsc::unbounded_channel();
        let (report, internal) = mpsc::unbounded_channel();
        let session = Self {
            dial: Arc::new(seams.dial),
            launcher: seams.launcher,
            present: seams.present,
            time: seams.time,
            log: seams.log,
            commands,
            internal,
            report,
            stage: Stage::Attempting,
            live: None,
            attempt: None,
            generation: 0,
            starts_left: 1,
            retry_at: None,
            end_by: None,
            quit_when_reached: false,
            next_tick: Instant::now() + TICK,
            recheck: None,
            ended: None,
            connection: Connection::StartingMarshal,
            snapshot: None,
            last_heard_ms: None,
            pending: None,
            answered: None,
            tokens: TokenUi::default(),
            settings: SettingsUi::default(),
            announce: Vec::new(),
            tracker: Tracker::new(),
            shown: None,
            revision: 0,
            version: seams.version,
            agent_version: seams.agent_version,
            updater: seams.updater,
            processes: seams.processes,
            update_record: seams.record,
            next_launch: Some(seams.launch),
            send_launch: seams.fresh,
            launch_owed: false,
            agent: AgentInfo::default(),
            next_check: None,
            checking: false,
            verified: None,
            drain: None,
            drain_again_at: None,
            told_no_drain: false,
            window_shown: false,
            github: Github::new(),
            ended_jobs: BTreeMap::new(),
        };
        (
            session,
            Handle {
                commands: commands_sender,
            },
        )
    }

    /// runs until the agent ends or the user quits; the caller then ends the app.
    pub async fn run(mut self) -> Ended {
        self.first_check();
        self.begin_attempt();
        self.refresh();
        loop {
            let wake = self.wake_at();
            tokio::select! {
                command = self.commands.recv() => match command {
                    Some(command) => self.command(command),
                    None => self.ended = Some(Ended::AppOnly),
                },
                Some(internal) = self.internal.recv() => self.internal(internal),
                pushed = next_push(&mut self.live) => self.pushed(pushed),
                pushed = next_push(&mut self.github.live) => self.github_pushed(pushed),
                () = sleep_until(wake) => self.woke(),
            }
            if let Some(ended) = self.ended.take() {
                self.abort_attempt();
                self.github.close();
                self.log.info(&format!("The session ended: {ended:?}."));
                return ended;
            }
        }
    }

    fn wake_at(&self) -> Instant {
        [
            Some(self.next_tick),
            self.recheck,
            self.end_by,
            self.retry_at,
            self.check_due(),
            self.drain_again_at,
            self.github.retry_at,
            self.github.renew_at,
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(self.next_tick)
    }

    fn now_ms(&self) -> u64 {
        self.time.now_ms()
    }

    fn link(&self) -> Option<Arc<D::Link>> {
        match self.stage {
            Stage::Live => self.live.as_ref().map(|live| Arc::clone(&live.link)),
            _ => None,
        }
    }

    // ---- the view

    /// computes the view and shows it when it, or what the live region says, changed.
    fn refresh(&mut self) {
        self.settle_pending();
        let clock = self.time.clock();
        self.ended_jobs
            .retain(|_, at| clock.now_ms.saturating_sub(*at) < ENDED_FOR_MS);
        let feed = match self.github.feed {
            Feed::Off => GithubFeed::Off,
            Feed::Unavailable => GithubFeed::Unavailable,
            Feed::ForNow => GithubFeed::UnavailableForNow,
            Feed::Live { opened_ms } => GithubFeed::Live {
                opened_ms,
                status: self.github.status.as_ref(),
            },
        };
        let input = Input::new(self.connection, self.snapshot.as_ref(), clock)
            .pending(self.pending)
            .tokens(&self.tokens)
            .settings(&self.settings)
            .github(feed)
            .ended_jobs(&self.ended_jobs);
        let Update { view, announced } = self.tracker.update(input);
        self.recheck = announced
            .recheck_at_ms
            .map(|at| Instant::now() + Duration::from_millis(at.saturating_sub(clock.now_ms)));
        let mut announce = announced.messages;
        announce.append(&mut self.announce);
        if self.shown.as_ref() != Some(&view) || !announce.is_empty() {
            self.shown = Some(view.clone());
            self.revision += 1;
            self.present.present(&Shown {
                view,
                announce,
                revision: self.revision,
            });
        }
    }

    /// forgets an action once the snapshot shows it, or once it was answered and the
    /// snapshot has not shown it in a while.
    fn settle_pending(&mut self) {
        let Some(pending) = self.pending else {
            return;
        };
        let settled = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| shows(snapshot, pending))
            || self.answered.is_some_and(|at| at.elapsed() >= PENDING_FOR);
        if settled {
            self.pending = None;
            self.answered = None;
            self.maybe_drain();
        }
    }

    // ---- reaching the agent

    fn launch_mode(&self) -> Launch {
        launch_for(self.snapshot.as_ref().is_some_and(keeps_stopped))
    }

    fn begin_attempt(&mut self) {
        self.abort_attempt();
        self.stage = Stage::Attempting;
        self.retry_at = None;
        self.generation += 1;
        let generation = self.generation;
        let start =
            (self.starts_left > 0).then(|| self.next_launch.unwrap_or_else(|| self.launch_mode()));
        let dial = Arc::clone(&self.dial);
        let launcher = Arc::clone(&self.launcher);
        let log = Arc::clone(&self.log);
        let report = self.report.clone();
        self.attempt = Some(tokio::spawn(async move {
            let attempt = attempt(&*dial, &*launcher, start, &*log).await;
            let _ = report.send(Internal::Attempted {
                generation,
                attempt,
            });
        }));
    }

    fn abort_attempt(&mut self) {
        if let Some(task) = self.attempt.take() {
            task.abort();
        }
    }

    /// what the connection is when the agent cannot be reached: lost contact while a
    /// list is kept, else no agent.
    fn unreachable(&self) -> Connection {
        match (&self.snapshot, self.last_heard_ms) {
            (Some(_), Some(last_heard_ms)) => Connection::LostContact {
                last_heard_ms,
                cause: Cause::RestartFailed,
            },
            _ => Connection::NoAgent,
        }
    }

    fn found(&mut self, attempted: Attempt<D::Link>) {
        // the launch's own mode is spent once an attempt reached or started an agent; an
        // attempt that did neither keeps it for the next
        let connected = matches!(attempted.found, Found::Connected(_));
        // the agent a fresh launch started is owed `launch` until something connects,
        // even when a later attempt reaches it without starting another
        if attempted.started && self.send_launch {
            self.launch_owed = true;
        }
        if attempted.started || connected {
            self.next_launch = None;
            self.send_launch = false;
        }
        let send_launch = connected && std::mem::take(&mut self.launch_owed);
        if attempted.started {
            self.starts_left = self.starts_left.saturating_sub(1);
            self.started_during_drain();
        }
        match attempted.found {
            Found::Connected(connected) => {
                self.starts_left = 1;
                self.connected_to(connected.agent.clone());
                self.live = Some(Live {
                    link: Arc::new(connected.link),
                    pushes: connected.pushes,
                });
                self.stage = Stage::Live;
                self.connection = Connection::Connected;
                self.last_heard_ms = Some(self.now_ms());
                self.log.info("Connected to the agent.");
                if send_launch {
                    self.send_launch_call();
                }
                // another agent may list the topic
                if self.github.feed == Feed::Unavailable {
                    self.github.feed = Feed::Off;
                }
                self.github_begin();
            }
            Found::NoAgent => {
                self.stage = Stage::Parked;
                self.connection = self.unreachable();
                self.unreached_during_drain();
            }
            Found::NotAnswering => {
                self.stage = Stage::Parked;
                self.connection = Connection::NotAnswering;
            }
            Found::Busy => {
                self.waiting();
                self.connection = Connection::LostContact {
                    last_heard_ms: self.last_heard_ms.unwrap_or_else(|| self.now_ms()),
                    cause: Cause::Busy,
                };
            }
            Found::LockHeld => {
                self.waiting();
                self.connection = Connection::LockHeld;
            }
        }
    }

    /// sends `launch` to the agent a fresh launch started stopped, so it starts the
    /// runners as `start_runners_on_launch` says, or `start_all` to an agent that does
    /// not list `launch`.
    fn send_launch_call(&self) {
        let Some(link) = self.link() else {
            return;
        };
        let log = Arc::clone(&self.log);
        tokio::spawn(async move {
            let mut sent = within(CALL_WITHIN, link.call(Method::Launch)).await;
            if matches!(sent, Err(Failure::NotServed(_))) {
                sent = within(CALL_WITHIN, link.call(Method::StartAll)).await;
            }
            if let Err(failure) = sent {
                log.warn(&format!(
                    "The agent was not told this is a fresh launch: {}",
                    failure.words()
                ));
            }
        });
    }

    fn waiting(&mut self) {
        self.stage = Stage::Waiting;
        self.retry_at = Some(Instant::now() + RETRY_EVERY);
    }

    fn woke(&mut self) {
        let now = Instant::now();
        if self.end_by.is_some_and(|by| now >= by) {
            self.ended = Some(Ended::Quit);
            return;
        }
        if matches!(self.stage, Stage::Waiting) && self.retry_at.is_some_and(|at| now >= at) {
            self.begin_attempt();
        }
        self.update_due(now);
        if self.github.retry_at.is_some_and(|at| now >= at) {
            self.github.retry_at = None;
            self.github_begin();
        }
        if self.github.renew_at.is_some_and(|at| now >= at) {
            self.github_renew();
        }
        if now >= self.next_tick {
            self.next_tick = now + TICK;
        }
        self.refresh();
    }

    // ---- what the agent sends

    fn pushed(&mut self, pushed: Option<Pushed>) {
        match pushed.unwrap_or(Pushed::Closed) {
            Pushed::Snapshot(snapshot) => {
                self.last_heard_ms = Some(self.now_ms());
                self.snapshot = Some(*snapshot);
                self.snapshot_for_drain();
                self.refresh();
            }
            Pushed::Heartbeat => self.last_heard_ms = Some(self.now_ms()),
            // this connection holds no `github` subscription
            Pushed::Github(_) => {}
            Pushed::Ending { reason, .. } if reason == QUIT || reason == EXECUTABLE_CHANGED => {
                self.log.info(&format!("The agent is ending: {reason}."));
                self.ended = Some(Ended::AgentEnded(reason));
            }
            Pushed::Ending {
                reason,
                update: Some(update),
            } if reason == ENDING_UPDATE && self.drain_is_for(&update) => {
                self.agent_ending(update.keeps_stopped());
            }
            // an agent may add reasons within version 2; what is not known is ignored,
            // and the connection's end that follows is handled as a drop. So is an end for
            // an update another client asked for.
            Pushed::Ending { update, .. } => {
                self.log
                    .info("The agent is ending for a reason this app does not act on.");
                // the agent's end is not this app's drain ending, so it installs nothing
                self.drain = None;
                // an end for an update still says how the runners were left, which the
                // start after it keeps
                if let Some(update) = update {
                    self.next_launch = Some(launch_for(update.keeps_stopped()));
                }
            }
            Pushed::Closed => self.closed(),
        }
    }

    fn closed(&mut self) {
        self.live = None;
        // the agent is gone, so is the subscription; the last GitHub status stays with
        // the last list until the agent is reached again
        self.github.close();
        if matches!(self.stage, Stage::Quitting) {
            self.ended = Some(Ended::Quit);
            return;
        }
        if matches!(self.stage, Stage::Ending) {
            // the agent said it is ending for the update; the wait for its process decides
            return;
        }
        self.log.warn("The connection to the agent ended.");
        if self.drain_closed() {
            return;
        }
        self.restart_after_drop();
    }

    /// what the app does when its agent is gone: keep the list under lost contact and
    /// start the agent once more.
    fn restart_after_drop(&mut self) {
        self.reconnect_after_drop(true);
    }

    /// keeps the list under lost contact and reaches the agent again, starting it once
    /// more only with `start`.
    fn reconnect_after_drop(&mut self, start: bool) {
        self.pending = None;
        self.answered = None;
        if let Some(repository) = self.tokens.editing.clone() {
            self.record(&repository, TokenResult::NotSaved(NotSaved::LostContact));
        }
        self.connection = Connection::LostContact {
            last_heard_ms: self.last_heard_ms.unwrap_or_else(|| self.now_ms()),
            cause: Cause::Dropped,
        };
        self.starts_left = u32::from(start);
        self.begin_attempt();
        self.refresh();
    }

    // ---- what the windows ask

    fn command(&mut self, command: Command) {
        match command {
            Command::Read { forms, reply } => {
                if let Some(forms) = forms {
                    self.forms(forms);
                }
                let revision = self.revision;
                let shown = self.shown.clone().map(|view| Shown {
                    view,
                    announce: Vec::new(),
                    revision,
                });
                if let Some(shown) = shown {
                    let _ = reply.send(shown);
                }
            }
            Command::Act { act, reply } => self.act(act, reply),
            Command::TryAgain { reply } => self.try_again(reply),
            Command::SetToken {
                repository,
                token,
                reply,
            } => self.set_token(&repository, token, reply),
            Command::RemoveToken { repository, reply } => self.remove_token(&repository, reply),
            Command::SetSetting { key, on, reply } => self.set_setting(key, on, reply),
            Command::WindowShown(shown) => self.set_window_shown(shown),
            Command::OpenJob {
                pool,
                runner,
                reply,
            } => {
                let _ = reply.send(self.open_job(&pool, &runner));
            }
            Command::Quit => self.quit(),
        }
    }

    fn forms(&mut self, forms: Forms) {
        if let (Some(old), Some(new)) = (&self.tokens.editing, &forms.editing)
            && old != new
        {
            self.announce
                .push(marshal_view::discarded_announcement(old, false));
        }
        self.tokens.editing = forms.editing;
        self.tokens.removing = forms.removing;
        self.refresh();
    }

    fn act(&mut self, act: Act, reply: Reply<()>) {
        let Some(link) = self.link() else {
            let _ = reply.send(Err(self.unavailable()));
            return;
        };
        let since_ms = self.now_ms();
        self.pending = Some(Pending {
            kind: act.pending(),
            since_ms,
        });
        self.answered = None;
        // the transition shows at once, before the agent has answered
        self.refresh();
        let report = self.report.clone();
        tokio::spawn(async move {
            let outcome = within(CALL_WITHIN, link.call(act.method())).await;
            let _ = report.send(Internal::Acted {
                since_ms,
                outcome: outcome.clone(),
            });
            let _ = reply.send(outcome.map_err(|failure| failure.words()));
        });
    }

    fn unavailable(&self) -> String {
        if matches!(self.stage, Stage::Quitting) {
            "Marshal is quitting.".to_string()
        } else {
            Failure::Lost.words()
        }
    }

    fn try_again(&mut self, reply: Reply<()>) {
        if !matches!(self.stage, Stage::Parked | Stage::Waiting) {
            let _ = reply.send(Err("Marshal is not waiting to try again.".to_string()));
            return;
        }
        if matches!(
            self.connection,
            Connection::NoAgent | Connection::NotAnswering
        ) {
            self.connection = Connection::StartingMarshal;
        }
        self.starts_left = 1;
        self.begin_attempt();
        self.refresh();
        let _ = reply.send(Ok(()));
    }

    fn quit(&mut self) {
        let Some(link) = self.link() else {
            match self.stage {
                Stage::Quitting => {}
                // the attempt may be starting the agent: a Quit now must not leave one
                // behind, running the runners with no app
                Stage::Attempting => {
                    self.quit_when_reached = true;
                    self.connection = Connection::Quitting;
                    self.refresh();
                }
                Stage::Live | Stage::Waiting | Stage::Parked | Stage::Ending => {
                    self.ended = Some(Ended::AppOnly);
                }
            }
            return;
        };
        self.stage = Stage::Quitting;
        self.connection = Connection::Quitting;
        self.refresh();
        let report = self.report.clone();
        tokio::spawn(async move {
            let outcome = within(QUIT_WITHIN, link.call(Method::Quit)).await;
            let _ = report.send(Internal::QuitAnswered(outcome));
        });
    }

    // ---- tokens

    fn record(&mut self, repository: &str, result: TokenResult) {
        self.announce.push(result.announcement(repository));
        self.tokens.results.insert(repository.to_string(), result);
    }

    fn set_token(&mut self, repository: &str, token: String, reply: Reply<()>) {
        let Ok(key) = TokenKey::parse(repository) else {
            let _ = reply.send(Err("A repository is written owner/name.".to_string()));
            return;
        };
        let key = key.as_str().to_string();
        // the limits apply here, before anything is sent
        let secret = accepted_token(&token);
        drop(token);
        let secret = match secret {
            Ok(secret) => secret,
            Err(refusal) => {
                let failure = match refusal {
                    TokenRefusal::TooLong => NotSaved::TooLong,
                    TokenRefusal::Empty => NotSaved::Refused(refusal.to_string()),
                };
                self.record(&key, TokenResult::NotSaved(failure));
                self.refresh();
                let _ = reply.send(Ok(()));
                return;
            }
        };
        let Some(link) = self.link() else {
            self.record(&key, TokenResult::NotSaved(NotSaved::LostContact));
            self.refresh();
            let _ = reply.send(Ok(()));
            return;
        };
        let started = self
            .snapshot
            .as_ref()
            .map(|snapshot| pools_started_by(snapshot, &key))
            .unwrap_or_default();
        let report = self.report.clone();
        tokio::spawn(async move {
            let outcome = within(
                TOKEN_WITHIN,
                link.call(Method::TokenSet {
                    repository: key.clone(),
                    token: secret,
                }),
            )
            .await;
            let _ = report.send(Internal::Token {
                repository: key,
                removal: false,
                started,
                outcome,
                reply,
            });
        });
    }

    fn remove_token(&mut self, repository: &str, reply: Reply<()>) {
        let Ok(key) = TokenKey::parse(repository) else {
            let _ = reply.send(Err("A repository is written owner/name.".to_string()));
            return;
        };
        let key = key.as_str().to_string();
        let Some(link) = self.link() else {
            self.record(&key, TokenResult::NotRemoved(not_removed(&Failure::Lost)));
            self.refresh();
            let _ = reply.send(Ok(()));
            return;
        };
        let report = self.report.clone();
        tokio::spawn(async move {
            let outcome = within(
                TOKEN_WITHIN,
                link.call(Method::TokenRemove {
                    repository: key.clone(),
                }),
            )
            .await;
            let _ = report.send(Internal::Token {
                repository: key,
                removal: true,
                started: Vec::new(),
                outcome,
                reply,
            });
        });
    }

    // ---- settings

    fn set_setting(&mut self, key: String, on: bool, reply: Reply<()>) {
        let Some(label) = setting_label(&key) else {
            let _ = reply.send(Err(format!(
                "{} is not a setting.",
                marshal_view::plain(&key)
            )));
            return;
        };
        if self.settings.writing.is_some() {
            let _ = reply.send(Err("Another setting is being saved.".to_string()));
            return;
        }
        let Some(link) = self.link() else {
            self.record_setting(&key, label, SettingResult::LostContact);
            self.refresh();
            let _ = reply.send(Ok(()));
            return;
        };
        self.settings.writing = Some(key.clone());
        self.refresh();
        let report = self.report.clone();
        tokio::spawn(async move {
            let outcome = within(
                SETTING_WITHIN,
                link.call(Method::LifecycleSet {
                    key: key.clone(),
                    on,
                }),
            )
            .await;
            let _ = report.send(Internal::Setting {
                key,
                on,
                outcome,
                reply,
            });
        });
    }

    fn record_setting(&mut self, key: &str, label: &str, result: SettingResult) {
        self.announce.push(result.announcement(label));
        self.settings.results.insert(key.to_string(), result);
    }

    fn setting_answered(&mut self, key: &str, on: bool, outcome: Result<(), Failure>) {
        self.settings.writing = None;
        let result = match outcome {
            Ok(()) => SettingResult::Saved { on },
            Err(Failure::Lost) => SettingResult::LostContact,
            Err(Failure::TimedOut) => SettingResult::NotAnswered,
            Err(Failure::Busy(words)) => SettingResult::Refused {
                code: "busy".to_string(),
                words,
            },
            Err(Failure::Config { code, message }) => SettingResult::Refused {
                code,
                words: message,
            },
            Err(other) => SettingResult::Refused {
                code: String::new(),
                words: other.words(),
            },
        };
        let label = setting_label(key).unwrap_or(key).to_string();
        if !matches!(result, SettingResult::Saved { .. }) {
            self.log.warn(&format!(
                "{key} was not set {}: {}",
                if on { "on" } else { "off" },
                result.announcement(&label)
            ));
        }
        self.record_setting(key, &label, result);
        self.refresh();
    }

    // ---- GitHub status

    fn set_window_shown(&mut self, shown: bool) {
        if shown == self.window_shown {
            return;
        }
        self.window_shown = shown;
        if shown {
            self.github_begin();
        } else {
            self.github.close();
            // an agent without the topic stays without it until another is reached
            if self.github.feed != Feed::Unavailable {
                self.github.feed = Feed::Off;
            }
            self.github.status = None;
            self.ended_jobs.clear();
            self.log
                .info("The window is hidden: the GitHub subscription ends.");
        }
        self.refresh();
    }

    /// opens the second connection and subscribes it to `github`, when the window is
    /// shown, the agent is reached and nothing of the kind is under way.
    fn github_begin(&mut self) {
        let Some(_) = self.link() else {
            return;
        };
        if !self.window_shown
            || self.github.feed == Feed::Unavailable
            || self.github.live.is_some()
            || self.github.attempt.is_some()
            || self.github.retry_at.is_some()
        {
            return;
        }
        let opened_ms = self.now_ms();
        self.github.generation += 1;
        let generation = self.github.generation;
        let dial = Arc::clone(&self.dial);
        let report = self.report.clone();
        self.github.attempt = Some(tokio::spawn(async move {
            let found = match dial.dial().await {
                Ok(connected) => {
                    match tokio::time::timeout(GITHUB_WITHIN, connected.link.subscribe_github())
                        .await
                    {
                        Ok(Ok(())) => Ok(connected),
                        Ok(Err(failure)) => Err(failure),
                        Err(_) => Err(Failure::TimedOut),
                    }
                }
                Err(DialError::Busy) => Err(Failure::Busy(String::new())),
                Err(other) => Err(Failure::Other(format!("{other:?}"))),
            };
            let _ = report.send(Internal::GithubAttempted { generation, found });
        }));
        // the subscription's own time, so the first value, which holds what the agent
        // learned before, reads as checking; a feed the agent had no room for says so
        // until it is held
        if matches!(self.github.feed, Feed::Off | Feed::Live { .. }) {
            self.github.feed = Feed::Live { opened_ms };
        }
    }

    fn github_found(&mut self, found: Result<Connected<D::Link>, Failure>) {
        match found {
            Ok(connected) => {
                self.github.live = Some(Live {
                    link: Arc::new(connected.link),
                    pushes: connected.pushes,
                });
                if self.github.feed == Feed::ForNow {
                    self.github.feed = Feed::Live {
                        opened_ms: self.now_ms(),
                    };
                }
                self.github.renew_at = Some(Instant::now() + RENEW_EVERY);
                self.log.info("Subscribed to GitHub status.");
            }
            Err(Failure::NotServed(_)) => {
                self.github.feed = Feed::Unavailable;
                self.github.status = None;
                self.log
                    .info("The agent does not report GitHub status; none is shown.");
            }
            Err(Failure::Busy(_)) => {
                self.github.feed = Feed::ForNow;
                self.github.retry_at = Some(Instant::now() + GITHUB_ASK_AGAIN);
                self.log.info(&format!(
                    "The agent has no room for the GitHub subscription; asking again in {} s.",
                    GITHUB_ASK_AGAIN.as_secs()
                ));
            }
            Err(failure) => {
                self.github.retry_at = Some(Instant::now() + RETRY_EVERY);
                self.log.warn(&format!(
                    "The GitHub subscription failed: {}",
                    failure.words()
                ));
            }
        }
    }

    fn github_renew(&mut self) {
        self.github.renew_at = Some(Instant::now() + RENEW_EVERY);
        let Some(link) = self.github.live.as_ref().map(|live| Arc::clone(&live.link)) else {
            return;
        };
        let generation = self.github.generation;
        let report = self.report.clone();
        tokio::spawn(async move {
            let (resubscribed, outcome) = match within(GITHUB_WITHIN, link.renew_github()).await {
                // the agent ended it, so it is subscribed again on the same connection
                Err(Failure::NotSubscribed) => {
                    (true, within(GITHUB_WITHIN, link.subscribe_github()).await)
                }
                other => (false, other),
            };
            let _ = report.send(Internal::GithubRenewed {
                generation,
                resubscribed,
                outcome,
            });
        });
    }

    fn github_renewed(&mut self, outcome: Result<(), Failure>) {
        match outcome {
            Ok(()) => {}
            Err(Failure::Busy(_)) => {
                self.github.close();
                self.github.feed = Feed::ForNow;
                self.github.retry_at = Some(Instant::now() + GITHUB_ASK_AGAIN);
                self.refresh();
            }
            Err(failure) => {
                self.log.warn(&format!(
                    "The GitHub subscription was not renewed: {}",
                    failure.words()
                ));
                self.github.close();
                self.github.retry_at = Some(Instant::now() + RETRY_EVERY);
            }
        }
    }

    fn github_pushed(&mut self, pushed: Option<Pushed>) {
        match pushed.unwrap_or(Pushed::Closed) {
            Pushed::Github(status) => {
                self.github.status = Some(*status);
                self.refresh();
            }
            Pushed::Closed => {
                self.github.close();
                if self.window_shown {
                    self.github.retry_at = Some(Instant::now() + RETRY_EVERY);
                }
            }
            Pushed::Snapshot(_) | Pushed::Heartbeat | Pushed::Ending { .. } => {}
        }
    }

    /// the job page of `pool`'s runner `runner`, or `None` with the pool saying the job
    /// has ended.
    fn open_job(&mut self, pool: &str, runner: &str) -> Result<Option<String>, String> {
        let known = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.pools.iter().any(|each| each.name == pool));
        if !known {
            return Err("Marshal has no such pool.".to_string());
        }
        if self.link().is_none() {
            return Err(self.unavailable());
        }
        let page = match (&self.snapshot, &self.github.status) {
            (Some(snapshot), Some(status)) => job_page(snapshot, status, pool, runner),
            _ => None,
        };
        match page {
            Some(page) => {
                self.ended_jobs.remove(pool);
                Ok(Some(page))
            }
            None => {
                let now_ms = self.now_ms();
                self.ended_jobs.entry(pool.to_string()).or_insert(now_ms);
                self.refresh();
                Ok(None)
            }
        }
    }

    // ---- what its own tasks report

    fn internal(&mut self, internal: Internal<D::Link>) {
        match internal {
            Internal::Attempted {
                generation,
                attempt,
            } => {
                if generation == self.generation {
                    self.attempt = None;
                    self.found(attempt);
                    if std::mem::take(&mut self.quit_when_reached) {
                        self.quit_what_was_reached();
                    }
                    self.refresh();
                }
            }
            Internal::Acted { since_ms, outcome } => {
                match outcome {
                    Ok(()) => self.answered = Some(Instant::now()),
                    Err(failure) => {
                        self.log
                            .warn(&format!("An action failed: {}", failure.words()));
                        if self
                            .pending
                            .is_some_and(|pending| pending.since_ms == since_ms)
                        {
                            self.pending = None;
                            self.maybe_drain();
                        }
                    }
                }
                self.refresh();
            }
            Internal::Token {
                repository,
                removal,
                started,
                outcome,
                reply,
            } => {
                self.token_answered(&repository, removal, started, outcome);
                let _ = reply.send(Ok(()));
            }
            Internal::Setting {
                key,
                on,
                outcome,
                reply,
            } => {
                self.setting_answered(&key, on, outcome);
                let _ = reply.send(Ok(()));
            }
            Internal::QuitAnswered(outcome) => self.quit_answered(outcome),
            Internal::Checked(outcome) => self.checked(outcome),
            Internal::DrainAnswered(outcome) => self.drain_answered(outcome),
            Internal::AgentGone(gone) => self.agent_gone(gone),
            Internal::GithubAttempted { generation, found } => {
                if generation == self.github.generation {
                    self.github.attempt = None;
                    self.github_found(found);
                    self.refresh();
                }
            }
            Internal::GithubRenewed {
                generation,
                resubscribed,
                outcome,
            } => {
                if generation == self.github.generation {
                    // collection stopped meanwhile, so what the agent sends next is
                    // from before and reads as checking until it checks again
                    if resubscribed && outcome.is_ok() {
                        self.github.feed = Feed::Live {
                            opened_ms: self.now_ms(),
                        };
                        self.refresh();
                    }
                    self.github_renewed(outcome);
                }
            }
        }
    }

    fn token_answered(
        &mut self,
        repository: &str,
        removal: bool,
        started: Vec<String>,
        outcome: Result<(), Failure>,
    ) {
        let result = match (removal, outcome) {
            (false, Ok(())) => TokenResult::Saved { started },
            (true, Ok(())) => TokenResult::Removed,
            (_, Err(Failure::TimedOut)) => {
                // the connection is open and the agent may still do it, so the snapshot,
                // not this call, says where the token stands
                self.log.warn(&format!(
                    "The agent did not answer the token {} for {repository} in {} s; it may still do it.",
                    if removal { "removal" } else { "save" },
                    TOKEN_WITHIN.as_secs()
                ));
                TokenResult::NotAnswered { removal }
            }
            (_, Err(failure)) => {
                self.log.warn(&format!(
                    "A token for {repository} was not {}: {}",
                    if removal { "removed" } else { "saved" },
                    failure.words()
                ));
                if removal {
                    TokenResult::NotRemoved(not_removed(&failure))
                } else {
                    TokenResult::NotSaved(not_saved(failure))
                }
            }
        };
        if !matches!(
            result,
            TokenResult::NotSaved(_) | TokenResult::NotRemoved(_)
        ) {
            // the form closes on success and stays open, with its error, on failure
            if self.tokens.editing.as_deref() == Some(repository) {
                self.tokens.editing = None;
            }
            if self.tokens.removing.as_deref() == Some(repository) {
                self.tokens.removing = None;
            }
        }
        self.record(repository, result);
        self.refresh();
    }

    /// the user quit during an attempt, which has now ended: an agent it reached is
    /// quit, and with none the app ends alone.
    fn quit_what_was_reached(&mut self) {
        if self.link().is_some() {
            self.quit();
        } else {
            self.ended = Some(Ended::AppOnly);
        }
    }

    fn quit_answered(&mut self, outcome: Result<(), Failure>) {
        match outcome {
            Ok(()) => self.end_by = Some(Instant::now() + QUIT_LINGER),
            Err(Failure::Lost) => self.ended = Some(Ended::Quit),
            Err(Failure::TimedOut) => {
                // the connection is open and the agent may still be stopping, so the app
                // stays until the agent says it is ending or the connection closes
                self.log.warn(&format!(
                    "The agent did not answer quit in {} s; waiting for it to end.",
                    QUIT_WITHIN.as_secs()
                ));
            }
            Err(failure) => {
                // the agent did not quit, so the app does not end and leave it running
                self.log
                    .warn(&format!("The agent did not quit: {}", failure.words()));
                self.stage = Stage::Live;
                self.connection = Connection::Connected;
                self.refresh();
            }
        }
    }
}

async fn next_push<L>(live: &mut Option<Live<L>>) -> Option<Pushed> {
    match live {
        Some(live) => live.pushes.recv().await,
        None => pending().await,
    }
}

async fn within<F>(limit: Duration, call: F) -> Result<(), Failure>
where
    F: std::future::Future<Output = Result<(), Failure>>,
{
    tokio::time::timeout(limit, call)
        .await
        .unwrap_or(Err(Failure::TimedOut))
}

/// whether the agent's snapshot shows what `pending` was waiting for.
fn shows(snapshot: &Snapshot, pending: Pending) -> bool {
    let state = snapshot.agent.state.as_str();
    let unchanged = match pending.kind {
        PendingKind::StopAll => matches!(state, "running" | "resuming"),
        PendingKind::StartAll | PendingKind::Build => state == "stopped",
        PendingKind::StopBuild => state == "building",
    };
    !unchanged || snapshot.agent.since >= pending.since_ms
}

/// how the agent starts so that it leaves the runners as they were: stopped when they
/// stay stopped, else running.
fn launch_for(stopped: bool) -> Launch {
    if stopped {
        Launch::Stopped
    } else {
        Launch::Running
    }
}

/// whether the last state heard keeps the runners stopped across a restart: stopped by
/// you, stopped by a sleep, a network change or too many resumes, or with a resume
/// waiting, so a restart never starts them on another network.
fn keeps_stopped(snapshot: &Snapshot) -> bool {
    snapshot.agent.state == "stopped"
        && snapshot.agent.reason.as_ref().is_some_and(|reason| {
            reason.code == StopReason::BY_YOU || StopReason::is_signal_code(&reason.code)
        })
}

/// why a removal failed, with the token store's kind and code when the agent gave them.
fn not_removed(failure: &Failure) -> String {
    match failure {
        Failure::Store {
            detail: Some(detail),
        } => format!("The token store refused the request ({detail})"),
        other => other.words(),
    }
}

fn not_saved(failure: Failure) -> NotSaved {
    match failure {
        Failure::Store { detail } => NotSaved::Store { detail },
        Failure::Lost => NotSaved::LostContact,
        other => NotSaved::Refused(other.words()),
    }
}
