//! the resident agent: the state it is in, and what each request, each failure of
//! the runtime core and a changed executable do to it, as `docs/operations/cli.md`
//! describes; one task owns the state, so a request that arrives during a transition
//! is answered at once: `status` tells the truth, a stop is taken, and anything
//! that would start another transition is `busy`.

use std::cell::{Cell, RefCell};
use std::future::{Future, pending};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::json;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Instant, MissedTickBehavior, interval, sleep_until, timeout};

use crate::command::{LoadError, load_plan_under};
use crate::instance::lifecycle::{CatchPanic, CoreFailure, Ending, StopTimes};
use crate::instance::restart::{RestartPolicy, Verdict};
use crate::instance::steps::{self, Steps};
use crate::profile::Profile;
use crate::runtime::backend::Backend;
use crate::runtime::board::{Board, PoolState, SlotState};
use crate::runtime::clock::Clock;
use crate::runtime::image::check_image;
use crate::runtime::log::Log;
use crate::runtime::registration::Registrar;
use crate::runtime::scrub::scrub_error_text;
use crate::runtime::worker::sleep_or_stop;
use crate::token_store::RepositoryKey;
use crate::validate::Plan;

use super::api::{ApiError, ApiResult, Call, Method};
use super::builder::ImageBuilder;
use super::core::AgentCore;
use super::snapshot::{BuildInfo, MAX_SNAPSHOT_BYTES, Snapshot};
use super::state::{Action, Drain, Inner, Phase, Reason};
use super::tokens::StoreTokens;
use super::view::{View, millis};
use super::wire::{ENDING_UPDATE, ErrorCode, UpdateEnding};

/// the reason the agent ends when asked to by a client.
pub const QUIT: &str = "quit";

/// the reason it ends when the file it started from has changed.
pub const EXECUTABLE_CHANGED: &str = "executable_changed";

/// the reason it ends for an update.
pub const UPDATE: &str = ENDING_UPDATE;

/// how long a drain for an update waits for busy runners before it stops them.
pub const DRAIN_BOUND: Duration = Duration::from_secs(6 * 60 * 60);

/// how often the snapshot is read again for subscribers, the coalescing time of an
/// event; nothing is collected while no one subscribes.
const REFRESH_EVERY: Duration = Duration::from_millis(250);

/// whether the agent runs the pools at once or waits to be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    Running,
    Stopped,
}

/// what the agent was started with.
#[derive(Debug, Clone)]
pub struct Settings {
    pub version: String,
    /// the configuration file the agent reads at every start.
    pub config_path: std::path::PathBuf,
    pub times: StopTimes,
    pub launch: Launch,
    /// how long a drain for an update waits for busy runners; [`DRAIN_BOUND`].
    pub drain_bound: Duration,
}

/// how the agent ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    /// [`QUIT`], [`EXECUTABLE_CHANGED`] or [`UPDATE`].
    pub reason: &'static str,
    /// the version the agent ended for and whether its runners were stopped by you,
    /// when it ended for an update.
    pub update: Option<UpdateEnding>,
    /// whether every removal of ordinary containers in the agent's life succeeded.
    pub containers_removed: bool,
    /// the diagnostic containers the backend still retains, which the next start
    /// finalises.
    pub kept: Vec<String>,
}

type Requests = mpsc::UnboundedReceiver<Call>;
type Reply = oneshot::Sender<ApiResult>;

enum Next {
    Resume(Option<Reply>),
    Run(Plan),
    Idle,
    Build(Reply),
    Exit,
}

/// what is left of a call for the handler of a phase.
enum Routed {
    /// the call is answered.
    Answered,
    /// the method begins a transition, and the reply waits for it.
    Begin(Method, Reply),
    /// the method stops what runs; the reply waits for the stop.
    Stop(Reply),
    /// the method ends the agent, and the exit is set; the reply waits for the stop.
    Quit(Reply),
}

enum CycleEnd {
    StopRequested,
    /// a drain for an update left the core nothing to run.
    Drained,
    LimitReached,
    NothingToRun,
    ImageRefused(String),
}

enum Reloaded {
    Run(Plan),
    Stopped(Reason),
}

