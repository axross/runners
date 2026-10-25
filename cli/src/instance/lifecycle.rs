//! The instance: the state it is in, and what each request and each failure of the
//! runtime core does to it.
//!
//! The instance is running or stopped, with stopping and resuming between them.
//! One task owns the state and serves the requests of the control channel, so a
//! request that arrives during a transition is answered at once: `status` tells the
//! truth, `stop` is taken, and `restart` and `resume` are refused until the
//! transition ends. The pools themselves run in a [`Core`], which this module
//! supervises: an error or a panic of the core while no stop was asked for is a
//! failure, and the core is restarted. What starts and stops a process is the
//! caller's.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::{Pin, pin};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};

use crate::profile::Profile;
use crate::runtime::backend::{Backend, StopRequest};
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;
use crate::runtime::scrub::scrub_error_text;
use crate::runtime::worker::sleep_or_stop;
use crate::validate::Plan;

use super::protocol::{Incoming, Request, Response};
use super::restart::{RestartPolicy, Verdict, WINDOW};

/// The times of the stop sequence, each counted from the step's own start except
/// the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopTimes {
    /// The grace `docker stop` gives each container before it kills it.
    pub grace: Duration,
    /// The stop call is ended at this budget.
    pub stop_budget: Duration,
    /// The removal of what remains is ended at this budget, so the container steps
    /// take 26 s at the most.
    pub remove_budget: Duration,
    /// Counted from the stop request: whatever still runs then, the container
    /// steps, the core's workers and their clients, is dropped, which kills the
    /// clients, and the instance ends.
    pub exit_deadline: Duration,
}

impl Default for StopTimes {
    fn default() -> Self {
        Self {
            grace: Duration::from_secs(15),
            stop_budget: Duration::from_secs(20),
            remove_budget: Duration::from_secs(6),
            exit_deadline: Duration::from_secs(28),
        }
    }
}

/// Why the instance is stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The configuration failed validation at a restart or a resume.
    InvalidConfiguration(String),
    /// The runtime core failed more often than the window allows.
    RestartLimit,
    /// The configured image's contract is not this CLI's, or the image is missing;
    /// the text says which and how to rebuild it.
    Image(String),
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
            Self::Image(reason) => f.write_str(reason),
        }
    }
}

/// Where the instance is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Running,
    Stopping,
    Resuming,
    Stopped(StopReason),
}

impl State {
    /// The word `status` and a refusal use for the state.
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

/// How the runtime core ended when no stop was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFailure {
    /// What went wrong; it never holds a secret.
    pub message: String,
    /// The containers the core had started and not seen end. The core is gone, so
    /// only it can say; the instance stops and removes them.
    pub containers: Vec<String>,
    /// No pool could start. At the instance's first run of the core this ends the
    /// instance; at a later run it is a failure like any other.
    pub nothing_to_run: bool,
    /// The configured image cannot run jobs, as `message` says. At the instance's
    /// first run of the core this ends the instance; at a later run it stops it.
    /// Either way it is not counted as a failure.
    pub image_refused: bool,
}

/// The part of `start` that runs the pools.
///
/// A real core prepares the pools and runs their slots until the stop is asked for.
/// A panic is a failure: the instance turns it into a [`CoreFailure`].
pub trait Core {
    /// Runs until `stop` becomes true, then returns `Ok`. Returning in any other
    /// way, with or without an error, is a failure of the core.
    fn run(
        &self,
        plan: &Plan,
        stop: watch::Receiver<bool>,
    ) -> impl Future<Output = Result<(), CoreFailure>>;

    /// Checks the image `plan` names once, without waiting for Docker, as a resume
    /// does before it answers. Returns why the image cannot run jobs, or `Ok` when
    /// it can or Docker could not say: the next run checks again once Docker
    /// answers.
    fn check_image(&self, plan: &Plan) -> impl Future<Output = Result<(), String>>;

    /// What the core is doing now, as `status` fields: each pool's and each slot's
    /// state, and the image's. A core that is not running reports nothing.
    fn report(&self) -> Vec<(&'static str, String)>;
}

/// The instance's own configuration file, read again at a restart or a resume. It
/// takes no path: the file it reads is fixed when it is built, and no request can
/// name another.
pub trait Source {
    /// The validated plan, or every problem that stops it from being one.
    fn load(&self) -> impl Future<Output = Result<Plan, Vec<String>>>;
}

/// What the instance was started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The canonical path of the configuration file, the only one a resume may name.
    pub config_path: String,
    pub profile: Profile,
}

