//! the session: one task that owns everything the app knows about its agent: the
//! connection and what it is doing, the latest snapshot, the actions and token
//! results the snapshot does not show yet, and the view computed from them. The
//! windows and the menu act on it through a [`Handle`], and it shows what it knows
//! through [`Present`](crate::seam::Present).
//!
//! The session connects at launch, starts the agent when none answers, keeps the last
//! list under lost contact, starts the agent once more, tries again on `busy` and on a
//! held lock every 5 s, and ends when the agent ends for `quit` or
//! `executable_changed`.

use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::{EXECUTABLE_CHANGED, Launch, QUIT, StopReason};
use marshal_cli::token_store::{RepositoryKey, TokenRefusal, accepted_token};
use marshal_view::{
    Cause, Connection, Input, NotSaved, Pending, PendingKind, TokenResult, TokenUi, Tracker,
    Update, View, pools_started_by,
};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};

use crate::connect::{Attempt, Found, RETRY_EVERY, attempt};
use crate::seam::{Dial, Failure, Launcher, Link, Log, Method, Present, Pushed, Shown, TimeSource};

/// how often the view is computed again with no news, so a build's elapsed time and the
/// settled count move.
const TICK: Duration = Duration::from_secs(1);

/// the longest a stop, a start or a build call waits for its answer: the stop sequence
/// takes at most 30 s.
const CALL_WITHIN: Duration = Duration::from_secs(40);

/// the longest a token call waits for the Credential Manager.
const TOKEN_WITHIN: Duration = Duration::from_secs(10);

/// the longest `quit` waits: the stop sequence, then the agent ending.
const QUIT_WITHIN: Duration = Duration::from_secs(45);

/// how long the app waits for the agent's connection to end after `quit` answered, so
/// the agent is gone when the app is.
const QUIT_LINGER: Duration = Duration::from_secs(3);

/// an answered action whose snapshot never shows it stops being shown after this.
const PENDING_FOR: Duration = Duration::from_secs(5);

/// why the session ended, and so the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// the user quit and the agent ended with the app.
    Quit,
    /// the agent ended for this reason: `quit` or `executable_changed`.
    AgentEnded(String),
    /// the user quit with no agent to quit: only the app ends.
    AppOnly,
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
    QuitAnswered(Result<(), Failure>),
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
    announce: Vec<String>,
    tracker: Tracker,
    shown: Option<View>,
    /// the revision of `shown`, which counts the views the session has shown.
    revision: u64,
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
            announce: Vec::new(),
            tracker: Tracker::new(),
            shown: None,
            revision: 0,
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
                () = sleep_until(wake) => self.woke(),
            }
            if let Some(ended) = self.ended.take() {
                self.abort_attempt();
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
        let input = Input::new(self.connection, self.snapshot.as_ref(), clock)
            .pending(self.pending)
            .tokens(&self.tokens);
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
        }
    }

    // ---- reaching the agent

    fn launch_mode(&self) -> Launch {
        if self.snapshot.as_ref().is_some_and(stopped_by_you) {
            Launch::Stopped
        } else {
            Launch::Running
        }
    }

    fn begin_attempt(&mut self) {
        self.abort_attempt();
        self.stage = Stage::Attempting;
        self.retry_at = None;
        self.generation += 1;
        let generation = self.generation;
        let start = (self.starts_left > 0).then(|| self.launch_mode());
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
        if attempted.started {
            self.starts_left = self.starts_left.saturating_sub(1);
        }
        match attempted.found {
            Found::Connected(connected) => {
                self.starts_left = 1;
                self.live = Some(Live {
                    link: Arc::new(connected.link),
                    pushes: connected.pushes,
                });
                self.stage = Stage::Live;
                self.connection = Connection::Connected;
                self.last_heard_ms = Some(self.now_ms());
                self.log.info("Connected to the agent.");
            }
            Found::NoAgent => {
                self.stage = Stage::Parked;
                self.connection = self.unreachable();
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
                self.refresh();
            }
            Pushed::Heartbeat => self.last_heard_ms = Some(self.now_ms()),
            Pushed::Ending(reason) if reason == QUIT || reason == EXECUTABLE_CHANGED => {
                self.log.info(&format!("The agent is ending: {reason}."));
                self.ended = Some(Ended::AgentEnded(reason));
            }
            // an agent may add reasons within version 2; what is not known is ignored,
            // and the connection's end that follows is handled as a drop
            Pushed::Ending(_) => self
                .log
                .info("The agent is ending for a reason this app does not know."),
            Pushed::Closed => self.closed(),
        }
    }

    fn closed(&mut self) {
        self.live = None;
        if matches!(self.stage, Stage::Quitting) {
            self.ended = Some(Ended::Quit);
            return;
        }
        self.log.warn("The connection to the agent ended.");
        self.pending = None;
        self.answered = None;
        if let Some(repository) = self.tokens.editing.clone() {
            self.record(&repository, TokenResult::NotSaved(NotSaved::LostContact));
        }
        self.connection = Connection::LostContact {
            last_heard_ms: self.last_heard_ms.unwrap_or_else(|| self.now_ms()),
            cause: Cause::Dropped,
        };
        self.starts_left = 1;
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
                Stage::Live | Stage::Waiting | Stage::Parked => self.ended = Some(Ended::AppOnly),
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
        let Ok(key) = RepositoryKey::parse(repository) else {
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
        let Ok(key) = RepositoryKey::parse(repository) else {
            let _ = reply.send(Err("A repository is written owner/name.".to_string()));
            return;
        };
        let key = key.as_str().to_string();
        let Some(link) = self.link() else {
            self.record(
                &key,
                TokenResult::NotSaved(NotSaved::Refused(Failure::Lost.words())),
            );
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
            Internal::QuitAnswered(outcome) => self.quit_answered(outcome),
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
            (_, Err(failure)) => {
                self.log.warn(&format!(
                    "A token for {repository} was not {}: {}",
                    if removal { "removed" } else { "saved" },
                    failure.words()
                ));
                TokenResult::NotSaved(not_saved(failure))
            }
        };
        if !matches!(result, TokenResult::NotSaved(_)) {
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

/// whether the last state heard is "stopped by you", which a restart keeps.
fn stopped_by_you(snapshot: &Snapshot) -> bool {
    snapshot.agent.state == "stopped"
        && snapshot
            .agent
            .reason
            .as_ref()
            .is_some_and(|reason| reason.code == StopReason::BY_YOU)
}

fn not_saved(failure: Failure) -> NotSaved {
    match failure {
        Failure::Store { detail } => NotSaved::Store { detail },
        Failure::Lost => NotSaved::LostContact,
        other => NotSaved::Refused(other.words()),
    }
}
