//! the instance: the state it is in, and what each request and each failure of the
//! runtime core does to it, as `docs/operations/cli.md` describes; one task owns the
//! state, so a request that arrives during a transition is answered at once: `status`
//! tells the truth, `stop` is taken, and `restart`, `diagnostics` and `resume` are
//! refused until the transition ends.

use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::{Pin, pin};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};

use crate::macos::network::{NetworkReading, PrimaryNetwork};
use crate::macos::prerequisites::{RESUME_VERSION_BUDGET, VERSION_BUDGET};
use crate::macos::reason::sleep_reason;
use crate::macos::watch::NetworkChange;
use crate::profile::Profile;
use crate::runtime::backend::{Backend, DiagnosticRecord, REMOVAL_WITHIN};
use crate::runtime::clock::Clock;
use crate::runtime::diagnostic::{self, Flushed};
use crate::runtime::log::Log;
use crate::runtime::pool::diagnostic_records;
use crate::runtime::scrub::scrub_error_text;
use crate::runtime::worker::sleep_or_stop;
use crate::validate::Plan;

use super::protocol::{Incoming, Request, Response};
use super::restart::{RestartPolicy, Verdict, WINDOW};
use super::steps;

/// the times of the stop sequence, each counted from the step's own start except
/// the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopTimes {
    /// the grace `docker stop` gives each container before it kills it.
    pub grace: Duration,
    /// the stop call is ended at this budget.
    pub stop_budget: Duration,
    /// the removal of what remains is ended at this budget, so the container steps
    /// take 26 s at the most.
    pub remove_budget: Duration,
    /// the release of the prepared job hosts, which follows the container steps and
    /// is ended at this budget; zero when the backend has no hosts.
    pub release_budget: Duration,
    /// counted from the stop request; whatever still runs then is dropped, which
    /// kills the clients, and the instance ends.
    pub exit_deadline: Duration,
}

impl Default for StopTimes {
    fn default() -> Self {
        Self {
            grace: Duration::from_secs(15),
            stop_budget: Duration::from_secs(20),
            remove_budget: Duration::from_secs(6),
            release_budget: Duration::ZERO,
            exit_deadline: Duration::from_secs(28),
        }
    }
}

/// the time a stop gives the container stop call to kill what ignores its grace.
const KILL_MARGIN: Duration = Duration::from_secs(5);

impl StopTimes {
    /// the times for a backend that reserves `release` of the sequence to release its
    /// job hosts: the container stop gives up the time, so that the stop, the removal
    /// and the release together fit the exit deadline. No release leaves the default
    /// times as they are.
    #[must_use]
    pub fn reserving(release: Duration) -> Self {
        let times = Self::default();
        let stop_budget = times.stop_budget.min(
            times
                .exit_deadline
                .saturating_sub(times.remove_budget)
                .saturating_sub(release),
        );
        Self {
            grace: times.grace.min(stop_budget.saturating_sub(KILL_MARGIN)),
            stop_budget,
            release_budget: release,
            ..times
        }
    }

    /// how much earlier the diagnostic column's deadlines fall so that the release
    /// still has its budget before the exit deadline: the release takes the slack the
    /// column leaves there first, and the column gives up the rest.
    #[must_use]
    pub fn diagnostic_shortening(&self) -> Duration {
        let slack = self.exit_deadline.saturating_sub(REMOVAL_WITHIN);
        self.release_budget.saturating_sub(slack)
    }
}

/// why the instance is stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// the configuration failed validation at a restart or a resume.
    InvalidConfiguration(String),
    /// the runtime core failed more often than the window allows.
    RestartLimit,
    /// the configured image's contract is not this CLI's, or the image is missing;
    /// the text says which and how to rebuild it.
    Image(String),
    /// a host prerequisite refused at a restart or a resume; the text is the
    /// refusal, which names the host document's section.
    Prerequisite(String),
    /// the Mac slept, or woke: a wake is when the instance learns of the sleep.
    Slept,
    /// the primary network differed from its record on two checks in a row.
    NetworkChanged(NetworkChange),
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(problem) => {
                write!(f, "the configuration is not valid: {problem}")
            }
            Self::RestartLimit => write!(
                f,
                "the runtime core failed {} times within {} minutes",
                super::restart::DELAYS.len() + 1,
                WINDOW.as_secs() / 60
            ),
            Self::Image(reason) | Self::Prerequisite(reason) => f.write_str(reason),
            Self::Slept => f.write_str(&sleep_reason()),
            Self::NetworkChanged(change) => f.write_str(&change.reason()),
        }
    }
}