/// How the instance ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended {
    /// Whether every removal of containers in the instance's life succeeded: at a
    /// stop, a restart, or a failure of the runtime core.
    pub containers_removed: bool,
    /// Whether the instance ended because no pool could start at its first run.
    pub nothing_to_run: bool,
    /// Whether the instance ended because its image could not run jobs at its first
    /// run.
    pub image_refused: bool,
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
    /// The image cannot run jobs, as the message says, at the first run of the core.
    ImageRefusedAtStart(String),
    /// The same at a later run.
    ImageRefused(String),
}

/// What reading and checking the configuration again at a resume found.
enum Reloaded {
    Run(Plan),
    Invalid(Vec<String>),
    ImageRefused(String),
}

/// Polls the core and turns a panic into an error carrying the panic's message, so
/// a panicking core is a failure rather than the end of the process.
struct CatchPanic<F>(Pin<Box<F>>);

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

pub struct Instance<'a, B, K, C, S> {
    settings: Settings,
    backend: &'a B,
    clock: &'a K,
    core: &'a C,
    source: &'a S,
    log: &'a dyn Log,
    times: StopTimes,
    state: RefCell<State>,
    policy: RefCell<RestartPolicy>,
    exit_requested: Cell<bool>,
    containers_removed: Cell<bool>,
    core_has_run: Cell<bool>,
    nothing_to_run: Cell<bool>,
    image_refused: Cell<bool>,
    force: watch::Sender<bool>,
}