fn ticker() -> tokio::time::Interval {
    let mut ticker = interval(REFRESH_EVERY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}

fn refused(message: impl Into<String>) -> ApiResult {
    Err(ApiError::new(ErrorCode::Refused, message))
}

pub struct Agent<'a, B, K, R, D> {
    settings: Settings,
    backend: &'a B,
    clock: &'a K,
    registrar: &'a R,
    builder: &'a D,
    tokens: &'a StoreTokens,
    log: &'a dyn Log,
    board: Board,
    inner: RefCell<Inner>,
    /// the containers a stop could not remove.
    left: RefCell<Vec<String>>,
    snapshots: watch::Sender<Arc<Snapshot>>,
    executable: watch::Receiver<bool>,
    policy: RefCell<RestartPolicy>,
    exit: Cell<Option<&'static str>>,
    containers_removed: Cell<bool>,
    force: watch::Sender<bool>,
    /// whether the pools of the latest start are admitted and prepared.
    admitted: watch::Sender<bool>,
    /// true while the agent drains for an update.
    drain: watch::Sender<bool>,
    /// what `ending` says when the agent ends for an update.
    update: RefCell<Option<UpdateEnding>>,
    /// set when a stop all arrives during a drain's last steps, so the update keeps it.
    stopped_in_drain: Cell<bool>,
}

/// a drain for an update, as the running phase keeps it.
struct Draining {
    version: String,
    deadline: Instant,
    /// closes when the connection that asked for the drain closes.
    requester: Option<oneshot::Receiver<()>>,
}

/// returns when the connection that asked for the drain has closed; never without a
/// drain, or for a drain whose requester cannot close.
async fn requester_closed(draining: &mut Option<Draining>) {
    match draining
        .as_mut()
        .and_then(|draining| draining.requester.as_mut())
    {
        Some(requester) => {
            let _ = requester.await;
        }
        None => pending().await,
    }
}

async fn drain_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => pending().await,
    }
}

/// what the agent works with besides its settings.
pub struct Seams<'a, B, K, R, D> {
    pub backend: &'a B,
    pub clock: &'a K,
    pub registrar: &'a R,
    pub builder: &'a D,
    pub tokens: &'a StoreTokens,
    pub log: &'a dyn Log,
    /// receives the snapshot after each change.
    pub snapshots: watch::Sender<Arc<Snapshot>>,
    /// turns true when the executable has changed.
    pub executable: watch::Receiver<bool>,
}

impl<'a, B: Backend, K: Clock, R: Registrar, D: ImageBuilder> Agent<'a, B, K, R, D> {
    #[must_use]
    pub fn new(settings: Settings, seams: Seams<'a, B, K, R, D>) -> Self {
        let phase = match settings.launch {
            Launch::Running => Phase::Resuming,
            Launch::Stopped => Phase::Stopped(Reason::by_you()),
        };
        let agent = Self {
            settings,
            backend: seams.backend,
            clock: seams.clock,
            registrar: seams.registrar,
            builder: seams.builder,
            tokens: seams.tokens,
            log: seams.log,
            board: Board::default(),
            inner: RefCell::new(Inner::new(phase)),
            left: RefCell::new(Vec::new()),
            snapshots: seams.snapshots,
            executable: seams.executable,
            policy: RefCell::new(RestartPolicy::default()),
            exit: Cell::new(None),
            containers_removed: Cell::new(true),
            force: watch::Sender::new(false),
            admitted: watch::Sender::new(false),
            drain: watch::Sender::new(false),
            update: RefCell::new(None),
            stopped_in_drain: Cell::new(false),
        };
        agent.publish();
        agent
    }

    /// runs the agent until it is asked to end, answering every call that reaches
    /// `requests`.
    pub async fn run(&self, requests: &mut Requests) -> Ended {
        let mut next = match self.settings.launch {
            Launch::Running => Next::Resume(None),
            Launch::Stopped => {
                // the pools are listed stopped; a configuration that cannot be read
                // is named in the snapshot
                let _ = self.read_configuration();
                self.publish();
                Next::Idle
            }
        };
        loop {
            next = match next {
                Next::Resume(reply) => self.resume(requests, reply).await,
                Next::Run(plan) => self.running(&plan, requests).await,
                Next::Idle => self.stopped(requests).await,
                Next::Build(reply) => self.building(requests, reply).await,
                Next::Exit => {
                    let reason = self.exit.get().unwrap_or(QUIT);
                    return Ended {
                        reason,
                        update: if reason == UPDATE {
                            self.update.borrow().clone()
                        } else {
                            None
                        },
                        containers_removed: self.containers_removed.get(),
                        kept: self.backend.retained().names(),
                    };
                }
            };
        }
    }