/// where the instance is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Running,
    Stopping,
    Resuming,
    Stopped(StopReason),
}

impl State {
    /// the word `status` and a refusal use for the state.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Resuming => "resuming",
            Self::Stopped(_) => "stopped",
        }
    }
}

/// what a failed run of the runtime core means for the instance, and so how the
/// instance ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// a failure like any other, which the restart policy counts; for the instance,
    /// an end for any other reason, such as a stop.
    Ordinary,
    /// no pool could start; at the first run of the core this ends the instance, at
    /// a later run it is a failure like any other.
    NothingToRun,
    /// the configured image cannot run jobs; at the first run of the core this ends
    /// the instance, at a later run it stops it, and it is never counted as a
    /// failure.
    ImageRefused,
    /// a host prerequisite refused before the first run of the core, so the instance
    /// ended without starting anything.
    PrerequisiteRefused,
}

/// how the runtime core ended when no stop was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFailure {
    /// what went wrong; it never holds a secret.
    pub message: String,
    /// the containers the core had started and not seen end; the core is gone, so
    /// only it can say, and the instance stops and removes them.
    pub containers: Vec<String>,
    /// what the failure means for the instance; for a refused image, `message` says
    /// why.
    pub ending: Ending,
}

/// the part of `start` that runs the pools; a panic is a failure, which the
/// instance turns into a [`CoreFailure`].
pub trait Core {
    /// runs until `stop` becomes true, then returns `Ok`; returning in any other way,
    /// with or without an error, is a failure of the core.
    fn run(
        &self,
        plan: &Plan,
        stop: watch::Receiver<bool>,
    ) -> impl Future<Output = Result<(), CoreFailure>>;

    /// checks the image `plan` names once, without waiting for Docker; returns why
    /// the image cannot run jobs, or `Ok` when it can or Docker could not say, since
    /// the next run checks again.
    fn check_image(&self, plan: &Plan) -> impl Future<Output = Result<(), String>>;

    /// checks the host prerequisites of the pools `plan` runs once, without waiting for
    /// anything; returns the refusal's message, or `Ok` when they pass or nothing in
    /// `plan` needs them. A core without prerequisites passes. Each program the check
    /// runs gets `each_read` to answer.
    fn check_prerequisites(
        &self,
        _plan: &Plan,
        _each_read: Duration,
    ) -> impl Future<Output = Result<(), String>> {
        async { Ok(()) }
    }

    /// what the core is doing now, as `status` fields.
    fn report(&self) -> Vec<(&'static str, String)>;
}

/// the instance's own configuration file, read again at a restart or a resume; it
/// takes no path, so no request can name another.
pub trait Source {
    /// the validated plan, or every problem that stops it from being one.
    fn load(&self) -> impl Future<Output = Result<Plan, Vec<String>>>;
}

/// the signals that stop every runner, and the keep-awake hold, which the instance
/// runs beside its core. The instance asks it to `begin` each time it starts to run,
/// including after a resume, and polls [`watch`](Monitor::watch) while it runs.
pub trait Monitor {
    /// arms a new run, before the core starts: the sleep baseline is set again, and a
    /// primary network is recorded afresh by the next [`watch`](Monitor::watch).
    fn begin(&self);

    /// watches the clocks and the primary network until one of them stops every
    /// runner, and returns why. It never ends otherwise, and dropping it ends the
    /// watch.
    fn watch(&self) -> impl Future<Output = StopReason>;

    /// keeps the keep-awake hold in order while the instance is not running its
    /// core: resuming, and stopping until teardown ends. It never ends.
    fn maintain(&self) -> impl Future<Output = Infallible>;