impl<'a, B: Backend, K: Clock, C: Core, S: Source> Instance<'a, B, K, C, S> {
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
            log,
            times: StopTimes::default(),
            state: RefCell::new(State::Running),
            policy: RefCell::new(RestartPolicy::default()),
            exit_requested: Cell::new(false),
            containers_removed: Cell::new(true),
            core_has_run: Cell::new(false),
            nothing_to_run: Cell::new(false),
            image_refused: Cell::new(false),
            force: watch::Sender::new(false),
        }
    }

    /// The instance with other stop times, so a test can reach the exit deadline
    /// quickly; a debug build only.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    #[must_use]
    pub fn with_stop_times(mut self, times: StopTimes) -> Self {
        self.times = times;
        self
    }

    /// Skips the grace period: a `docker stop` under way, or one still to come, is
    /// abandoned and what remains is force-removed at once. A second Ctrl+C or
    /// SIGINT asks for it.
    pub fn force_removal(&self) {
        self.force.send_replace(true);
    }

    /// Runs the instance with its first plan until a stop is asked for. It answers
    /// every request that reaches `requests`. When the channel closes while the
    /// instance is stopped, nothing can resume it, so it ends.
    pub async fn run(&self, plan: Plan, requests: &mut Requests) -> Ended {
        let mut next = Next::Run(plan);
        loop {
            next = match next {
                Next::Run(plan) => self.running(&plan, requests).await,
                Next::Idle => self.stopped(requests).await,
                Next::Exit => {
                    return Ended {
                        containers_removed: self.containers_removed.get(),
                        nothing_to_run: self.nothing_to_run.get(),
                        image_refused: self.image_refused.get(),
                    };
                }
            };
        }
    }

    fn set_state(&self, state: State) {
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
        for (name, value) in self.core.report() {
            response = response.with(name, &value);
        }
        response
    }

    /// The stop reason for an image that cannot run jobs: `build` is refused while
    /// the instance exists, so the instance is stopped first.
    fn image_reason(&self, refusal: &str) -> StopReason {
        let config = &self.settings.config_path;
        StopReason::Image(format!(
            "{refusal}; run `runners stop`, then `runners build --config {config}`, \
             then `runners start --config {config}`"
        ))
    }

    fn refuse_busy(&self) -> Response {
        let name = self.state.borrow().name();
        Response::refused(&format!("the instance is {name}")).with("state", name)
    }

    async fn running(&self, plan: &Plan, requests: &mut Requests) -> Next {
        self.set_state(State::Running);
        let (stop, stop_receiver) = watch::channel(false);
        let mut cycle = Box::pin(self.core_cycle(plan, stop_receiver));
        let mut open = true;
        loop {
            tokio::select! {
                end = &mut cycle => {
                    return match end {
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
                            self.nothing_to_run.set(true);
                            Next::Exit
                        }
                        CycleEnd::ImageRefusedAtStart(refusal) => {
                            let config = &self.settings.config_path;
                            self.log.warn(&format!(
                                "The instance ends: {refusal}. Run `runners build --config {config}`, \
                                 then `runners start --config {config}`."
                            ));
                            self.image_refused.set(true);
                            Next::Exit
                        }
                        CycleEnd::ImageRefused(refusal) => {
                            let reason = self.image_reason(&refusal);
                            self.log.warn(&format!("The instance is stopped: {reason}."));
                            self.set_state(State::Stopped(reason));
                            Next::Idle
                        }
                    };
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
                        Request::Stop => {
                            let _ = incoming.reply.send(Response::ok("stopping"));
                            self.wind_down(&stop, cycle, requests).await;
                            return Next::Exit;
                        }
                        Request::Restart => {
                            self.wind_down(&stop, cycle, requests).await;
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

    /// Asks the core to stop and runs the container steps while it winds down, all
    /// within the exit deadline; whatever still runs then is dropped with `cycle`,
    /// which kills its clients. The containers are read before the core's workers
    /// go, because dropping a worker forgets its container.
    async fn wind_down<F: Future<Output = CycleEnd>>(
        &self,
        stop: &watch::Sender<bool>,
        cycle: Pin<Box<F>>,
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
            let both = async {
                tokio::join!(steps, cycle);
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

    /// Reads and validates the instance's own configuration again, clearing the
    /// core's failure count, checks its image, and answers `reply` with the result.
    async fn resume(&self, requests: &mut Requests, reply: oneshot::Sender<Response>) -> Next {
        self.policy.borrow_mut().clear();
        self.set_state(State::Resuming);
        self.log.info("Resuming: reading the configuration again.");
        let reload = async {
            let plan = match self.source.load().await {
                Ok(plan) => plan,
                Err(problems) => return Reloaded::Invalid(problems),
            };
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
            Some(Reloaded::ImageRefused(refusal)) => self.stay_stopped(self.image_reason(&refusal)),
        };
        let _ = reply.send(response);
        next
    }

    /// Leaves the instance stopped for `reason`, with the answer a resume gets.
    fn stay_stopped(&self, reason: StopReason) -> (Next, Response) {
        self.log
            .warn(&format!("The instance is stopped: {reason}."));
        let response = Response::refused(&format!("the instance stays stopped: {reason}"))
            .with("state", "stopped");
        self.set_state(State::Stopped(reason));
        (Next::Idle, response)
    }

    /// Runs `work` while answering requests. With `abortable`, a `stop` ends the wait
    /// and returns `None`; without it, the work finishes and the stop is left in
    /// `exit_requested` for the caller.
    async fn serve<T>(
        &self,
        requests: &mut Requests,
        abortable: bool,
        work: impl Future<Output = T>,
    ) -> Option<T> {
        let mut work = pin!(work);
        let mut open = true;
        loop {
            tokio::select! {
                output = &mut work => return Some(output),
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

    /// Answers a request that arrives during a transition.
    fn answer_busy(&self, incoming: Incoming) {
        let response = match incoming.request {
            Request::Status => self.status(),
            Request::Stop => {
                self.exit_requested.set(true);
                Response::ok("stopping")
            }
            Request::Restart | Request::Resume(_) => self.refuse_busy(),
        };
        let _ = incoming.reply.send(response);
    }

    /// Runs the core, and runs it again after each failure until the policy gives up
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
                    nothing_to_run: false,
                    image_refused: false,
                },
                // the panic unwound through the workers, so their containers are
                // still recorded by the backend and nowhere else.
                Err(message) => CoreFailure {
                    message,
                    containers: self.backend.running_containers(),
                    nothing_to_run: false,
                    image_refused: false,
                },
            };
            let message = scrub_error_text(&failure.message, "");
            if failure.image_refused {
                self.container_steps(failure.containers).await;
                return if first_run {
                    CycleEnd::ImageRefusedAtStart(message)
                } else {
                    CycleEnd::ImageRefused(message)
                };
            }
            if failure.nothing_to_run && first_run {
                self.log.warn(&message);
                self.container_steps(failure.containers).await;
                return CycleEnd::NothingToRun;
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

    /// Stops the containers in one call, and removes whatever remains in another. A
    /// forced removal abandons the stop, or skips it. A failed removal is
    /// remembered until the instance ends.
    async fn container_steps(&self, containers: Vec<String>) {
        if containers.is_empty() {
            return;
        }
        let request = StopRequest {
            names: containers.clone(),
            grace: self.times.grace,
            budget: self.times.stop_budget,
        };
        let mut force = self.force.subscribe();
        tokio::select! {
            biased;
            _ = force.wait_for(|forced| *forced) => {
                self.log.warn("Forced removal: the grace period is skipped.");
            }
            stopped = self.backend.stop_containers(&request) => {
                if let Err(error) = stopped {
                    self.log
                        .warn(&format!("Stopping the containers failed: {error}"));
                }
            }
        }
        if let Err(error) = self
            .backend
            .remove_containers(&containers, self.times.remove_budget)
            .await
        {
            self.log
                .warn(&format!("Removing the containers failed: {error}"));
            self.containers_removed.set(false);
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
                exit_deadline: Duration::from_secs(28),
            }
        );
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
