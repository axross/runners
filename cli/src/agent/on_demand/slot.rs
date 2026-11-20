//! the loop of one on-demand slot: wait for demand, run one admitted job, and settle the
//! registration that job owned before the slot looks for the next one.

use tokio::sync::watch;
use tokio::time::Instant;

use crate::runtime::backend::Backend;
use crate::runtime::board::SlotState;
use crate::runtime::clock::Clock;
use crate::runtime::pool::SlotSpec;
use crate::runtime::token::TokenSource;
use crate::runtime::worker::{Backoff, Services, run_one_job_with, sleep_or_stop, wait_for_stop};

use super::{Coordinator, Reservation};

/// runs `slot` until its stop, one job at a time, for as long as `listener` has demand for it.
pub(crate) async fn run_on_demand_slot<B: Backend, K: Clock, R, T: TokenSource>(
    slot: &SlotSpec,
    listener: &Coordinator<'_, T, K>,
    services: Services<'_, B, K, R>,
    slot_stop: watch::Receiver<bool>,
) {
    let mut backoff = Backoff::default();
    let mut stopping = slot_stop.clone();
    let mut draining = services.drain.map(watch::Sender::subscribe);
    loop {
        if let Some(drain) = &mut draining
            && *drain.borrow()
        {
            services.board.set_slot(&slot.label(), SlotState::Drained);
            tokio::select! {
                () = wait_for_stop(&mut stopping) => break,
                result = drain.wait_for(|draining| !*draining) => if result.is_err() { break; },
            }
        }
        let reservation = tokio::select! {
            reservation = listener.reserve(slot.number, slot_stop.clone()) => reservation,
            _ = async {
                match &mut draining {
                    Some(drain) => { let _ = drain.wait_for(|draining| *draining).await; },
                    None => std::future::pending::<()>().await,
                }
            } => continue,
        };
        let Some(mut reservation) = reservation else {
            break;
        };
        let admitted = Services {
            registrar: &reservation,
            backend: services.backend,
            clock: services.clock,
            tokens: services.tokens,
            log: services.log,
            board: services.board,
            drain: services.drain,
            stop_times: services.stop_times,
        };
        let result = run_one_job_with(slot, &admitted, slot_stop.clone()).await;
        let completed = result.as_ref().copied().unwrap_or(false);
        // the job's own failure is reported first, so a stop that ends the settling cannot hide it
        if let Err(reason) = &result {
            services.log.warn(&format!("{}: {reason}", slot.label()));
            services
                .board
                .set_slot(&slot.label(), SlotState::BackingOff(reason.clone()));
        }
        match settle(
            &mut reservation,
            completed,
            slot,
            &services,
            &mut stopping,
            &mut draining,
        )
        .await
        {
            Cleanup::Settled => {}
            Cleanup::Stopped => break,
            Cleanup::Abandoned(reason) => {
                park(slot, &services, &mut stopping, &mut draining, &reason).await;
                break;
            }
        }
        if result.is_err() {
            if sleep_or_stop(services.clock, &mut stopping, backoff.fail()).await {
                break;
            }
        } else {
            backoff.reset();
            services.board.remove_slot(&slot.label());
        }
    }
}

/// how a finished job's registration cleanup ended for its slot.
enum Cleanup {
    Settled,
    /// a stop ended it with the cleanup open.
    Stopped,
    /// a retry cannot succeed, for this reason; the cleanup stays open and the seat stays held.
    Abandoned(String),
}

fn is_draining(draining: &Option<watch::Receiver<bool>>) -> bool {
    draining.as_ref().is_some_and(|drain| *drain.borrow())
}

/// returns when the drain starts, or never when there is none.
async fn drain_begins(draining: &mut Option<watch::Receiver<bool>>) {
    match draining {
        Some(drain) => {
            if drain.wait_for(|draining| *draining).await.is_err() {
                std::future::pending::<()>().await;
            }
        }
        None => std::future::pending::<()>().await,
    }
}

/// repeats a finished job's registration cleanup, with the slot's back-off, until it settles;
/// the capacity lease stays held meanwhile. A stop during the wait gets one more attempt, within
/// the stop deadline. A drain gets at most one more, then the slot counts as drained with the
/// cleanup still reported open, and the retries resume when the drain ends. An error that a retry
/// cannot change abandons the cleanup, which Start all retries.
async fn settle<B, K: Clock, R, T: TokenSource>(
    reservation: &mut Reservation<'_, '_, T, K>,
    completed: bool,
    slot: &SlotSpec,
    services: &Services<'_, B, K, R>,
    stopping: &mut watch::Receiver<bool>,
    draining: &mut Option<watch::Receiver<bool>>,
) -> Cleanup {
    let mut backoff = Backoff::default();
    loop {
        let (stopped, drained) = (*stopping.borrow(), is_draining(draining));
        let deadline = Instant::now() + services.stop_times.exit_deadline;
        let Err(failure) = reservation.finish(completed, deadline).await else {
            return Cleanup::Settled;
        };
        services
            .log
            .warn(&format!("{}: {}", slot.label(), failure.reason));
        if failure.permanent {
            return Cleanup::Abandoned(failure.reason);
        }
        services.board.set_slot(
            &slot.label(),
            SlotState::BackingOff(format!(
                "registration cleanup incomplete ({})",
                failure.reason
            )),
        );
        if stopped {
            return Cleanup::Stopped;
        }
        if drained {
            services.board.set_slot(&slot.label(), SlotState::Drained);
            let Some(drain) = draining else {
                return Cleanup::Stopped;
            };
            tokio::select! {
                () = wait_for_stop(stopping) => {},
                result = drain.wait_for(|draining| !*draining) => if result.is_err() {
                    return Cleanup::Stopped;
                },
            }
            backoff.reset();
            continue;
        }
        tokio::select! {
            () = services.clock.sleep(backoff.fail()) => {},
            () = wait_for_stop(stopping) => {},
            () = drain_begins(draining) => {},
        }
    }
}

/// holds a slot whose cleanup is abandoned until the stop, counting it as drained meanwhile.
async fn park<B, K: Clock, R>(
    slot: &SlotSpec,
    services: &Services<'_, B, K, R>,
    stopping: &mut watch::Receiver<bool>,
    draining: &mut Option<watch::Receiver<bool>>,
    reason: &str,
) {
    loop {
        let state = if is_draining(draining) {
            SlotState::Drained
        } else {
            SlotState::Held(format!(
                "registration cleanup incomplete, retried at Start all ({reason})"
            ))
        };
        services.board.set_slot(&slot.label(), state);
        tokio::select! {
            () = wait_for_stop(stopping) => return,
            () = async {
                match draining {
                    Some(drain) => {
                        if drain.changed().await.is_err() {
                            std::future::pending::<()>().await;
                        }
                    }
                    None => std::future::pending::<()>().await,
                }
            } => {},
        }
    }
}
