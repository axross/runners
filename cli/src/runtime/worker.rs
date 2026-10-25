//! The loop of one slot: check the image, probe the CPUs, register, run one job
//! container, repeat.

use std::time::Duration;

use tokio::sync::watch;

use crate::image::check::ImageCheck;

use super::backend::{Backend, ContainerSpec, CpuRequest};
use super::board::{Board, SlotState};
use super::clock::Clock;
use super::docker_args::RUN_COMMAND;
use super::image::check_image;
use super::log::{Log, Masked};
use super::name::job_container_name;
use super::pool::SlotSpec;
use super::registration::{JitRequest, Registrar};
use super::token::read_token;

/// The delay after a slot's first failure.
pub const INITIAL_BACKOFF: Duration = Duration::from_secs(5);

/// The longest delay between attempts.
pub const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// A slot's delay between failed attempts: 5 s, doubling to 300 s, back to 5 s
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
    /// The delay to wait now; the one after it doubles.
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

/// A stop request for the workers. Dropping the sender stops them too.
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

/// Sleeps `delay` unless a stop arrives first. Returns whether a stop was asked for.
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

/// Where a slot reports: its log and the status board.
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

/// Warns that an attempt failed and waits out the backoff, ending early on a stop.
/// Returns whether a stop was asked for.
async fn back_off<K: Clock>(
    report: &Report<'_>,
    clock: &K,
    stop: &mut watch::Receiver<bool>,
    backoff: &mut Backoff,
    reason: &str,
) -> bool {
    report.log.warn(&format!(
        "{}: {reason} - retrying in {}s.",
        report.label,
        backoff.current().as_secs()
    ));
    report.state(SlotState::BackingOff);
    sleep_or_stop(clock, stop, backoff.fail()).await
}

/// Runs one slot until a stop is asked for. A container already running is left to
/// the stop sequence, which ends it; the loop then returns.
///
/// Each attempt checks the image, then probes the CPUs, then registers, then runs
/// the container. An image that cannot run jobs, a Docker that does not answer the
/// check, a failed probe or registration, or a non-zero exit, waits out the
/// backoff; an exit of 0 starts the replacement at once. The slot's state goes on
/// `board`, and the token it reads and the JIT configuration it holds are masked in
/// `log`.
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
            if back_off(&report, clock, &mut stop, &mut backoff, &reason).await {
                return;
            }
            continue;
        }

        let request = CpuRequest {
            probe_name: slot.probe_name(),
            image: slot.image.clone(),
            count: slot.cpu_count,
            offset: slot.cpu_offset,
        };
        let Ok(cpuset) = backend.select_cpuset(&request).await else {
            let reason = "CPU discovery or probe cleanup failed; no registration";
            if back_off(&report, clock, &mut stop, &mut backoff, reason).await {
                return;
            }
            continue;
        };

        let name = job_container_name(&slot.pool, slot.number, clock.now());
        let registration = async {
            let token = read_token(&slot.token_path).map_err(|error| error.to_string())?;
            log.mask(&token);
            registrar
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
            () = wait_for_stop(&mut stop) => return,
        };
        let jit = match registered {
            Ok(jit) => jit,
            Err(reason) => {
                let reason = format!("failed to request a JIT registration ({reason})");
                if back_off(&report, clock, &mut stop, &mut backoff, &reason).await {
                    return;
                }
                continue;
            }
        };

        let _masked = Masked::new(log, &jit);
        if stopped(&stop) {
            return;
        }
        log.info(&format!("{}: starting container {name}.", report.label));
        report.state(SlotState::Running(name.clone()));
        let spec = ContainerSpec {
            name,
            cpus: slot.cpus.clone(),
            cpuset,
            memory_gb: slot.memory_gb,
            mounts: slot.mounts.clone(),
            image: slot.image.clone(),
            run_command: RUN_COMMAND.to_string(),
        };
        // the JIT configuration stays masked until the outcome below is reported,
        // since a failed run's error text could hold it
        let exit = backend.run_container(&spec, &jit).await;
        match exit {
            Ok(0) => {
                log.info(&format!(
                    "{}: container exited successfully (GitHub owns the job result) - starting a replacement.",
                    report.label
                ));
                backoff.reset();
            }
            outcome => {
                let reason = match outcome {
                    Ok(code) => format!("container exited with code {code}"),
                    Err(error) => format!("container could not run ({error})"),
                };
                if back_off(&report, clock, &mut stop, &mut backoff, &reason).await {
                    return;
                }
            }
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