    /// takes the keep-awake hold, as the instance begins to run or resume.
    fn hold(&self);

    /// ends the keep-awake hold, as the instance stops.
    fn release(&self);

    /// what the monitor reports to `status`.
    fn report(&self) -> Vec<(&'static str, String)>;

    /// the primary network last recorded while running, which a resume after a signal
    /// stop waits for; `None` from a monitor that reads no network.
    fn recorded(&self) -> Option<PrimaryNetwork> {
        None
    }

    /// waits one network check and reads the primary network, while a resume waits in
    /// stopped; it never ends on a monitor that reads no network.
    fn next_reading(&self) -> impl Future<Output = NetworkReading> {
        std::future::pending()
    }
}

/// the monitor of a profile that has no signals.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoMonitor;

impl Monitor for NoMonitor {
    fn begin(&self) {}

    fn watch(&self) -> impl Future<Output = StopReason> {
        std::future::pending()
    }

    fn maintain(&self) -> impl Future<Output = Infallible> {
        std::future::pending()
    }

    fn hold(&self) {}

    fn release(&self) {}

    fn report(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

/// what the instance was started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// the canonical path of the configuration file, the only one a resume may name.
    pub config_path: String,
    pub profile: Profile,
}

/// how the instance ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    /// whether every removal of ordinary containers in the instance's life
    /// succeeded.
    pub containers_removed: bool,
    /// why the instance ended: what its first run of the core found, or
    /// [`Ending::Ordinary`] when it ended otherwise.
    pub ending: Ending,
    /// the diagnostic containers the backend still retains, which the next start
    /// finalises: kept by a forced stop, or not yet removed.
    pub kept: Vec<String>,
}

enum Next {
    Run(Plan),
    Idle,
    Exit,
}

enum CycleEnd {
    StopRequested,
    LimitReached,
    NothingToRun,
    ImageRefusedAtStart(String),
    ImageRefused(String),
}

enum Reloaded {
    Run(Plan),
    Invalid(Vec<String>),
    PrerequisiteRefused(String),
    ImageRefused(String),
}

/// polls the core, turning a panic into an error carrying its message, so a
/// panicking core is a failure rather than the end of the process.
pub(crate) struct CatchPanic<F>(pub(crate) Pin<Box<F>>);

impl<F: Future> Future for CatchPanic<F> {
    type Output = Result<F::Output, String>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.get_mut().0.as_mut();
        match catch_unwind(AssertUnwindSafe(|| inner.poll(context))) {
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(payload) => Poll::Ready(Err(panic_message(payload.as_ref()))),
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_string());
    format!("the runtime core panicked: {detail}")
}

type Requests = mpsc::UnboundedReceiver<Incoming>;

/// a `diagnostics` request under way, and where its answer goes.
type Flush<'f> = Option<(
    Pin<Box<dyn Future<Output = Response> + 'f>>,
    oneshot::Sender<Response>,
)>;

async fn flushed(flush: &mut Flush<'_>) -> Response {
    match flush {
        Some((future, _)) => future.as_mut().await,
        None => std::future::pending().await,
    }
}

/// the answer to `diagnostics`: a `pool` field per pool, a `container` field per
/// container line, and a `result` of `removed`, `incomplete` or `nothing`.
fn flushed_response(flushed: &Flushed) -> Response {
    let response = Response::ok("");
    if flushed.nothing {
        return response.with("result", "nothing");
    }
    let mut response = response;
    for pool in &flushed.pools {
        response = response.with("pool", &pool.line());
    }
    for line in &flushed.containers {
        response = response.with("container", line);
    }
    response.with(
        "result",
        if flushed.complete {
            "removed"
        } else {
            "incomplete"
        },
    )
}

pub struct Instance<'a, B, K, C, S, M = NoMonitor> {
    settings: Settings,
    backend: &'a B,
    clock: &'a K,
    core: &'a C,
    source: &'a S,
    monitor: &'a M,
    log: &'a dyn Log,
    times: StopTimes,
    state: RefCell<State>,
    policy: RefCell<RestartPolicy>,
    exit_requested: Cell<bool>,
    containers_removed: Cell<bool>,
    core_has_run: Cell<bool>,
    ending: Cell<Ending>,
    force: watch::Sender<bool>,
    /// the diagnostics-enabled pools of the plan the core runs, or ran last.
    enabled: RefCell<Vec<DiagnosticRecord>>,
}

