//! The loop of one slot: probe the CPUs, register, run one job container, repeat.

use std::time::Duration;

use tokio::sync::watch;

use super::backend::{Backend, ContainerSpec, CpuRequest};
use super::clock::Clock;
use super::docker_args::RUN_COMMAND;
use super::log::Log;
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

fn stopped(stop: &watch::Receiver<bool>) -> bool {
    *stop.borrow()
}

async fn wait_for_stop(stop: &mut watch::Receiver<bool>) {
    while !stopped(stop) {
        if stop.changed().await.is_err() {
            return;
        }
    }
}

/// Sleeps `delay` unless a stop arrives first. Returns whether a stop was asked for.
async fn sleep_or_stop<K: Clock>(
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

/// Runs one slot until a stop is asked for. A container already running is left to
/// the stop sequence, which ends it; the loop then returns.
///
/// Each attempt probes the CPUs, then registers, then runs the container. A failed
/// probe or registration, or a non-zero exit, waits out the backoff; an exit of 0
/// starts the replacement at once.
pub async fn run_slot<B: Backend, K: Clock, R: Registrar>(
    slot: &SlotSpec,
    backend: &B,
    clock: &K,
    registrar: &R,
    log: &dyn Log,
    mut stop: watch::Receiver<bool>,
) {
    let label = slot.label();
    let mut backoff = Backoff::default();
    while !stopped(&stop) {
        let request = CpuRequest {
            probe_name: slot.probe_name(),
            image: slot.image.clone(),
            count: slot.cpu_count,
            offset: slot.cpu_offset,
        };
        let Ok(cpuset) = backend.select_cpuset(&request).await else {
            log.warn(&format!(
                "{label}: CPU discovery or probe cleanup failed; no registration - retrying in {}s.",
                backoff.current().as_secs()
            ));
            if sleep_or_stop(clock, &mut stop, backoff.fail()).await {
                return;
            }
            continue;
        };

        let name = job_container_name(&slot.pool, slot.number, clock.now());
        let registration = async {
            let token = read_token(&slot.token_path).map_err(|error| error.to_string())?;
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
                log.warn(&format!(
                    "{label}: failed to request a JIT registration ({reason}) - retrying in {}s.",
                    backoff.current().as_secs()
                ));
                if sleep_or_stop(clock, &mut stop, backoff.fail()).await {
                    return;
                }
                continue;
            }
        };

        log.info(&format!("{label}: starting container {name}."));
        let spec = ContainerSpec {
            name,
            cpus: slot.cpus.clone(),
            cpuset,
            memory_gb: slot.memory_gb,
            mounts: slot.mounts.clone(),
            image: slot.image.clone(),
            run_command: RUN_COMMAND.to_string(),
        };
        let exit = backend.run_container(&spec, &jit).await;
        drop(jit);
        match exit {
            Ok(0) => {
                log.info(&format!(
                    "{label}: container exited successfully (GitHub owns the job result) - starting a replacement."
                ));
                backoff.reset();
            }
            outcome => {
                let reason = match outcome {
                    Ok(code) => format!("container exited with code {code}"),
                    Err(error) => format!("container could not run ({error})"),
                };
                log.warn(&format!(
                    "{label}: {reason} - retrying in {}s.",
                    backoff.current().as_secs()
                ));
                if sleep_or_stop(clock, &mut stop, backoff.fail()).await {
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