    fn set_phase(&self, phase: Phase) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.phase = phase;
            inner.since = SystemTime::now();
        }
        self.publish();
    }

    fn phase_name(&self) -> &'static str {
        self.inner.borrow().phase.name()
    }

    /// the snapshot as the agent is now, cut to the message limit.
    fn snapshot(&self) -> Snapshot {
        let mut inner = self.inner.borrow_mut();
        if let Some(image) = self.board.image_record() {
            inner.image = Some(image);
        }
        let left = self.left.borrow();
        let path = self.settings.config_path.display().to_string();
        let mut snapshot = View {
            version: &self.settings.version,
            inner: &inner,
            config_path: &path,
            board: &self.board,
            retained: self.backend.retained(),
            tokens: self.tokens,
            left: &left,
            now: self.clock.now(),
        }
        .snapshot();
        snapshot.fit(MAX_SNAPSHOT_BYTES);
        snapshot
    }

    /// hands subscribers the snapshot when it differs from the last.
    fn publish(&self) {
        let snapshot = Arc::new(self.snapshot());
        self.snapshots.send_if_modified(|held| {
            if **held == *snapshot {
                false
            } else {
                *held = snapshot;
                true
            }
        });
    }

    /// whether anyone subscribes: the channel holds one receiver, and each
    /// subscription another.
    fn watched(&self) -> bool {
        self.snapshots.receiver_count() > 1
    }

    /// publishes on a tick when someone subscribes. `select!` reads a branch
    /// precondition once, so the check cannot sit on the tick's branch: a
    /// subscriber that arrives while the loop waits would never be served.
    fn refresh(&self) {
        if self.watched() {
            self.publish();
        }
    }

    /// returns when the executable has changed, once, and sets the exit.
    async fn exit_asked(&self) {
        if self.exit.get().is_some() {
            return pending().await;
        }
        let mut changed = self.executable.clone();
        if changed.wait_for(|changed| *changed).await.is_err() {
            return pending().await;
        }
        self.log
            .warn("The executable this agent started from has changed, so the agent ends.");
        self.exit.set(Some(EXECUTABLE_CHANGED));
    }

    /// answers a method that needs no particular state, or gives it back.
    fn simple(&self, method: Method) -> Result<ApiResult, Method> {
        match method {
            Method::Status => Ok(Ok(self.snapshot_value())),
            Method::TokenSet { key, token } => {
                self.log.mask(&token);
                Ok(match self.tokens.set(&key, &token) {
                    Ok(()) => {
                        self.publish();
                        Ok(json!({ "repository": key.as_str() }))
                    }
                    Err(error) => Err(ApiError::store_failed("saved", &error)),
                })
            }
            Method::TokenRemove { key } => Ok(match self.tokens.remove(&key) {
                Ok(()) => {
                    self.publish();
                    Ok(json!({ "repository": key.as_str() }))
                }
                Err(error) => Err(ApiError::store_failed("removed", &error)),
            }),
            other => Err(other),
        }
    }

    fn snapshot_value(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or(serde_json::Value::Null)
    }

    fn busy(&self) -> ApiResult {
        Err(ApiError::new(
            ErrorCode::Busy,
            format!("the agent is {}", self.phase_name()),
        ))
    }

    fn state_result(&self) -> serde_json::Value {
        json!({ "state": self.phase_name() })
    }

    /// reads the configuration again into the agent's record; the reason is why it
    /// could not be read.
    fn read_configuration(&self) -> Result<Plan, Reason> {
        let result = self.read_configuration_file();
        // the read replaced what the snapshot shows, so subscribers are told now, even
        // when what follows refuses the request that asked for it
        self.publish();
        result
    }

    fn read_configuration_file(&self) -> Result<Plan, Reason> {
        let path = &self.settings.config_path;
        // the configuration lives outside any checkout, so the rule is applied from each
        // diagnostic directory's own place
        let loaded = load_plan_under(path, Profile::Axpc, true);
        match loaded.outcome {
            Ok(plan) => {
                {
                    let mut inner = self.inner.borrow_mut();
                    inner.plan = Some(plan.clone());
                    inner.problems = Vec::new();
                }
                self.refresh_tokens(&plan);
                Ok(plan)
            }
            Err(error) => {
                let missing = matches!(error, LoadError::Unreadable(_)) && !path.exists();
                let problems = if missing {
                    Vec::new()
                } else {
                    error.into_problems()
                };
                let reason = if missing {
                    Reason::new(
                        Reason::NO_CONFIGURATION,
                        format!("there is no configuration at {}", path.display()),
                    )
                } else {
                    let text = scrub_error_text(&problems.join("; "), "");
                    Reason::new(
                        Reason::INVALID_CONFIGURATION,
                        format!("the configuration is not valid: {text}"),
                    )
                };
                let mut inner = self.inner.borrow_mut();
                inner.plan = None;
                inner.problems = problems;
                Err(reason)
            }
        }
    }

    fn refresh_tokens(&self, plan: &Plan) {
        let keys: Vec<RepositoryKey> = plan
            .entries
            .iter()
            .map(|entry| RepositoryKey::of(&entry.owner, &entry.repository))
            .collect();
        self.tokens.refresh(&keys);
    }

    fn core(&self) -> AgentCore<'_, B, K, R> {
        AgentCore {
            backend: self.backend,
            clock: self.clock,
            registrar: self.registrar,
            tokens: self.tokens,
            log: self.log,
            board: &self.board,
            admitted: &self.admitted,
            drain: &self.drain,
            times: self.settings.times,
        }
    }

    fn steps(&self) -> Steps<'_, B> {
        Steps {
            backend: self.backend,
            log: self.log,
            times: self.settings.times,
            force: &self.force,
            containers_removed: &self.containers_removed,
            left: Some(&self.left),
        }
    }

    async fn container_steps(&self, containers: Vec<String>) {
        steps::container_steps(&self.steps(), containers).await;
    }

    /// keeps why each pool was skipped, and empties the board.
    fn capture_skips(&self) {
        let skips = self
            .board
            .pool_rows()
            .into_iter()
            .filter_map(|row| match row.state {
                PoolState::Skipped(reason) => Some((row.name, reason)),
                _ => None,
            })
            .collect();
        self.inner.borrow_mut().skips = skips;
        self.board.clear();
    }

    /// the state after a stop: ended when an end was asked for, and otherwise stopped
    /// for `reason`.
    fn after_stop(&self, reason: Reason) -> Next {
        if self.exit.get().is_some() {
            self.set_phase(Phase::Quitting);
            Next::Exit
        } else {
            self.set_phase(Phase::Stopped(reason));
            Next::Idle
        }
    }

    async fn stopped(&self, requests: &mut Requests) -> Next {
        loop {
            tokio::select! {
                biased;
                () = self.exit_asked() => {
                    self.set_phase(Phase::Quitting);
                    return Next::Exit;
                }
                call = requests.recv() => {
                    let Some(call) = call else {
                        self.requests_closed();
                        return Next::Exit;
                    };
                    match self.route(call) {
                        Routed::Answered => {}
                        Routed::Begin(Method::DrainForUpdate { version, .. }, reply) => {
                            return self.end_stopped_for_update(version, reply);
                        }
                        Routed::Begin(Method::Build, reply) => return Next::Build(reply),
                        Routed::Begin(_, reply) => return Next::Resume(Some(reply)),
                        Routed::Quit(reply) => {
                            self.set_phase(Phase::Quitting);
                            self.reply_state(reply);
                            return Next::Exit;
                        }
                        Routed::Stop(reply) => self.reply_state(reply),
                    }
                }
            }
        }
    }

    /// reads the configuration again and checks the image, answering `reply` with
    /// where that left the agent.
    async fn resume(&self, requests: &mut Requests, reply: Option<Reply>) -> Next {
        self.policy.borrow_mut().clear();
        self.set_phase(Phase::Resuming);
        self.log.info("Resuming: reading the configuration again.");
        let (next, result) = match self.serve(requests, true, self.reload()).await {
            None => {
                let next = self.after_stop(Reason::by_you());
                (next, json!({ "state": self.phase_name() }))
            }
            Some(Reloaded::Run(plan)) => (Next::Run(plan), json!({ "state": "resuming" })),
            Some(Reloaded::Stopped(reason)) => {
                self.log
                    .warn(&format!("The agent is stopped: {}.", reason.message));
                let value = json!({ "state": "stopped", "reason": reason.wire() });
                self.set_phase(Phase::Stopped(reason));
                (Next::Idle, value)
            }
        };
        if let Some(reply) = reply {
            let _ = reply.send(Ok(result));
        }
        next
    }

    async fn reload(&self) -> Reloaded {
        let plan = match self.read_configuration() {
            Ok(plan) => plan,
            Err(reason) => return Reloaded::Stopped(reason),
        };
        let Some(image) = plan.image_name.as_deref() else {
            return Reloaded::Stopped(Reason::new(
                Reason::INVALID_CONFIGURATION,
                "the configuration names no image",
            ));
        };
        let check = check_image(self.backend, &self.board, image).await;
        match check.refusal(image) {
            Some(refusal) => Reloaded::Stopped(Reason::new(Reason::IMAGE_UNUSABLE, refusal)),
            None => Reloaded::Run(plan),
        }
    }

    async fn running(&self, plan: &Plan, requests: &mut Requests) -> Next {
        self.inner.borrow_mut().skips.clear();
        // the agent is resuming until the core has admitted and prepared the pools
        self.admitted.send_replace(false);
        let mut admitted = self.admitted.subscribe();
        let mut resuming = true;
        self.backend
            .retained()
            .retain_unmanaged(|pool| plan.entries.iter().any(|entry| entry.name == pool));
        let (stop, stop_receiver) = watch::channel(false);
        let mut cycle = Box::pin(self.core_cycle(plan, stop_receiver));
        let mut ticker = ticker();
        let mut draining: Option<Draining> = None;
        loop {
            let deadline = draining.as_ref().map(|drain| drain.deadline);
            tokio::select! {
                biased;
                () = self.exit_asked() => {
                    self.wind_down(&stop, cycle, requests).await;
                    return self.after_stop(Reason::by_you());
                }
                end = &mut cycle => {
                    if matches!(end, CycleEnd::Drained) && draining.is_none() {
                        // the drain was cancelled while the core ended for it, so the
                        // runners run again
                        self.log.info("The drain was cancelled as the runtime core ended, so it starts again.");
                        cycle = Box::pin(self.core_cycle(plan, stop.subscribe()));
                        continue;
                    }
                    if let Some(drain) = &draining {
                        // nothing runs any more; the retained diagnostic containers are
                        // finalised as a stop does
                        self.capture_skips();
                        let containers = self.backend.running_containers();
                        self.serve(requests, false, self.container_steps(containers)).await;
                        return self.end_for_update(&drain.version, false);
                    }
                    return self.cycle_ended(end);
                }
                () = drain_deadline(deadline) => {
                    if let Some(drain) = draining.take() {
                        self.log.warn(&format!(
                            "The drain for the update to {} reached its bound of {} s, so the remaining runners are stopped.",
                            drain.version,
                            self.settings.drain_bound.as_secs()
                        ));
                        self.wind_down(&stop, cycle, requests).await;
                        return self.end_for_update(&drain.version, false);
                    }
                }
                () = requester_closed(&mut draining) => {
                    if let Some(drain) = draining.take() {
                        self.log.info(&format!(
                            "The connection that asked for the drain for the update to {} closed, so the drain is cancelled and the runners register again.",
                            drain.version
                        ));
                        self.inner.borrow_mut().drain = None;
                        self.drain.send_replace(false);
                        self.publish();
                    }
                }
                _ = admitted.wait_for(|admitted| *admitted), if resuming => {
                    resuming = false;
                    // the pools are prepared, which removed the stale containers that
                    // could be, so what an earlier stop could not remove is stale now
                    self.left.borrow_mut().clear();
                    self.set_phase(Phase::Running);
                }
                _ = ticker.tick() => {
                    if let Some(drain) = &draining
                        && self.drain_progress()
                    {
                        self.log.info(&format!(
                            "No runner is busy any more, so the agent ends for the update to {}.",
                            drain.version
                        ));
                        let version = drain.version.clone();
                        self.wind_down(&stop, cycle, requests).await;
                        return self.end_for_update(&version, false);
                    }
                    self.refresh();
                }
                call = requests.recv() => {
                    let Some(call) = call else {
                        self.requests_closed();
                        self.wind_down(&stop, cycle, requests).await;
                        return self.after_stop(Reason::by_you());
                    };
                    match self.route(call) {
                        Routed::Answered => {}
                        Routed::Stop(reply) => {
                            self.wind_down(&stop, cycle, requests).await;
                            let next = match &draining {
                                Some(drain) => self.end_for_update(&drain.version, true),
                                None => self.after_stop(Reason::by_you()),
                            };
                            self.reply_state(reply);
                            return next;
                        }
                        Routed::Quit(reply) => {
                            self.wind_down(&stop, cycle, requests).await;
                            let next = self.after_stop(Reason::by_you());
                            self.reply_state(reply);
                            return next;
                        }
                        Routed::Begin(Method::DrainForUpdate { version, requester }, reply) => {
                            draining = Some(self.begin_drain(version, requester, reply));
                        }
                        Routed::Begin(_, reply) => self.reply_state(reply),
                    }
                }
            }
        }
    }

    /// starts a drain for an update to `version` and answers `reply` with it.
    fn begin_drain(
        &self,
        version: String,
        requester: Option<oneshot::Receiver<()>>,
        reply: Reply,
    ) -> Draining {
        let bound = self.settings.drain_bound;
        self.log.info(&format!(
            "Draining for the update to {version}: nothing registers, idle runners are stopped, and busy runners finish their jobs, for at most {} s.",
            bound.as_secs()
        ));
        self.inner.borrow_mut().drain = Some(Drain {
            version: version.clone(),
            waiting: self.waited_for(),
            deadline: SystemTime::now() + bound,
        });
        self.drain.send_replace(true);
        self.publish();
        let _ = reply.send(Ok(self.drain_answer()));
        Draining {
            version,
            deadline: Instant::now() + bound,
            requester,
        }
    }

    /// ends the agent at once for an update to `version` asked for while the runners
    /// are stopped, stopped by you when the stop was yours, and answers `reply`.
    fn end_stopped_for_update(&self, version: String, reply: Reply) -> Next {
        let by_you = matches!(
            &self.inner.borrow().phase,
            Phase::Stopped(reason) if reason.code == Reason::BY_YOU
        );
        self.log.info(&format!(
            "A drain for the update to {version} was asked for while the runners are stopped, so the agent ends for it now."
        ));
        self.exit.set(Some(UPDATE));
        *self.update.borrow_mut() = Some(UpdateEnding::new(&version, by_you));
        self.inner.borrow_mut().drain = Some(Drain {
            version,
            waiting: 0,
            deadline: SystemTime::now(),
        });
        self.set_phase(Phase::Quitting);
        let _ = reply.send(Ok(self.drain_answer()));
        Next::Exit
    }

    /// the answer to `drain_for_update`: the state and the drain the snapshot shows.
    fn drain_answer(&self) -> serde_json::Value {
        json!({
            "state": self.phase_name(),
            "update": self.snapshot().agent.update,
        })
    }

    /// the runners a drain waits for: every slot still running a container, which
    /// includes an idle runner until its stop ends and one whose stop and kill failed.
    fn waited_for(&self) -> u32 {
        let waited = self
            .board
            .slot_rows()
            .into_iter()
            .filter(|row| matches!(row.state, SlotState::Running(_)))
            .count();
        u32::try_from(waited).unwrap_or(u32::MAX)
    }

    /// refreshes the count a drain waits for, and says whether every slot is drained.
    fn drain_progress(&self) -> bool {
        let waiting = self.waited_for();
        if let Some(drain) = self.inner.borrow_mut().drain.as_mut() {
            drain.waiting = waiting;
        }
        self.board
            .slot_rows()
            .iter()
            .all(|row| row.state == SlotState::Drained)
    }

    /// ends the agent for an update to `version`, once its runners have stopped.
    fn end_for_update(&self, version: &str, stopped_by_you: bool) -> Next {
        self.inner.borrow_mut().drain = None;
        if self.exit.get().is_none() {
            let stopped_by_you = stopped_by_you || self.stopped_in_drain.get();
            self.exit.set(Some(UPDATE));
            *self.update.borrow_mut() = Some(UpdateEnding::new(version, stopped_by_you));
        }
        self.after_stop(Reason::by_you())
    }

    /// what a core that ended without a stop means for the agent.
    fn cycle_ended(&self, end: CycleEnd) -> Next {
        self.capture_skips();
        let reason = match end {
            CycleEnd::StopRequested | CycleEnd::Drained => Reason::by_you(),
            CycleEnd::LimitReached => {
                self.log.warn(
                    "The runtime core failed too often, so the agent is stopped. \
                     Start all runners again to resume.",
                );
                Reason::new(
                    Reason::RESTART_LIMIT,
                    "the runtime core failed too often within 10 minutes",
                )
            }
            CycleEnd::NothingToRun => Reason::new(Reason::NO_POOL_STARTED, "no pool could start"),
            CycleEnd::ImageRefused(refusal) => {
                self.log.warn(&format!("The agent is stopped: {refusal}."));
                Reason::new(Reason::IMAGE_UNUSABLE, refusal)
            }
        };
        self.after_stop(reason)
    }

    /// asks the core to stop and runs the container steps within the exit deadline,
    /// dropping whatever still runs; the containers are read first, because dropping
    /// a worker forgets its container.
    async fn wind_down<F: Future<Output = CycleEnd>>(
        &self,
        stop: &watch::Sender<bool>,
        cycle: Pin<Box<F>>,
        requests: &mut Requests,
    ) {
        let containers = self.backend.running_containers();
        let _ = stop.send(true);
        self.set_phase(Phase::Stopping);
        let steps_done = Cell::new(false);
        let work = async {
            let steps = async {
                self.container_steps(containers).await;
                steps_done.set(true);
            };
            let both = async {
                tokio::join!(steps, cycle);
            };
            if timeout(self.settings.times.exit_deadline, both)
                .await
                .is_err()
            {
                self.log.warn(&format!(
                    "The stop did not finish within {} s, so what still runs is ended now.",
                    self.settings.times.exit_deadline.as_secs()
                ));
                if !steps_done.get() {
                    self.containers_removed.set(false);
                }
            }
        };
        self.serve(requests, false, work).await;
        self.capture_skips();
    }

    /// runs `work` while answering calls; with `abortable`, a stop or a quit ends the
    /// wait and returns `None`, and without it the work finishes first.
    async fn serve<T>(
        &self,
        requests: &mut Requests,
        abortable: bool,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let mut work = pin!(work);
        let mut ticker = ticker();
        let mut open = true;
        loop {
            tokio::select! {
                biased;
                output = &mut work => return Some(output),
                () = self.exit_asked() => {
                    if abortable {
                        return None;
                    }
                }
                _ = ticker.tick() => self.refresh(),
                call = requests.recv(), if open => match call {
                    None => open = false,
                    Some(call) => {
                        if self.answer_busy(call) && abortable {
                            return None;
                        }
                    }
                },
            }
        }
    }

    /// answers a call that arrives during a transition; true when it asks to stop or
    /// to end.
    fn answer_busy(&self, call: Call) -> bool {
        match self.route(call) {
            Routed::Answered => false,
            Routed::Stop(reply) => {
                if self.inner.borrow().drain.is_some() {
                    self.stopped_in_drain.set(true);
                }
                let _ = reply.send(Ok(json!({ "state": "stopped" })));
                true
            }
            Routed::Quit(reply) => {
                let _ = reply.send(Ok(json!({ "state": "quitting" })));
                true
            }
            Routed::Begin(_, reply) => {
                self.reply_state(reply);
                false
            }
        }
    }

    /// answers a call the way every phase does, so that a handler keeps only what it
    /// does itself with the methods that change the agent's state: a stateless method
    /// and a refusal are answered here, a quit has set the exit.
    fn route(&self, Call { method, reply }: Call) -> Routed {
        let method = match self.simple(method) {
            Ok(result) => {
                let _ = reply.send(result);
                return Routed::Answered;
            }
            Err(method) => method,
        };
        match self.action(&method) {
            Action::Begin => Routed::Begin(method, reply),
            Action::Stop => Routed::Stop(reply),
            Action::Quit => {
                self.exit.set(Some(QUIT));
                Routed::Quit(reply)
            }
            other => {
                let _ = reply.send(self.answer(other));
                Routed::Answered
            }
        }
    }

    /// the callers are gone, so the agent ends as if it had been asked to quit.
    fn requests_closed(&self) {
        self.exit.set(Some(QUIT));
    }

    fn reply_state(&self, reply: Reply) {
        let _ = reply.send(Ok(self.state_result()));
    }

    /// what the agent is doing now decides what a lifecycle method does.
    fn action(&self, method: &Method) -> Action {
        let inner = self.inner.borrow();
        inner.phase.action(method, inner.drain.is_some())
    }

    /// the answer to a method that changes nothing.
    fn answer(&self, action: Action) -> ApiResult {
        match action {
            Action::Busy => self.busy(),
            Action::Refuse(message) => refused(message),
            _ => Ok(self.state_result()),
        }
    }

    /// runs the core, and runs it again after each failure until the policy gives up
    /// or a stop is asked for.
    async fn core_cycle(&self, plan: &Plan, mut stop: watch::Receiver<bool>) -> CycleEnd {
        loop {
            let result = CatchPanic(Box::pin(self.core().run(plan, stop.clone()))).await;
            let failed_at = self.clock.now();
            if *stop.borrow() {
                return CycleEnd::StopRequested;
            }
            let draining = *self.drain.borrow();
            if draining && matches!(result, Ok(Ok(()))) {
                return CycleEnd::Drained;
            }
            let failure = match result {
                Ok(Err(failure)) => failure,
                Ok(Ok(())) => CoreFailure {
                    message: "the runtime core ended without being asked to".to_string(),
                    containers: Vec::new(),
                    ending: Ending::Ordinary,
                },
                // the panic unwound through the workers, so their containers are
                // still recorded by the backend and nowhere else
                Err(message) => CoreFailure {
                    message,
                    containers: self.backend.running_containers(),
                    ending: Ending::Ordinary,
                },
            };
            let message = scrub_error_text(&failure.message, "");
            match failure.ending {
                Ending::ImageRefused => {
                    self.container_steps(failure.containers).await;
                    return CycleEnd::ImageRefused(message);
                }
                Ending::NothingToRun => {
                    self.log.warn(&message);
                    self.container_steps(failure.containers).await;
                    return CycleEnd::NothingToRun;
                }
                Ending::Ordinary | Ending::PrerequisiteRefused => {}
            }
            self.log
                .warn(&format!("The runtime core failed: {message}"));
            if draining {
                // a drain starts nothing again
                self.container_steps(failure.containers).await;
                return CycleEnd::Drained;
            }
            let verdict = self.policy.borrow_mut().record_failure(failed_at);
            self.container_steps(failure.containers).await;
            match verdict {
                Verdict::Stop => return CycleEnd::LimitReached,
                Verdict::RestartAfter(delay) => {
                    self.log.warn(&format!(
                        "Restarting the runtime core in {} s.",
                        delay.as_secs()
                    ));
                    self.inner.borrow_mut().restart_due = Some(failed_at + delay);
                    self.publish();
                    let mut drain = self.drain.subscribe();
                    let drained = tokio::select! {
                        stopped = sleep_or_stop(self.clock, &mut stop, delay) => {
                            if stopped {
                                self.inner.borrow_mut().restart_due = None;
                                return CycleEnd::StopRequested;
                            }
                            false
                        }
                        _ = drain.wait_for(|draining| *draining) => true,
                    };
                    self.inner.borrow_mut().restart_due = None;
                    if drained {
                        return CycleEnd::Drained;
                    }
                }
            }
        }
    }

    /// builds the image the configuration names while the runners are stopped; the
    /// call is answered once the build has started, and the snapshot tells the rest.
    async fn building(&self, requests: &mut Requests, reply: Reply) -> Next {
        let image = match self.image_to_build() {
            Ok(image) => image,
            Err(message) => {
                let _ = reply.send(refused(message));
                return Next::Idle;
            }
        };
        let started = SystemTime::now();
        self.inner.borrow_mut().build = Some(BuildInfo {
            started: millis(started),
            outcome: "building".to_string(),
            finished: None,
            message: None,
        });
        self.log.info(&format!("Building {image}."));
        self.set_phase(Phase::Building);
        let _ = reply.send(Ok(
            json!({ "state": "building", "started": millis(started) }),
        ));

        let (stop, stop_receiver) = watch::channel(None::<Instant>);
        let mut work = pin!(self.builder.build(&image, self.log, stop_receiver));
        let mut waiting: Vec<Reply> = Vec::new();
        let mut cancelled = false;
        let mut ticker = ticker();
        let mut open = true;
        let outcome = loop {
            let deadline = stop
                .borrow()
                .map(|at| at + self.settings.times.exit_deadline);
            tokio::select! {
                biased;
                outcome = &mut work => break outcome,
                () = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    break Err(format!(
                        "the build did not end within {} s of the stop, so what still runs is \
                         ended now; the image {image} may hold the previous image or the new one",
                        self.settings.times.exit_deadline.as_secs()
                    ));
                }
                () = self.exit_asked() => cancelled |= self.cancel_build(&stop),
                _ = ticker.tick() => self.refresh(),
                call = requests.recv(), if open => match call {
                    None => open = false,
                    Some(call) => match self.route(call) {
                        Routed::Answered => {}
                        Routed::Stop(reply) | Routed::Quit(reply) => {
                            cancelled |= self.cancel_build(&stop);
                            waiting.push(reply);
                        }
                        Routed::Begin(_, reply) => self.reply_state(reply),
                    },
                },
            }
        };
        let reason = self.build_ended(requests, &image, outcome, cancelled).await;
        let next = self.after_stop(reason);
        for reply in waiting {
            let _ = reply.send(Ok(self.state_result()));
        }
        next
    }

    /// asks the build to stop; true when this asked.
    fn cancel_build(&self, stop: &watch::Sender<Option<Instant>>) -> bool {
        if stop.borrow().is_some() {
            return false;
        }
        self.log.info("Stopping: the build is aborted.");
        stop.send_replace(Some(Instant::now()));
        true
    }

    /// the image a build may build now, or why it may not.
    fn image_to_build(&self) -> Result<String, String> {
        let plan = self.read_configuration().map_err(|reason| {
            format!(
                "the image name comes from the configuration, which is not usable: {}",
                reason.message
            )
        })?;
        match plan.image_name {
            None => Err("the configuration names no image to build".to_string()),
            Some(image) if image.contains('@') => Err(format!(
                "the image name {image} names a digest, which cannot be tagged; give it a tag instead"
            )),
            Some(image) => Ok(image),
        }
    }

    /// records how the build ended and the image it left, and says why the agent is
    /// stopped; the image is checked while calls are still answered, and a stop or a
    /// quit during the build skips the check or during the check abandons it.
    async fn build_ended(
        &self,
        requests: &mut Requests,
        image: &str,
        outcome: Result<(), String>,
        cancelled: bool,
    ) -> Reason {
        let finished = Some(millis(SystemTime::now()));
        let (word, message, reason) = match outcome {
            Ok(()) => ("ready", None, Reason::by_you()),
            Err(error) => {
                let error = scrub_error_text(&error, "");
                if cancelled {
                    ("cancelled", Some(error), Reason::by_you())
                } else {
                    self.log.warn(&format!("The image build failed: {error}"));
                    let reason = Reason::new(
                        Reason::IMAGE_BUILD_FAILED,
                        format!("the image build failed: {error}"),
                    );
                    ("failed", Some(error), reason)
                }
            }
        };
        if word == "ready" && !cancelled {
            self.serve(
                requests,
                true,
                check_image(self.backend, &self.board, image),
            )
            .await;
        }
        if let Some(build) = self.inner.borrow_mut().build.as_mut() {
            build.outcome = word.to_string();
            build.finished = finished;
            build.message = message;
        }
        reason
    }
}