impl<'a, B: Backend, K: Clock, C: Core, S: Source> Instance<'a, B, K, C, S> {
    /// an instance without signals, as axpc runs.
    #[must_use]
    pub fn new(
        settings: Settings,
        backend: &'a B,
        clock: &'a K,
        core: &'a C,
        source: &'a S,
        log: &'a dyn Log,
    ) -> Self {
        Self {
            settings,
            backend,
            clock,
            core,
            source,
            monitor: &NoMonitor,
            log,
            times: StopTimes::reserving(backend.release_budget()),
            state: RefCell::new(State::Running),
            policy: RefCell::new(RestartPolicy::default()),
            exit_requested: Cell::new(false),
            containers_removed: Cell::new(true),
            core_has_run: Cell::new(false),
            ending: Cell::new(Ending::Ordinary),
            force: watch::Sender::new(false),
            enabled: RefCell::new(Vec::new()),
        }
    }

    /// the instance with the signals of `monitor`.
    #[must_use]
    pub fn with_monitor<M: Monitor>(self, monitor: &'a M) -> Instance<'a, B, K, C, S, M> {
        Instance {
            settings: self.settings,
            backend: self.backend,
            clock: self.clock,
            core: self.core,
            source: self.source,
            monitor,
            log: self.log,
            times: self.times,
            state: self.state,
            policy: self.policy,
            exit_requested: self.exit_requested,
            containers_removed: self.containers_removed,
            core_has_run: self.core_has_run,
            ending: self.ending,
            force: self.force,
            enabled: self.enabled,
        }
    }
}

impl<'a, B: Backend, K: Clock, C: Core, S: Source, M: Monitor> Instance<'a, B, K, C, S, M> {
    /// the instance with other stop times, so a test can reach the exit deadline
    /// quickly; a debug build only.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    #[must_use]
    pub fn with_stop_times(mut self, times: StopTimes) -> Self {
        self.times = times;
        self
    }

    /// skips the grace period, force-removing what remains at once while every
    /// diagnostic container is ended and kept without its export; a second Ctrl+C or
    /// SIGINT asks for it.
    pub fn force_removal(&self) {
        self.force.send_replace(true);
        self.backend.retained().force();
    }

    /// runs the instance with its first plan until a stop is asked for, answering
    /// every request that reaches `requests`; when the channel closes while the
    /// instance is stopped, nothing can resume it, so it ends.
    pub async fn run(&self, plan: Plan, requests: &mut Requests) -> Ended {
        let checking = self.core.check_prerequisites(&plan, VERSION_BUDGET);
        let mut next = match self.serve(requests, true, checking).await {
            None => Next::Exit,
            Some(Ok(())) => Next::Run(plan),
            Some(Err(refusal)) => {
                self.log.warn(&format!("The instance ends: {refusal}"));
                self.ending.set(Ending::PrerequisiteRefused);
                Next::Exit
            }
        };
        loop {
            next = match next {
                Next::Run(plan) => self.running(&plan, requests).await,
                Next::Idle => self.stopped(requests).await,
                Next::Exit => {
                    self.monitor.release();
                    return Ended {
                        containers_removed: self.containers_removed.get(),
                        ending: self.ending.get(),
                        kept: self.backend.retained().names(),
                    };
                }
            };
        }
    }

    /// moves the instance to `state`; the keep-awake hold follows the state: held
    /// when running or resuming, kept through stopping, and released on entering
    /// stopped.
    fn set_state(&self, state: State) {
        match &state {
            State::Running | State::Resuming => self.monitor.hold(),
            State::Stopped(_) => self.monitor.release(),
            State::Stopping => {}
        }
        *self.state.borrow_mut() = state;
    }

