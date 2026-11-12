//! the agent's runtime core: the pools' slots as `start` runs them, except that a
//! pool's token comes from the token store at its admission and at each of its
//! registrations, and that a pool skipped for want of a token starts once one is
//! set, without a restart.

use std::cell::RefCell;
use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::task::Poll;

use tokio::sync::watch;

use crate::instance::lifecycle::{CoreFailure, Ending, StopTimes};
use crate::instance::runtime_core::{failure, verify_image};
use crate::runtime::backend::Backend;
use crate::runtime::board::Board;
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;
use crate::runtime::pool::{
    NoPoolStartable, PoolSpec, SlotSpec, admit_pool, pools_from_plan, prepare_admitted,
};
use crate::runtime::registration::Registrar;
use crate::runtime::worker::{Services, run_slot_with, wait_for_stop};
use crate::validate::Plan;

use super::tokens::StoreTokens;

type Worker<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// the pools of a plan on one backend, with their state on the agent's board.
pub struct AgentCore<'a, B, K, R> {
    pub backend: &'a B,
    pub clock: &'a K,
    pub registrar: &'a R,
    pub tokens: &'a StoreTokens,
    pub log: &'a dyn Log,
    pub board: &'a Board,
    /// set once the pools are admitted and prepared, or have been given up on.
    pub admitted: &'a watch::Sender<bool>,
    /// true while the agent drains for an update: nothing registers, and no pool
    /// begins its preparation; one already preparing finishes, and its slots register
    /// nothing.
    pub drain: &'a watch::Sender<bool>,
    /// the times of a stop, which an idle runner's stop during a drain uses.
    pub times: StopTimes,
}

impl<'a, B: Backend, K: Clock, R: Registrar> AgentCore<'a, B, K, R> {
    fn services(&self) -> Services<'a, B, K, R> {
        Services {
            backend: self.backend,
            clock: self.clock,
            registrar: self.registrar,
            tokens: self.tokens,
            log: self.log,
            board: self.board,
            drain: Some(self.drain),
            stop_times: self.times,
        }
    }

    /// the pool's token is there, or the pool is skipped with the reason.
    fn admit(&self, pool: &PoolSpec) -> bool {
        admit_pool(
            pool,
            self.backend.engine_name(),
            self.tokens,
            self.log,
            self.board,
        )
    }

    fn workers(&self, pool: &PoolSpec, stop: &watch::Receiver<bool>) -> Vec<Worker<'a>> {
        let services = self.services();
        pool.slot_specs()
            .into_iter()
            .map(|slot: SlotSpec| -> Worker<'a> {
                let stop = stop.clone();
                Box::pin(async move { run_slot_with(&slot, &services, stop).await })
            })
            .collect()
    }

    /// admits the pools, waits for Docker, checks the image, and prepares the pools,
    /// in that order; the pools skipped for want of a token are returned too.
    async fn prepare(
        &self,
        pools: Vec<PoolSpec>,
        image: &str,
    ) -> Result<(Vec<PoolSpec>, Vec<PoolSpec>), CoreFailure> {
        let (admitted, waiting): (Vec<_>, Vec<_>) =
            pools.into_iter().partition(|pool| self.admit(pool));
        if admitted.is_empty() {
            return Err(failure(NoPoolStartable.to_string(), Ending::NothingToRun));
        }
        self.backend.wait_until_available().await;
        verify_image(self.backend, self.clock, self.log, self.board, image).await?;
        let prepared = prepare_admitted(self.backend, admitted, self.log, self.board)
            .await
            .map_err(|NoPoolStartable| {
                failure(NoPoolStartable.to_string(), Ending::NothingToRun)
            })?;
        Ok((prepared, waiting))
    }

    /// runs every slot until `stop` becomes true; a pool that waits for a token is
    /// started when `tokens.changes()` says one was set. The board is left as the
    /// run ended it, for the agent to read and clear.
    pub async fn run(
        &self,
        plan: &Plan,
        mut stop: watch::Receiver<bool>,
    ) -> Result<(), CoreFailure> {
        self.board.clear();
        // taken before the pools are admitted, so a token set meanwhile is seen
        let changes = self.tokens.changes();
        let pools =
            pools_from_plan(plan).map_err(|error| failure(error.to_string(), Ending::Ordinary))?;
        let image = plan.image_name.as_deref().unwrap_or_default();
        self.board.expect_image(image);
        let mut drain = self.drain.subscribe();
        let prepared = tokio::select! {
            prepared = self.prepare(pools, image) => prepared?,
            () = wait_for_stop(&mut stop) => return Ok(()),
            _ = drain.wait_for(|draining| *draining) => return Ok(()),
        };
        let (prepared, waiting) = prepared;
        self.admitted.send_replace(true);
        let workers = RefCell::new(
            prepared
                .iter()
                .flat_map(|pool| self.workers(pool, &stop))
                .collect::<Vec<_>>(),
        );
        let late = self.start_waiting(waiting, changes, &workers, &stop);
        let mut late = pin!(late);
        let mut late_done = false;
        poll_fn(|context| {
            if !late_done && late.as_mut().poll(context).is_ready() {
                late_done = true;
            }
            let mut workers = workers.borrow_mut();
            workers.retain_mut(|worker| worker.as_mut().poll(context).is_pending());
            if workers.is_empty() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        Ok(())
    }

    /// starts each pool that was skipped for want of a token once its repository has
    /// one, until the stop.
    async fn start_waiting(
        &self,
        mut waiting: Vec<PoolSpec>,
        mut changes: watch::Receiver<u64>,
        workers: &RefCell<Vec<Worker<'a>>>,
        stop: &watch::Receiver<bool>,
    ) {
        let mut stop = stop.clone();
        let mut drain = self.drain.subscribe();
        while !waiting.is_empty() {
            // a pool that waits for a token is not started while a drain lasts
            if *drain.borrow_and_update() {
                let ended = tokio::select! {
                    () = wait_for_stop(&mut stop) => return,
                    ended = drain.wait_for(|draining| !*draining) => ended.is_ok(),
                };
                if !ended {
                    return;
                }
            }
            tokio::select! {
                biased;
                () = wait_for_stop(&mut stop) => return,
                changed = drain.changed() => {
                    if changed.is_err() { return }
                    continue;
                }
                changed = changes.changed() => if changed.is_err() { return },
            }
            // a drain that began with the token's change is seen before a preparation;
            // the change is kept, so the pool is admitted once the drain is cancelled
            if *drain.borrow() {
                changes.mark_changed();
                continue;
            }
            let (ready, still): (Vec<_>, Vec<_>) =
                waiting.into_iter().partition(|pool| self.admit(pool));
            waiting = still;
            if ready.is_empty() {
                continue;
            }
            // a pool that cannot be prepared is skipped with its reason and left
            if let Ok(prepared) = prepare_admitted(self.backend, ready, self.log, self.board).await
            {
                for pool in &prepared {
                    workers.borrow_mut().extend(self.workers(pool, &stop));
                }
            }
        }
    }
}
