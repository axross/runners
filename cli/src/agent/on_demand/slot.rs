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
        if !settle(&mut reservation, completed, slot, &services, &mut stopping).await {
            break;
        }
        if let Err(reason) = &result {
            services.log.warn(&format!("{}: {reason}", slot.label()));
            services
                .board
                .set_slot(&slot.label(), SlotState::BackingOff(reason.clone()));
            if sleep_or_stop(services.clock, &mut stopping, backoff.fail()).await {
                break;
            }
        } else {
            backoff.reset();
            services.board.remove_slot(&slot.label());
        }
    }
}

/// repeats a finished job's registration cleanup, with the slot's back-off, until it settles;
/// the capacity lease stays held meanwhile. A stop ends the retries, so the one attempt a
/// stop gets is the job's own, bounded by the stop deadline; false means the cleanup is open.
async fn settle<B, K: Clock, R, T: TokenSource>(
    reservation: &mut Reservation<'_, '_, T, K>,
    completed: bool,
    slot: &SlotSpec,
    services: &Services<'_, B, K, R>,
    stopping: &mut watch::Receiver<bool>,
) -> bool {
    let mut backoff = Backoff::default();
    let mut deadline = Instant::now() + services.stop_times.exit_deadline;
    loop {
        let Err(reason) = reservation.finish(completed, deadline).await else {
            return true;
        };
        services.log.warn(&format!("{}: {reason}", slot.label()));
        services.board.set_slot(
            &slot.label(),
            SlotState::BackingOff(format!("registration cleanup incomplete ({reason})")),
        );
        if sleep_or_stop(services.clock, stopping, backoff.fail()).await {
            return false;
        }
        deadline = Instant::now() + services.stop_times.exit_deadline;
    }
}