    fn status(&self) -> Response {
        let state = self.state.borrow();
        let mut response = Response::ok("").with("state", state.name());
        if let State::Stopped(reason) = &*state {
            response = response.with("reason", &reason.to_string());
        }
        response = response
            .with("config", &self.settings.config_path)
            .with("profile", self.settings.profile.name());
        for (name, value) in self.core.report().into_iter().chain(self.monitor.report()) {
            response = response.with(name, &value);
        }
        response
    }

    /// the stop reason for an image that cannot run jobs; `build` is refused while
    /// the instance exists, so the instance is stopped first.
    fn image_reason(&self, refusal: &str) -> StopReason {
        let config = &self.settings.config_path;
        StopReason::Image(format!(
            "{refusal}; run `marshal stop`, then `marshal build --config {config}`, \
             then `marshal start --config {config}`"
        ))
    }

    fn refuse_busy(&self) -> Response {
        let name = self.state.borrow().name();
        Response::refused(&format!("the instance is {name}")).with("state", name)
    }

    async fn flush_diagnostics(&self) -> Response {
        let enabled = self.enabled.borrow().clone();
        flushed_response(&diagnostic::flush(self.backend, self.log, &enabled).await)
    }

    async fn running(&self, plan: &Plan, requests: &mut Requests) -> Next {
        self.set_state(State::Running);
        *self.enabled.borrow_mut() = diagnostic_records(plan);
        self.backend
            .retained()
            .retain_unmanaged(|pool| plan.entries.iter().any(|entry| entry.name == pool));
        let (stop, stop_receiver) = watch::channel(false);
        self.monitor.begin();
        let mut watching = Box::pin(self.monitor.watch());
        let mut cycle = Box::pin(self.core_cycle(plan, stop_receiver));
        let mut flush: Flush<'_> = None;
        let mut open = true;
        loop {
            tokio::select! {
                response = flushed(&mut flush), if flush.is_some() => {
                    if let Some((_, reply)) = flush.take() {
                        let _ = reply.send(response);
                    }
                }
                end = &mut cycle => {
                    // a `diagnostics` under way finishes first, while the requests
                    // are answered; a stop among them ends the instance after it
                    if let Some((future, reply)) = flush.take() {
                        let response = self
                            .serve(requests, false, future)
                            .await
                            .unwrap_or_else(Response::ending);
                        let _ = reply.send(response);
                    }
                    let next = match end {
                        CycleEnd::LimitReached => {
                            self.log.warn(
                                "The runtime core failed too often, so the instance is stopped. \
                                 Start it again from another terminal to resume it.",
                            );
                            self.set_state(State::Stopped(StopReason::RestartLimit));
                            Next::Idle
                        }
                        CycleEnd::StopRequested => Next::Exit,
                        CycleEnd::NothingToRun => {
                            self.ending.set(Ending::NothingToRun);
                            Next::Exit
                        }
                        CycleEnd::ImageRefusedAtStart(refusal) => {
                            let config = &self.settings.config_path;
                            self.log.warn(&format!(
                                "The instance ends: {refusal}. Run `marshal build --config {config}`, \
                                 then `marshal start --config {config}`."
                            ));
                            self.ending.set(Ending::ImageRefused);
                            Next::Exit
                        }
                        CycleEnd::ImageRefused(refusal) => {
                            let reason = self.image_reason(&refusal);
                            self.log.warn(&format!("The instance is stopped: {reason}."));
                            self.set_state(State::Stopped(reason));
                            Next::Idle
                        }
                    };
                    if self.exit_requested.get() && matches!(next, Next::Idle) {
                        self.set_state(State::Stopping);
                        return Next::Exit;
                    }
                    return next;
                }
                reason = &mut watching => {
                    self.log.warn(&format!("Stopping every runner: {reason}."));
                    self.wind_down(&stop, cycle, flush, requests).await;
                    if self.exit_requested.get() {
                        return Next::Exit;
                    }
                    self.log.warn(&format!("The instance is stopped: {reason}."));
                    self.set_state(State::Stopped(reason));
                    return Next::Idle;
                }
                incoming = requests.recv(), if open => {
                    let Some(incoming) = incoming else {
                        open = false;
                        continue;
                    };
                    match incoming.request {
                        Request::Status => {
                            let _ = incoming.reply.send(self.status());
                        }
                        Request::Resume(_) => {
                            let _ = incoming.reply.send(self.refuse_busy());
                        }
                        Request::Diagnostics if flush.is_some() => {
                            let _ = incoming.reply.send(Response::refused(
                                "a diagnostics request is already under way",
                            ));
                        }
                        Request::Diagnostics => {
                            flush = Some((Box::pin(self.flush_diagnostics()), incoming.reply));
                        }
                        Request::Stop => {
                            let _ = incoming.reply.send(Response::ok("stopping"));
                            self.wind_down(&stop, cycle, flush, requests).await;
                            return Next::Exit;
                        }
                        Request::Restart => {
                            self.wind_down(&stop, cycle, flush, requests).await;
                            if self.exit_requested.get() {
                                let _ = incoming.reply.send(self.refuse_busy());
                                return Next::Exit;
                            }
                            return self.resume(requests, incoming.reply).await;
                        }
                    }
                }
            }
        }
    }

