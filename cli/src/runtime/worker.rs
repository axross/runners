//! the loop of one slot: check the image, prepare a host, probe the CPUs, register,
//! run one job container, release the host, repeat.

use std::time::Duration;

use tokio::sync::watch;

use crate::image::check::ImageCheck;

use super::backend::{
    Backend, ContainerSpec, CpuRequest, HostPreparation, RELEASE_WITHIN, StopRequest,
};
use super::board::{Board, SlotState};
use super::clock::Clock;
use super::diagnostic::finalize_at_job_end;
use super::image::check_image;
use super::job_marks::Activity;
use super::log::{Log, Masked};
use super::name::job_container_name;
use super::pool::SlotSpec;
use super::registration::{JitRequest, Registrar};
use super::scrub::scrub_error_text;
use super::token::{TokenFiles, TokenSource};
use crate::instance::lifecycle::StopTimes;

/// the delay after a slot's first failure.
pub const INITIAL_BACKOFF: Duration = Duration::from_secs(5);

/// the longest delay between attempts.
pub const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// a slot's delay between failed attempts: 5 s, doubling to 300 s, back to 5 s
/// after a container exits 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            next: INITIAL_BACKOFF,
        }
    }
}

impl Backoff {
    /// the delay to wait now; the one after it doubles.
    pub fn fail(&mut self) -> Duration {
        let delay = self.next;
        self.next = (delay * 2).min(MAX_BACKOFF);
        delay
    }

    pub fn reset(&mut self) {
        self.next = INITIAL_BACKOFF;
    }

    #[must_use]
    pub fn current(&self) -> Duration {
        self.next
    }
}

/// a stop request for the workers; dropping the sender stops them too.
#[must_use]
pub fn stop_channel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}

pub(crate) fn stopped(stop: &watch::Receiver<bool>) -> bool {
    *stop.borrow()
}

pub(crate) async fn wait_for_stop(stop: &mut watch::Receiver<bool>) {
    while !stopped(stop) {
        if stop.changed().await.is_err() {
            return;
        }
    }
}

/// sleeps `delay` unless a stop arrives first; returns whether a stop was asked for.
pub(crate) async fn sleep_or_stop<K: Clock>(
    clock: &K,
    stop: &mut watch::Receiver<bool>,
    delay: Duration,
) -> bool {
    if stopped(stop) {
        return true;
    }
    tokio::select! {
        () = clock.sleep(delay) => stopped(stop),
        () = wait_for_stop(stop) => true,
    }
}

/// what ends a slot's loop before it registers again: a stop, or a drain, which lets a
/// running job container run to its end and holds the slot until the drain ends.
pub(crate) struct Halt {
    stop: watch::Receiver<bool>,
    drain: Option<watch::Receiver<bool>>,
}

impl Halt {
    fn stopped(&self) -> bool {
        stopped(&self.stop)
    }

    fn draining(&self) -> bool {
        self.drain.as_ref().is_some_and(|drain| *drain.borrow())
    }

    fn halted(&self) -> bool {
        self.stopped() || self.draining()
    }

    /// returns once a stop or a drain is asked for.
    async fn wait(&mut self) {
        let Self { stop, drain } = self;
        match drain {
            None => wait_for_stop(stop).await,
            Some(drain) => tokio::select! {
                () = wait_for_stop(stop) => {}
                _ = drain.wait_for(|draining| *draining) => {}
            },
        }
    }

    /// holds a drained slot until the drain ends, when it goes on, or a stop, when it
    /// returns true.
    async fn hold(&mut self) -> bool {
        let Self { stop, drain } = self;
        if let Some(drain) = drain {
            tokio::select! {
                () = wait_for_stop(stop) => {}
                _ = drain.wait_for(|draining| !*draining) => {}
            }
        }
        stopped(stop)
    }

    /// sleeps `delay` unless a stop or a drain arrives first; true when one did.
    async fn sleep<K: Clock>(&mut self, clock: &K, delay: Duration) -> bool {
        if self.halted() {
            return true;
        }
        tokio::select! {
            () = clock.sleep(delay) => self.halted(),
            () = self.wait() => true,
        }
    }

