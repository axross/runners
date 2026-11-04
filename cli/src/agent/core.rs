//! the agent's runtime core: the pools' slots as `start` runs them, except that a
//! pool's token comes from the token store at its admission and at each of its
//! registrations, and that a pool skipped for want of a token starts once one is
//! set, without a restart.

use std::cell::RefCell;
use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::task::Poll;

use tokio::sync::watch;

use crate::instance::lifecycle::{CoreFailure, Ending};
use crate::instance::runtime_core::{failure, verify_image};
use crate::runtime::backend::Backend;
use crate::runtime::board::{Board, PoolState};
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;
use crate::runtime::pool::{
    NoPoolStartable, PoolSpec, SlotSpec, pools_from_plan, prepare_admitted,
};
use crate::runtime::registration::Registrar;
use crate::runtime::worker::{Services, run_slot_with, wait_for_stop};
use crate::token_store::RepositoryKey;
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
        }
    }

    /// the pool's token is there, or the pool is skipped with the reason.
    fn admit(&self, pool: &PoolSpec) -> bool {
        match self
            .tokens
            .read(&RepositoryKey::of(&pool.owner, &pool.repository))
        {
            Ok(token) => {
                self.log.mask(&token);
                self.board.set_pool(&pool.name, PoolState::WaitingForDocker);
                true
            }
            Err(reason) => {
                let reason = format!("{}: {reason}.", pool.path);
                self.log.warn(&format!(
                    "{} is not started and gets no slots: {reason}",
                    pool.describe()
                ));
                self.board.set_pool(&pool.name, PoolState::Skipped(reason));
                false
            }
        }
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
        let prepared = tokio::select! {
            prepared = self.prepare(pools, image) => prepared?,
            () = wait_for_stop(&mut stop) => return Ok(()),
        };
        let (prepared, waiting) = prepared;
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
        while !waiting.is_empty() {
            tokio::select! {
                () = wait_for_stop(&mut stop) => return,
                changed = changes.changed() => if changed.is_err() { return },
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