    /// asks the core to stop and runs the container steps within the exit deadline,
    /// dropping whatever still runs; the containers are read first, because dropping
    /// a worker forgets its container.
    async fn wind_down<F: Future<Output = CycleEnd>>(
        &self,
        stop: &watch::Sender<bool>,
        cycle: Pin<Box<F>>,
        flush: Flush<'_>,
        requests: &mut Requests,
    ) {
        let containers = self.backend.running_containers();
        let _ = stop.send(true);
        self.set_state(State::Stopping);
        let steps_done = Cell::new(false);
        let work = async {
            let steps = async {
                self.container_steps(containers).await;
                steps_done.set(true);
            };
            let flushing = async {
                if let Some((future, reply)) = flush {
                    let _ = reply.send(future.await);
                }
            };
            let both = async {
                tokio::join!(steps, cycle, flushing);
            };
            if tokio::time::timeout(self.times.exit_deadline, both)
                .await
                .is_err()
            {
                self.log.warn(&format!(
                    "The stop did not finish within {} s, so what still runs is ended now.",
                    self.times.exit_deadline.as_secs()
                ));
                if !steps_done.get() {
                    self.containers_removed.set(false);
                }
            }
        };
        self.serve(requests, false, work).await;
    }

    async fn stopped(&self, requests: &mut Requests) -> Next {
        loop {
            let Some(incoming) = requests.recv().await else {
                return Next::Exit;
            };
            match incoming.request {
                Request::Status => {
                    let _ = incoming.reply.send(self.status());
                }
                Request::Stop => {
                    let _ = incoming.reply.send(Response::ok("stopping"));
                    self.set_state(State::Stopping);
                    return Next::Exit;
                }
                Request::Restart => return self.resume(requests, incoming.reply).await,
                Request::Diagnostics => {
                    let response = self
                        .serve(requests, false, self.flush_diagnostics())
                        .await
                        .unwrap_or_else(Response::ending);
                    let _ = incoming.reply.send(response);
                    if self.exit_requested.get() {
                        self.set_state(State::Stopping);
                        return Next::Exit;
                    }
                }
                Request::Resume(resume) => {
                    if resume.profile != self.settings.profile
                        || resume.path != self.settings.config_path
                    {
                        let _ = incoming.reply.send(
                            Response::refused(
                                "the instance was started with another configuration path or \
                                 profile, and a resume must name the same",
                            )
                            .with("state", "stopped"),
                        );
                        continue;
                    }
                    return self.resume(requests, incoming.reply).await;
                }
            }
        }
    }

