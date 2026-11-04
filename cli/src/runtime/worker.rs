//! the loop of one slot: check the image, prepare a host, probe the CPUs, register,
//! run one job container, release the host, repeat.

use std::time::Duration;

use tokio::sync::watch;

use crate::image::check::ImageCheck;

use super::backend::{Backend, ContainerSpec, CpuRequest, HostPreparation, RELEASE_WITHIN};
use super::board::{Board, SlotState};
use super::clock::Clock;
use super::diagnostic::finalize_at_job_end;
use super::image::check_image;
use super::log::{Log, Masked};
use super::name::job_container_name;
use super::pool::SlotSpec;
use super::registration::{JitRequest, Registrar};
use super::token::read_token;

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

/// warns that an attempt failed and waits out the backoff, ending early on a stop;
/// returns whether a stop was asked for.
async fn back_off<K: Clock>(
    report: &Report<'_>,
    clock: &K,
    stop: &mut watch::Receiver<bool>,
    backoff: &mut Backoff,
    reason: &str,
    state: SlotState,
) -> bool {
    report.log.warn(&format!(
        "{}: {reason} - retrying in {}s.",
        report.label,
        backoff.current().as_secs()
    ));
    report.state(state);
    sleep_or_stop(clock, stop, backoff.fail()).await
}

/// how one attempt, from the host's preparation to its release, ended.
enum Attempt {
    /// the container exited 0, and the slot starts a replacement at once.
    Ended,
    /// the attempt failed, or the container exited otherwise, and the slot backs off.
    Failed(String),
    /// a host prerequisite refused the boot, and the slot waits out the backoff held.
    Held(String),
    Stopped,
}

struct Context<'a, B, K, R> {
    backend: &'a B,
    clock: &'a K,
    registrar: &'a R,
    report: &'a Report<'a>,
}

impl<B: Backend, K: Clock, R: Registrar> Context<'_, B, K, R> {
    /// prepares the slot's host, runs one job on it, and releases the host on every
    /// path out of the preparation, whatever its outcome and a stop included, before
    /// the caller backs off.
    async fn attempt(&self, slot: &SlotSpec, stop: &mut watch::Receiver<bool>) -> Attempt {
        let request = slot.host_request();
        let prepared = tokio::select! {
            prepared = self.backend.prepare_job_host(&request) => Some(prepared),
            () = wait_for_stop(stop) => None,
        };
        let attempt = match prepared {
            None => Attempt::Stopped,
            Some(HostPreparation::Ready) => self.run_job(slot, stop).await,
            Some(HostPreparation::Failed(error)) => Attempt::Failed(format!(
                "the job host could not be prepared ({error}); no registration"
            )),
            Some(HostPreparation::Held(reason)) => Attempt::Held(reason),
        };
        let budget = if stopped(stop) {
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
    async fn run_job(&self, slot: &SlotSpec, stop: &mut watch::Receiver<bool>) -> Attempt {
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
            let token = read_token(&slot.token_path).map_err(|error| error.to_string())?;
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
            () = wait_for_stop(stop) => return Attempt::Stopped,
        };
        let jit = match registered {
            Ok(jit) => jit,
            Err(reason) => {
                return Attempt::Failed(format!("failed to request a JIT registration ({reason})"));
            }
        };

        let _masked = Masked::new(log, &jit);
        if stopped(stop) {
            return Attempt::Stopped;
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
        let exit = self.backend.run_container(&spec, &jit).await;
        if spec.diagnostic.is_some() && !stopped(stop) {
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
    mut stop: watch::Receiver<bool>,
) {
    let report = Report {
        log,
        board,
        label: slot.label(),
    };
    let context = Context {
        backend,
        clock,
        registrar,
        report: &report,
    };
    let mut backoff = Backoff::default();
    while !stopped(&stop) {
        report.state(SlotState::Registering);
        let checked = tokio::select! {
            checked = check_image(backend, board, &slot.image) => checked,
            () = wait_for_stop(&mut stop) => return,
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
            let state = SlotState::BackingOff;
            if back_off(&report, clock, &mut stop, &mut backoff, &reason, state).await {
                return;
            }
            continue;
        }

        let (reason, state) = match context.attempt(slot, &mut stop).await {
            Attempt::Stopped => return,
            Attempt::Ended => {
                log.info(&format!(
                    "{}: container exited successfully (GitHub owns the job result) - starting a replacement.",
                    report.label
                ));
                backoff.reset();
                continue;
            }
            Attempt::Failed(reason) => (reason, SlotState::BackingOff),
            Attempt::Held(reason) => (
                format!("held, nothing is started ({reason})"),
                SlotState::Held(reason),
            ),
        };
        if back_off(&report, clock, &mut stop, &mut backoff, &reason, state).await {
            return;
        }
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