    /// returns once a drain is under way and the container `name` is idle, checking
    /// its output's marks every [`IDLE_CHECK_EVERY`].
    async fn idle_in_drain<B: Backend>(&self, backend: &B, name: &str) {
        let Some(drain) = &self.drain else {
            return std::future::pending().await;
        };
        let mut drain = drain.clone();
        loop {
            if drain.wait_for(|draining| *draining).await.is_err() {
                return std::future::pending().await;
            }
            if backend.job_activity(name) == Activity::Idle {
                return;
            }
            tokio::select! {
                () = tokio::time::sleep(IDLE_CHECK_EVERY) => {}
                _ = drain.wait_for(|draining| !*draining) => {}
            }
        }
    }
}

/// how often a drain looks again at a running container that is not idle yet.
const IDLE_CHECK_EVERY: Duration = Duration::from_millis(250);

struct Report<'a> {
    log: &'a dyn Log,
    board: &'a Board,
    label: String,
}

impl Report<'_> {
    fn state(&self, state: SlotState) {
        self.board.set_slot(&self.label, state);
    }
}

/// why a slot waits out a backoff.
enum Pause {
    /// an attempt failed, for this reason.
    Failed(String),
    /// a host prerequisite refuses the boot, for this reason.
    Held(String),
}

impl Pause {
    fn words(&self) -> String {
        match self {
            Self::Failed(reason) => reason.clone(),
            Self::Held(reason) => format!("held, nothing is started ({reason})"),
        }
    }

    /// the slot's state while it waits; the board's text reaches `status` and the
    /// agent's snapshot, so the reason is scrubbed and masked.
    fn state(&self, log: &dyn Log) -> SlotState {
        let shown = |reason: &str| log.masks().apply(&scrub_error_text(reason, ""));
        match self {
            Self::Failed(reason) => SlotState::BackingOff(shown(reason)),
            Self::Held(reason) => SlotState::Held(shown(reason)),
        }
    }
}

/// warns that a slot waits and for what, then waits out the backoff, ending early on
/// a stop or a drain; while the agent drains, the reason is logged at info level and
/// the slot neither waits nor lengthens its backoff.
async fn back_off<K: Clock>(
    report: &Report<'_>,
    clock: &K,
    halt: &mut Halt,
    backoff: &mut Backoff,
    pause: Pause,
) {
    if halt.draining() {
        report.log.info(&format!(
            "{}: {} - not retried while the agent drains.",
            report.label,
            pause.words()
        ));
        return;
    }
    report.log.warn(&format!(
        "{}: {} - retrying in {}s.",
        report.label,
        pause.words(),
        backoff.current().as_secs()
    ));
    report.state(pause.state(report.log));
    halt.sleep(clock, backoff.fail()).await;
}

/// how one attempt, from the host's preparation to its release, ended.
enum Attempt {
    /// the container exited 0, and the slot starts a replacement at once.
    Ended,
    /// the attempt failed, or the container exited otherwise, and the slot backs off.
    Failed(String),
    /// a host prerequisite refused the boot, and the slot waits out the backoff held.
    Held(String),
    /// a stop or a drain ended the attempt before a container started.
    Halted,
}

struct Context<'a, B, K, R> {
    backend: &'a B,
    clock: &'a K,
    registrar: &'a R,
    tokens: &'a dyn TokenSource,
    report: &'a Report<'a>,
    times: StopTimes,
}

