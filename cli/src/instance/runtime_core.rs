//! The runtime core `start` runs: it prepares the pools and runs every slot's loop
//! until the stop is asked for.

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

use tokio::sync::watch;

use crate::runtime::backend::Backend;
use crate::runtime::board::Board;
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;
use crate::runtime::pool::{NoPoolStartable, pools_from_plan, prepare_pools};
use crate::runtime::registration::Registrar;
use crate::runtime::worker::{run_slot, wait_for_stop};
use crate::validate::Plan;

use super::lifecycle::{Core, CoreFailure};

/// The pools of the plan on one backend, with their state on a board of their own.
pub struct RuntimeCore<'a, B, K, R> {
    backend: &'a B,
    clock: &'a K,
    registrar: &'a R,
    log: &'a dyn Log,
    board: Board,
}

impl<'a, B: Backend, K: Clock, R: Registrar> RuntimeCore<'a, B, K, R> {
    #[must_use]
    pub fn new(backend: &'a B, clock: &'a K, registrar: &'a R, log: &'a dyn Log) -> Self {
        Self {
            backend,
            clock,
            registrar,
            log,
            board: Board::default(),
        }
    }
}

/// Empties the board when a run of the core ends, however it ends.
struct Cleared<'a>(&'a Board);

impl Drop for Cleared<'_> {
    fn drop(&mut self) {
        self.0.clear();
    }
}

fn failure(message: String, nothing_to_run: bool) -> CoreFailure {
    CoreFailure {
        message,
        containers: Vec::new(),
        nothing_to_run,
    }
}

/// Runs every worker to its end, polling each that has not ended whenever one wakes.
async fn run_all<F: Future<Output = ()>>(mut workers: Vec<Pin<Box<F>>>) {
    std::future::poll_fn(|context| {
        workers.retain_mut(|worker| worker.as_mut().poll(context).is_pending());
        if workers.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

impl<B: Backend, K: Clock, R: Registrar> Core for RuntimeCore<'_, B, K, R> {
    async fn run(&self, plan: &Plan, mut stop: watch::Receiver<bool>) -> Result<(), CoreFailure> {
        self.board.clear();
        let _cleared = Cleared(&self.board);
        let pools = pools_from_plan(plan).map_err(|error| failure(error.to_string(), false))?;
        let prepared = tokio::select! {
            prepared = prepare_pools(self.backend, pools, self.log, &self.board) => prepared,
            () = wait_for_stop(&mut stop) => return Ok(()),
        };
        let prepared =
            prepared.map_err(|NoPoolStartable| failure(NoPoolStartable.to_string(), true))?;
        let slots: Vec<_> = prepared.iter().flat_map(|pool| pool.slot_specs()).collect();
        let workers = slots
            .iter()
            .map(|slot| {
                Box::pin(run_slot(
                    slot,
                    self.backend,
                    self.clock,
                    self.registrar,
                    self.log,
                    &self.board,
                    stop.clone(),
                ))
            })
            .collect();
        run_all(workers).await;
        Ok(())
    }

    fn report(&self) -> Vec<(&'static str, String)> {
        self.board.fields()
    }
}