    /// reads and validates the instance's own configuration again, clears the core's
    /// failure count, checks the host prerequisites and the image, and answers `reply`.
    async fn resume(&self, requests: &mut Requests, reply: oneshot::Sender<Response>) -> Next {
        self.policy.borrow_mut().clear();
        self.set_state(State::Resuming);
        self.log.info("Resuming: reading the configuration again.");
        let reload = async {
            let plan = match self.source.load().await {
                Ok(plan) => plan,
                Err(problems) => return Reloaded::Invalid(problems),
            };
            if let Err(refusal) = self
                .core
                .check_prerequisites(&plan, RESUME_VERSION_BUDGET)
                .await
            {
                return Reloaded::PrerequisiteRefused(refusal);
            }
            match self.core.check_image(&plan).await {
                Ok(()) => Reloaded::Run(plan),
                Err(refusal) => Reloaded::ImageRefused(refusal),
            }
        };
        let (next, response) = match self.serve(requests, true, reload).await {
            None => (Next::Exit, Response::refused("the instance is stopping")),
            Some(Reloaded::Run(plan)) => {
                self.set_state(State::Running);
                (
                    Next::Run(plan),
                    Response::ok("running").with("state", "running"),
                )
            }
            Some(Reloaded::Invalid(problems)) => self.stay_stopped(
                StopReason::InvalidConfiguration(scrub_error_text(&problems.join("; "), "")),
            ),
            Some(Reloaded::PrerequisiteRefused(refusal)) => {
                self.stay_stopped(StopReason::Prerequisite(refusal))
            }
            Some(Reloaded::ImageRefused(refusal)) => self.stay_stopped(self.image_reason(&refusal)),
        };
        let _ = reply.send(response);
        next
    }

    /// leaves the instance stopped for `reason`, with the answer a resume gets.
    fn stay_stopped(&self, reason: StopReason) -> (Next, Response) {
        self.log
            .warn(&format!("The instance is stopped: {reason}."));
        let response = Response::refused(&format!("the instance stays stopped: {reason}"))
            .with("state", "stopped");
        self.set_state(State::Stopped(reason));
        (Next::Idle, response)
    }

    /// runs `work` while answering requests; with `abortable`, a `stop` ends the wait
    /// and returns `None`, and without it the work finishes and the stop is left in
    /// `exit_requested` for the caller.
    async fn serve<T>(
        &self,
        requests: &mut Requests,
        abortable: bool,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let mut work = pin!(work);
        let mut upkeep = pin!(self.monitor.maintain());
        let mut open = true;
        loop {
            tokio::select! {
                output = &mut work => return Some(output),
                never = &mut upkeep => match never {},
                incoming = requests.recv(), if open => match incoming {
                    None => open = false,
                    Some(incoming) => {
                        self.answer_busy(incoming);
                        if abortable && self.exit_requested.get() {
                            return None;
                        }
                    }
                },
            }
        }
    }

    /// answers a request that arrives during a transition.
    fn answer_busy(&self, incoming: Incoming) {
        let response = match incoming.request {
            Request::Status => self.status(),
            Request::Stop => {
                self.exit_requested.set(true);
                Response::ok("stopping")
            }
            Request::Restart | Request::Diagnostics | Request::Resume(_) => self.refuse_busy(),
        };
        let _ = incoming.reply.send(response);
    }