impl<B: Backend, K: Clock, R: Registrar> Context<'_, B, K, R> {
    /// prepares the slot's host, runs one job on it, and releases the host on every
    /// path out of the preparation, whatever its outcome and a stop included, before
    /// the caller backs off.
    async fn attempt(&self, slot: &SlotSpec, halt: &mut Halt) -> Attempt {
        let request = slot.host_request();
        let prepared = tokio::select! {
            prepared = self.backend.prepare_job_host(&request) => Some(prepared),
            () = halt.wait() => None,
        };
        let attempt = match prepared {
            None => Attempt::Halted,
            Some(HostPreparation::Ready) => self.run_job(slot, halt).await,
            Some(HostPreparation::Failed(error)) => Attempt::Failed(format!(
                "the job host could not be prepared ({error}); no registration"
            )),
            Some(HostPreparation::Held(reason)) => Attempt::Held(reason),
        };
        if halt.stopped() && self.backend.host_awaits_finalisation(&request.host) {
            // the stop sequence exports the host's evidence first, then releases it
            return attempt;
        }
        let budget = if halt.stopped() {
            self.backend.release_budget()
        } else {
            RELEASE_WITHIN
        };
        if let Err(error) = self.backend.release_job_host(&request.host, budget).await {
            self.report.log.warn(&format!(
                "{}: releasing the job host failed ({error}).",
                self.report.label
            ));
        }
        attempt
    }

    /// probes the host's CPUs, registers, runs one job container to its end and
    /// finalises it.
    async fn run_job(&self, slot: &SlotSpec, halt: &mut Halt) -> Attempt {
        let log = self.report.log;
        let request = CpuRequest {
            probe_name: slot.probe_name(),
            image: slot.image.clone(),
            count: slot.cpu_count,
            offset: slot.cpu_offset,
        };
        let Ok(cpuset) = self.backend.select_cpuset(&request).await else {
            return Attempt::Failed(
                "CPU discovery or probe cleanup failed; no registration".to_string(),
            );
        };

        let name = job_container_name(&slot.pool, slot.number, self.clock.now());
        let registration = async {
            let token = self
                .tokens
                .token(&slot.owner, &slot.repository, &slot.token_path)?;
            log.mask(&token);
            self.registrar
                .request_jit(&JitRequest {
                    owner: &slot.owner,
                    repository: &slot.repository,
                    name: &name,
                    labels: &slot.labels,
                    token: &token,
                })
                .await
                .map_err(|error| error.to_string())
        };
        let registered = tokio::select! {
            result = registration => result,
            () = halt.wait() => return Attempt::Halted,
        };
        let jit = match registered {
            Ok(jit) => jit,
            Err(reason) => {
                return Attempt::Failed(format!("failed to request a JIT registration ({reason})"));
            }
        };

        let _masked = Masked::new(log, &jit);
        if halt.halted() {
            return Attempt::Halted;
        }
        log.info(&format!(
            "{}: starting container {name}.",
            self.report.label
        ));
        self.report.state(SlotState::Running(name.clone()));
        let spec = ContainerSpec {
            name,
            cpus: slot.cpus.clone(),
            cpuset,
            memory_gb: slot.memory_gb,
            mounts: slot.mounts.clone(),
            image: slot.image.clone(),
            run_command: self.backend.run_command().to_string(),
            diagnostic: slot.diagnostic.clone(),
        };
        let exit = {
            let mut run = std::pin::pin!(self.backend.run_container(&spec, &jit));
            tokio::select! {
                biased;
                exit = &mut run => exit,
                () = halt.idle_in_drain(self.backend, &spec.name) => {
                    let (_, exit) = tokio::join!(self.stop_idle(&spec.name), &mut run);
                    exit
                }
            }
        };
        if spec.diagnostic.is_some() && !halt.stopped() {
            finalize_at_job_end(self.backend, log, &spec.name, tokio::time::Instant::now()).await;
        }
        match exit {
            Ok(0) => Attempt::Ended,
            outcome => {
                let reason = match outcome {
                    Ok(code) => format!("container exited with code {code}"),
                    Err(error) => format!("container could not run ({error})"),
                };
                // the JIT configuration is masked only until this function ends, and a
                // failed run's error text could hold it
                Attempt::Failed(log.masks().apply(&reason))
            }
        }
    }
}

impl<B: Backend, K: Clock, R: Registrar> Context<'_, B, K, R> {
    /// stops an idle runner's container during a drain, with a stop's grace, and kills
    /// it when the stop fails, which keeps the container for a finalisation; its run
    /// then ends, and the slot finalises it as after any job.
    async fn stop_idle(&self, name: &str) {
        let times = self.times;
        self.report.log.info(&format!(
            "{}: the runner is idle during a drain for an update, so its container {name} is stopped.",
            self.report.label
        ));
        let request = StopRequest {
            names: vec![name.to_string()],
            grace: times.grace,
            budget: times.stop_budget,
        };
        let Err(error) = self.backend.stop_containers(&request).await else {
            return;
        };
        self.report.log.warn(&format!(
            "{}: stopping the idle container {name} failed ({error}), so it is killed.",
            self.report.label
        ));
        if let Err(error) = self
            .backend
            .kill_containers(&request.names, times.remove_budget)
            .await
        {
            self.report.log.warn(&format!(
                "{}: killing the idle container {name} failed ({error}); the drain waits for it as if busy.",
                self.report.label
            ));
        }
    }
}

