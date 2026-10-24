//! The instance: the state it is in, and what each request and each failure of the
//! runtime core does to it.
//!
//! The instance is running or stopped, with stopping and resuming between them.
//! One task owns the state and serves the requests of the control channel, so a
//! request that arrives during a transition is answered at once: `status` tells the
//! truth, `stop` is taken, and `restart` and `resume` are refused until the
//! transition ends. The pools themselves run in a [`Core`], which this module
//! restarts when it fails; what starts and stops a process is the caller's.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::future::Future;
use std::pin::{Pin, pin};
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

/// The grace `docker stop` gives each container before it kills it.
const STOP_GRACE: Duration = Duration::from_secs(15);
/// The stop call is ended at this budget, counted from its start.
const STOP_BUDGET: Duration = Duration::from_secs(20);
/// The removal of what remains is ended at this budget, so the container steps take
/// 26 s at the most.
const REMOVE_BUDGET: Duration = Duration::from_secs(6);

/// Why the instance is stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The configuration failed validation at a restart or a resume.
    InvalidConfiguration(String),
    /// The runtime core failed more often than the window allows.
    RestartLimit,
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
}

/// The part of `start` that runs the pools.
///
/// A real core prepares the pools and runs their slots until the stop is asked for.
/// A panic is a failure: whatever supervises the core turns it into a
/// [`CoreFailure`].
pub trait Core {
    /// Runs until `stop` becomes true, then returns `Ok`. Returning in any other
    /// way, with or without an error, is a failure of the core.
    fn run(
        &self,
        plan: &Plan,
        stop: watch::Receiver<bool>,
    ) -> impl Future<Output = Result<(), CoreFailure>>;
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
}

enum Next {
    Run(Plan),
    Idle,
    Exit,
}

enum CycleEnd {
    StopRequested,
    LimitReached,
}

/// What a stop out of the running state leads to.
enum After {
    Exit,
    Restart(oneshot::Sender<Response>),
}

type Requests = mpsc::UnboundedReceiver<Incoming>;

pub struct Instance<'a, B, K, C, S> {
    settings: Settings,
    backend: &'a B,
    clock: &'a K,
    core: &'a C,
    source: &'a S,
    log: &'a dyn Log,
    state: RefCell<State>,
    policy: RefCell<RestartPolicy>,
    exit_requested: Cell<bool>,
    containers_removed: Cell<bool>,
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
            state: RefCell::new(State::Running),
            policy: RefCell::new(RestartPolicy::default()),
            exit_requested: Cell::new(false),
            containers_removed: Cell::new(true),
        }
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
        response
            .with("config", &self.settings.config_path)
            .with("profile", self.settings.profile.name())
    }

    fn refuse_busy(&self) -> Response {
        let name = self.state.borrow().name();
        Response::refused(&format!("the instance is {name}")).with("state", name)
    }

    async fn running(&self, plan: &Plan, requests: &mut Requests) -> Next {
        self.set_state(State::Running);
        let (stop, stop_receiver) = watch::channel(false);
        let mut cycle = pin!(self.core_cycle(plan, stop_receiver));
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
                            return self
                                .stop_running(&stop, cycle, requests, After::Exit)
                                .await;
                        }
                        Request::Restart => {
                            return self
                                .stop_running(
                                    &stop,
                                    cycle,
                                    requests,
                                    After::Restart(incoming.reply),
                                )
                                .await;
                        }
                    }
                }
            }
        }
    }

    /// Asks the core to stop, runs the container steps while it winds down, and then
    /// either ends or resumes. The containers are read before the core's workers go,
    /// because dropping a worker forgets its container.
    async fn stop_running<F: Future<Output = CycleEnd>>(
        &self,
        stop: &watch::Sender<bool>,
        cycle: Pin<&mut F>,
        requests: &mut Requests,
        after: After,
    ) -> Next {
        let containers = self.backend.running_containers();
        let _ = stop.send(true);
        self.set_state(State::Stopping);
        let work = async {
            tokio::join!(self.container_steps(containers), cycle);
        };
        self.serve(requests, false, work).await;
        match after {
            After::Exit => Next::Exit,
            After::Restart(reply) => {
                if self.exit_requested.get() {
                    let _ = reply.send(self.refuse_busy());
                    Next::Exit
                } else {
                    self.resume(requests, Some(reply)).await
                }
            }
        }
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
                Request::Restart => return self.resume(requests, Some(incoming.reply)).await,
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
                    return self.resume(requests, Some(incoming.reply)).await;
                }
            }
        }
    }

    /// Reads and validates the instance's own configuration again, clearing the
    /// core's failure count, and answers `reply` with the result.
    async fn resume(
        &self,
        requests: &mut Requests,
        reply: Option<oneshot::Sender<Response>>,
    ) -> Next {
        self.policy.borrow_mut().clear();
        self.set_state(State::Resuming);
        self.log.info("Resuming: reading the configuration again.");
        let loaded = self.serve(requests, true, self.source.load()).await;
        let (next, response) = match loaded {
            None => (Next::Exit, Response::refused("the instance is stopping")),
            Some(Ok(plan)) => {
                self.set_state(State::Running);
                (
                    Next::Run(plan),
                    Response::ok("running").with("state", "running"),
                )
            }
            Some(Err(problems)) => {
                let reason =
                    StopReason::InvalidConfiguration(scrub_error_text(&problems.join("; "), ""));
                self.log
                    .warn(&format!("The instance is stopped: {reason}."));
                let response = Response::refused(&format!("the instance stays stopped: {reason}"))
                    .with("state", "stopped");
                self.set_state(State::Stopped(reason));
                (Next::Idle, response)
            }
        };
        if let Some(reply) = reply {
            let _ = reply.send(response);
        }
        next
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
            let result = self.core.run(plan, stop.clone()).await;
            let failed_at = self.clock.now();
            if *stop.borrow() {
                return CycleEnd::StopRequested;
            }
            let failure = match result {
                Err(failure) => failure,
                Ok(()) => CoreFailure {
                    message: "the runtime core ended without being asked to".to_string(),
                    containers: Vec::new(),
                },
            };
            self.log.warn(&format!(
                "The runtime core failed: {}",
                scrub_error_text(&failure.message, "")
            ));
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

    /// Stops the containers in one call, and removes whatever remains in another.
    /// A failed removal is remembered until the instance ends.
    async fn container_steps(&self, containers: Vec<String>) {
        if containers.is_empty() {
            return;
        }
        let request = StopRequest {
            names: containers.clone(),
            grace: STOP_GRACE,
            budget: STOP_BUDGET,
        };
        if let Err(error) = self.backend.stop_containers(&request).await {
            self.log
                .warn(&format!("Stopping the containers failed: {error}"));
        }
        if let Err(error) = self
            .backend
            .remove_containers(&containers, REMOVE_BUDGET)
            .await
        {
            self.log
                .warn(&format!("Removing the containers failed: {error}"));
            self.containers_removed.set(false);
        }
    }
}