    /// runs the core, and runs it again after each failure until the policy gives up
    /// or a stop is asked for.
    async fn core_cycle(&self, plan: &Plan, mut stop: watch::Receiver<bool>) -> CycleEnd {
        loop {
            let result = CatchPanic(Box::pin(self.core.run(plan, stop.clone()))).await;
            let failed_at = self.clock.now();
            let first_run = !self.core_has_run.replace(true);
            if *stop.borrow() {
                return CycleEnd::StopRequested;
            }
            let failure = match result {
                Ok(Err(failure)) => failure,
                Ok(Ok(())) => CoreFailure {
                    message: "the runtime core ended without being asked to".to_string(),
                    containers: Vec::new(),
                    ending: Ending::Ordinary,
                },
                // the panic unwound through the workers, so their containers are
                // still recorded by the backend and nowhere else.
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
                    return if first_run {
                        CycleEnd::ImageRefusedAtStart(message)
                    } else {
                        CycleEnd::ImageRefused(message)
                    };
                }
                Ending::NothingToRun if first_run => {
                    self.log.warn(&message);
                    self.container_steps(failure.containers).await;
                    return CycleEnd::NothingToRun;
                }
                Ending::NothingToRun | Ending::Ordinary | Ending::PrerequisiteRefused => {}
            }
            self.log
                .warn(&format!("The runtime core failed: {message}"));
            let verdict = self.policy.borrow_mut().record_failure(failed_at);
            self.container_steps(failure.containers).await;
            match verdict {
                Verdict::Stop => return CycleEnd::LimitReached,
                Verdict::RestartAfter(delay) => {
                    self.log.warn(&format!(
                        "Restarting the runtime core in {} s.",
                        delay.as_secs()
                    ));
                    if sleep_or_stop(self.clock, &mut stop, delay).await {
                        return CycleEnd::StopRequested;
                    }
                }
            }
        }
    }

    async fn container_steps(&self, containers: Vec<String>) {
        steps::container_steps(&self.steps(), containers).await;
    }

    fn steps(&self) -> steps::Steps<'_, B> {
        steps::Steps {
            backend: self.backend,
            log: self.log,
            times: self.times,
            force: &self.force,
            containers_removed: &self.containers_removed,
            left: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stop_sequence_takes_15_20_6_and_28_seconds() {
        assert_eq!(
            StopTimes::default(),
            StopTimes {
                grace: Duration::from_secs(15),
                stop_budget: Duration::from_secs(20),
                remove_budget: Duration::from_secs(6),
                release_budget: Duration::ZERO,
                exit_deadline: Duration::from_secs(28),
            }
        );
    }

    #[test]
    fn a_backend_without_hosts_keeps_the_default_times() {
        assert_eq!(StopTimes::reserving(Duration::ZERO), StopTimes::default());
    }

    #[test]
    fn a_release_of_ten_seconds_shortens_the_container_stop_to_fit_the_exit_deadline() {
        let times = StopTimes::reserving(Duration::from_secs(10));
        assert_eq!(
            times,
            StopTimes {
                grace: Duration::from_secs(7),
                stop_budget: Duration::from_secs(12),
                remove_budget: Duration::from_secs(6),
                release_budget: Duration::from_secs(10),
                exit_deadline: Duration::from_secs(28),
            }
        );
        assert!(
            times.stop_budget + times.remove_budget + times.release_budget <= times.exit_deadline
        );
    }

    #[test]
    fn a_release_takes_the_slack_before_the_exit_deadline_and_the_diagnostic_column_gives_up_the_rest()
     {
        assert_eq!(
            StopTimes::default().diagnostic_shortening(),
            Duration::ZERO,
            "a backend without hosts keeps the diagnostic column as it is"
        );
        assert_eq!(
            StopTimes::reserving(Duration::from_secs(10)).diagnostic_shortening(),
            Duration::from_secs(6),
            "the 24 s column leaves 4 s before the 28 s deadline"
        );
        assert_eq!(
            StopTimes::reserving(Duration::from_secs(4)).diagnostic_shortening(),
            Duration::ZERO
        );
        let short = StopTimes {
            exit_deadline: Duration::from_millis(200),
            ..StopTimes::default()
        };
        assert_eq!(short.diagnostic_shortening(), Duration::ZERO);
    }

    #[test]
    fn a_release_larger_than_the_sequence_leaves_the_stop_no_time_and_does_not_underflow() {
        let times = StopTimes::reserving(Duration::from_secs(60));
        assert_eq!(times.stop_budget, Duration::ZERO);
        assert_eq!(times.grace, Duration::ZERO);
    }

    #[test]
    fn a_panic_message_is_read_from_either_payload() {
        assert_eq!(panic_message(&"boom"), "the runtime core panicked: boom");
        assert_eq!(
            panic_message(&String::from("bang")),
            "the runtime core panicked: bang"
        );
        assert_eq!(
            panic_message(&7_u8),
            "the runtime core panicked: no message"
        );
    }
}