/// runs one slot until a stop is asked for, leaving a container already running to
/// the stop sequence; a diagnostic container is finalised after its run unless a stop
/// is under way, and its export's gaps never change the backoff; the token it reads
/// and the JIT configuration it holds are masked in `log`. Each attempt checks the
/// image, prepares a host, probes the CPUs, registers, runs the container, finalises
/// it, and releases the host before the slot backs off.
pub async fn run_slot<B: Backend, K: Clock, R: Registrar>(
    slot: &SlotSpec,
    backend: &B,
    clock: &K,
    registrar: &R,
    log: &dyn Log,
    board: &Board,
    stop: watch::Receiver<bool>,
) {
    let services = Services {
        backend,
        clock,
        registrar,
        tokens: &TokenFiles,
        log,
        board,
        drain: None,
        stop_times: StopTimes::default(),
    };
    run_slot_with(slot, &services, stop).await;
}

/// what a slot's loop works with.
pub struct Services<'a, B, K, R> {
    pub backend: &'a B,
    pub clock: &'a K,
    pub registrar: &'a R,
    /// where each registration reads its token.
    pub tokens: &'a dyn TokenSource,
    pub log: &'a dyn Log,
    pub board: &'a Board,
    /// turns true while the agent drains for an update: the slot registers nothing,
    /// lets a busy job run to its end, stops an idle one, and waits until the drain ends.
    pub drain: Option<&'a watch::Sender<bool>>,
    /// the times of a stop, which an idle runner's stop during a drain uses.
    pub stop_times: StopTimes,
}

impl<B, K, R> Clone for Services<'_, B, K, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<B, K, R> Copy for Services<'_, B, K, R> {}

/// [`run_slot`] with its token source among its services.
pub async fn run_slot_with<B: Backend, K: Clock, R: Registrar>(
    slot: &SlotSpec,
    services: &Services<'_, B, K, R>,
    stop: watch::Receiver<bool>,
) {
    let Services {
        backend,
        clock,
        registrar,
        tokens,
        log,
        board,
        drain,
        stop_times,
    } = *services;
    let report = Report {
        log,
        board,
        label: slot.label(),
    };
    let context = Context {
        backend,
        clock,
        registrar,
        tokens,
        report: &report,
        times: stop_times,
    };
    let mut halt = Halt {
        stop,
        drain: drain.map(watch::Sender::subscribe),
    };
    let mut backoff = Backoff::default();
    loop {
        if halt.stopped() {
            return;
        }
        if halt.draining() {
            report.state(SlotState::Drained);
            if halt.hold().await {
                return;
            }
            continue;
        }
        report.state(SlotState::Registering);
        let checked = tokio::select! {
            checked = check_image(backend, board, &slot.image) => checked,
            () = halt.wait() => continue,
        };
        let refused = match &checked {
            ImageCheck::Unavailable(reason) => Some(format!(
                "the image check failed ({reason}); no registration"
            )),
            other => other
                .refusal(&slot.image)
                .map(|refusal| format!("{refusal}; no registration")),
        };
        if let Some(reason) = refused {
            back_off(
                &report,
                clock,
                &mut halt,
                &mut backoff,
                Pause::Failed(reason),
            )
            .await;
            continue;
        }

        let pause = match context.attempt(slot, &mut halt).await {
            Attempt::Halted => continue,
            Attempt::Ended if halt.draining() => continue,
            Attempt::Ended => {
                log.info(&format!(
                    "{}: container exited successfully (GitHub owns the job result) - starting a replacement.",
                    report.label
                ));
                backoff.reset();
                continue;
            }
            Attempt::Failed(reason) => Pause::Failed(reason),
            Attempt::Held(reason) => Pause::Held(reason),
        };
        back_off(&report, clock, &mut halt, &mut backoff, pause).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_doubles_from_five_to_three_hundred_seconds_and_resets() {
        let mut backoff = Backoff::default();
        let delays: Vec<u64> = (0..9).map(|_| backoff.fail().as_secs()).collect();
        assert_eq!(delays, [5, 10, 20, 40, 80, 160, 300, 300, 300]);
        backoff.reset();
        assert_eq!(backoff.current(), Duration::from_secs(5));
        assert_eq!(backoff.fail(), Duration::from_secs(5));
        assert_eq!(backoff.fail(), Duration::from_secs(10));
    }
}
