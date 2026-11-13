//! the agent's runtime core: the pools' slots as `start` runs them, except that a
//! pool's token comes from the token store at its admission and at each of its
//! registrations, that a pool skipped for want of a token starts once one is set,
//! without a restart, and that an applied edit of the configuration stops and starts
//! only the slots it changes while the others run on.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::task::Poll;

use tokio::sync::{Mutex, Notify, mpsc, watch};
use tokio::time::timeout;

use crate::instance::lifecycle::{CoreFailure, Ending, StopTimes};
use crate::instance::runtime_core::{failure, verify_image};
use crate::instance::steps::{Steps, some_container_steps};
use crate::runtime::backend::{Backend, JobHost};
use crate::runtime::board::{Board, PoolState, SlotState};
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;
use crate::runtime::name::parse_stamp;
use crate::runtime::pool::{
    NoPoolStartable, PoolSpec, SlotSpec, admit_pool, pools_from_plan, prepare_admitted,
};
use crate::runtime::registration::Registrar;
use crate::runtime::worker::{Services, run_slot_with, wait_for_stop};
use crate::validate::Plan;

use super::config::{Change, Effect};
use super::tokens::StoreTokens;

type Worker<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// a slot, by its pool's name and its number.
type SlotKey = (String, u32);

/// why a slot that an edit stops is leaving, as its runner shows it.
pub const LEAVING: &str = "the edit of the configuration stops it";

/// an edit for the running core to apply: the new configuration, already written
/// and in use, and its effect on each pool.
pub struct ApplyOrder {
    pub plan: Plan,
    pub effects: Vec<Effect>,
}

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
    /// begins its preparation. The run's first preparation, waiting for Docker
    /// included, is dropped when a drain begins; a late pool already preparing
    /// finishes, and its slots register nothing.
    pub drain: &'a watch::Sender<bool>,
    /// the times of a stop, which an idle runner's stop during a drain uses.
    pub times: StopTimes,
    /// the container steps that stop the slots an edit removes.
    pub steps: Steps<'a, B>,
    /// set after each apply, once its stops have ended and its new slots have started:
    /// `None` when it applied, or why it failed.
    pub applied: &'a watch::Sender<Option<String>>,
}

/// a slot that runs, with its own stop and a flag set when its worker has ended.
struct SlotRun {
    stop: watch::Sender<bool>,
    ended: watch::Receiver<bool>,
}

/// what one run of the core keeps while it runs.
struct Running<'a> {
    workers: RefCell<Vec<Worker<'a>>>,
    slots: RefCell<BTreeMap<(String, u32), SlotRun>>,
    /// the pools skipped for want of a token, which start once one is set.
    waiting: RefCell<Vec<PoolSpec>>,
    /// held by an apply, and by the start of a pool a token let in, so neither sees
    /// the other's pools half changed.
    turn: Mutex<()>,
    /// the pools of the configuration in use, which a slot left by a stop that ran
    /// late starts from once its worker has ended.
    wanted: RefCell<Vec<PoolSpec>>,
    /// the slots whose stop did not end their worker in time, with the flag their
    /// worker sets as it ends.
    late: RefCell<Vec<(SlotKey, watch::Receiver<bool>)>>,
    late_added: Notify,
    /// the pools an apply restarts or adds while slots of theirs are still held, by
    /// a stop that ran late or for their evidence; each is prepared once none is.
    deferred: RefCell<Vec<PoolSpec>>,
}

/// why a pool an apply restarts waits.
pub const DEFERRED: &str =
    "the edit restarts it once its earlier runners have stopped and their evidence is kept";

/// what an apply does to one pool.
enum Step {
    Nothing,
    /// stops its slots, and forgets it.
    Remove,
    /// stops its slots, then admits and prepares it as a new pool.
    Restart,
    /// stops the slots numbered above this.
    StopAbove(u32),
    /// starts the slots it lacks.
    StartNew,
    /// replaces the settings it waits with.
    Rewait,
}

/// whether any slot of `pool` is still held.
fn holds_slots(running: &Running<'_>, pool: &str) -> bool {
    running.slots.borrow().keys().any(|(name, _)| name == pool)
}

/// whether `name` is a container of `pool`'s slot `slot`, as `<pool>-<slot>-<stamp>`.
fn is_slot_container(name: &str, pool: &str, slot: u32) -> bool {
    name.strip_prefix(&format!("{pool}-{slot}-"))
        .is_some_and(|stamp| parse_stamp(stamp).is_some())
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

    /// starts `slots` on workers of their own, each with its own stop, which starts
    /// set when the core is already stopping.
    fn start_slots(
        &self,
        slots: Vec<SlotSpec>,
        running: &Running<'a>,
        stop: &watch::Receiver<bool>,
    ) {
        let services = self.services();
        for slot in slots {
            let key = (slot.pool.clone(), slot.number);
            // a slot still held, by a worker a stop did not end in time or for its
            // evidence, keeps its place, so a second runner never takes its slot and host
            if running.slots.borrow().contains_key(&key) {
                self.log.warn(&format!(
                    "{} slot {} is not started: its previous runner has not ended yet.",
                    slot.pool, slot.number
                ));
                continue;
            }
            let (slot_stop, slot_stop_receiver) = watch::channel(*stop.borrow());
            let (ended, ended_receiver) = watch::channel(false);
            running.slots.borrow_mut().insert(
                key,
                SlotRun {
                    stop: slot_stop,
                    ended: ended_receiver,
                },
            );
            running.workers.borrow_mut().push(Box::pin(async move {
                run_slot_with(&slot, &services, slot_stop_receiver).await;
                ended.send_replace(true);
            }));
        }
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

    /// runs every slot until `stop` becomes true, applying each order of `orders` as it
    /// comes; a pool that waits for a token is started when `tokens.changes()` says one
    /// was set. The board is left as the run ended it, for the agent to read and clear.
    pub async fn run(
        &self,
        plan: &Plan,
        mut stop: watch::Receiver<bool>,
        orders: mpsc::UnboundedReceiver<ApplyOrder>,
    ) -> Result<(), CoreFailure> {
        self.board.clear();
        // taken before the pools are admitted, so a token set meanwhile is seen
        let changes = self.tokens.changes();
        let pools =
            pools_from_plan(plan).map_err(|error| failure(error.to_string(), Ending::Ordinary))?;
        let image = plan.image_name.as_deref().unwrap_or_default();
        self.board.expect_image(image);
        let mut drain = self.drain.subscribe();
        let wanted = pools.clone();
        let prepared = tokio::select! {
            prepared = self.prepare(pools, image) => prepared?,
            () = wait_for_stop(&mut stop) => return Ok(()),
            _ = drain.wait_for(|draining| *draining) => return Ok(()),
        };
        let (prepared, waiting) = prepared;
        self.admitted.send_replace(true);
        let running = Running {
            workers: RefCell::new(Vec::new()),
            slots: RefCell::new(BTreeMap::new()),
            waiting: RefCell::new(waiting),
            turn: Mutex::new(()),
            wanted: RefCell::new(wanted),
            late: RefCell::new(Vec::new()),
            late_added: Notify::new(),
            deferred: RefCell::new(Vec::new()),
        };
        for pool in &prepared {
            self.start_slots(pool.slot_specs(), &running, &stop);
        }
        let side = async {
            tokio::join!(
                self.start_waiting(changes, &running, &stop),
                self.apply_orders(orders, &running, &stop),
                self.forward_stop(&running, &stop),
                self.finish_late(&running, &stop),
            );
        };
        let mut side = pin!(side);
        let mut side_done = false;
        poll_fn(|context| {
            if !side_done && side.as_mut().poll(context).is_ready() {
                side_done = true;
            }
            let mut workers = running.workers.borrow_mut();
            workers.retain_mut(|worker| worker.as_mut().poll(context).is_pending());
            // the core ends with the stop, or with a drain once every slot has ended: an
            // edit that stops every slot for a moment, or a pool waiting for its token,
            // leaves it running
            if workers.is_empty() && (*stop.borrow() || *self.drain.borrow()) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        Ok(())
    }

    /// passes the core's stop to every slot.
    async fn forward_stop(&self, running: &Running<'a>, stop: &watch::Receiver<bool>) {
        let mut stop = stop.clone();
        wait_for_stop(&mut stop).await;
        for slot in running.slots.borrow().values() {
            slot.stop.send_replace(true);
        }
    }

    /// starts each pool that was skipped for want of a token once its repository has
    /// one, until the stop.
    async fn start_waiting(
        &self,
        mut changes: watch::Receiver<u64>,
        running: &Running<'a>,
        stop: &watch::Receiver<bool>,
    ) {
        let mut stopping = stop.clone();
        let mut drain = self.drain.subscribe();
        loop {
            // a pool that waits for a token is not started while a drain lasts
            if *drain.borrow_and_update() {
                let ended = tokio::select! {
                    () = wait_for_stop(&mut stopping) => return,
                    ended = drain.wait_for(|draining| !*draining) => ended.is_ok(),
                };
                if !ended {
                    return;
                }
            }
            tokio::select! {
                biased;
                () = wait_for_stop(&mut stopping) => return,
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
            let _turn = running.turn.lock().await;
            let waiting = running.waiting.take();
            let (ready, still): (Vec<_>, Vec<_>) =
                waiting.into_iter().partition(|pool| self.admit(pool));
            running.waiting.borrow_mut().extend(still);
            self.prepare_and_start(ready, running, stop).await;
        }
    }

    /// prepares `pools` and starts their slots; a pool that cannot be prepared is
    /// skipped with its reason and left.
    async fn prepare_and_start(
        &self,
        pools: Vec<PoolSpec>,
        running: &Running<'a>,
        stop: &watch::Receiver<bool>,
    ) {
        if pools.is_empty() {
            return;
        }
        if let Ok(prepared) = prepare_admitted(self.backend, pools, self.log, self.board).await {
            for pool in &prepared {
                self.start_slots(pool.slot_specs(), running, stop);
            }
        }
    }

    /// applies each order in turn until the stop.
    async fn apply_orders(
        &self,
        mut orders: mpsc::UnboundedReceiver<ApplyOrder>,
        running: &Running<'a>,
        stop: &watch::Receiver<bool>,
    ) {
        let mut stopping = stop.clone();
        loop {
            let order = tokio::select! {
                () = wait_for_stop(&mut stopping) => return,
                order = orders.recv() => match order {
                    Some(order) => order,
                    None => return,
                },
            };
            let outcome = tokio::select! {
                () = wait_for_stop(&mut stopping) => return,
                outcome = self.apply(&order.plan, &order.effects, running, stop) => outcome,
            };
            self.applied.send_replace(outcome.err());
        }
    }

    /// stops the slots the effects remove, then starts the ones they add: a restarted
    /// or new pool is admitted and prepared as at Start all, and a pool that gains
    /// runners starts its new slots alone. A pool that waits for its token takes its
    /// new settings and goes on waiting, and a pool with no slot running, because it
    /// was skipped, is admitted and prepared again.
    async fn apply(
        &self,
        plan: &Plan,
        effects: &[Effect],
        running: &Running<'a>,
        stop: &watch::Receiver<bool>,
    ) -> Result<(), String> {
        let pools = match pools_from_plan(plan) {
            Ok(pools) => pools,
            Err(error) => {
                self.log
                    .warn(&format!("The edit could not be applied: {error}."));
                return Err(error.to_string());
            }
        };
        let _turn = running.turn.lock().await;
        running.wanted.borrow_mut().clone_from(&pools);
        self.board
            .expect_image(plan.image_name.as_deref().unwrap_or_default());
        let spec = |name: &str| pools.iter().find(|pool| pool.name == name).cloned();
        let steps: Vec<(&str, Step)> = effects
            .iter()
            .map(|effect| (effect.pool.as_str(), self.step(effect, running)))
            .collect();

        let mut leaving = Vec::new();
        for (pool, step) in &steps {
            let keep = match step {
                Step::Remove | Step::Restart => 0,
                Step::StopAbove(keep) => *keep,
                Step::Nothing | Step::StartNew | Step::Rewait => continue,
            };
            // a slot already told to stop is held, and is not stopped again
            leaving.extend(
                running
                    .slots
                    .borrow()
                    .iter()
                    .filter(|((name, number), run)| {
                        name == pool && *number > keep && !*run.stop.borrow()
                    })
                    .map(|(key, _)| key.clone()),
            );
        }
        self.stop_slots(leaving, running).await;

        let mut fresh = Vec::new();
        for (pool, step) in steps {
            match step {
                Step::Remove => {
                    running
                        .waiting
                        .borrow_mut()
                        .retain(|held| held.name != pool);
                    running
                        .deferred
                        .borrow_mut()
                        .retain(|held| held.name != pool);
                    self.board.remove_pool(pool);
                }
                Step::Restart => {
                    running
                        .waiting
                        .borrow_mut()
                        .retain(|held| held.name != pool);
                    running
                        .deferred
                        .borrow_mut()
                        .retain(|held| held.name != pool);
                    let Some(new) = spec(pool) else { continue };
                    if holds_slots(running, pool) {
                        // preparing it now would clear what the held slots still hold
                        self.board
                            .set_pool(pool, PoolState::Skipped(DEFERRED.to_string()));
                        running.deferred.borrow_mut().push(new);
                    } else {
                        fresh.push(new);
                    }
                }
                Step::Rewait => {
                    if let Some(new) = spec(pool) {
                        for held in running.waiting.borrow_mut().iter_mut() {
                            if held.name == pool {
                                *held = new.clone();
                            }
                        }
                    }
                }
                Step::StartNew => {
                    if let Some(new) = spec(pool) {
                        let lacking = new
                            .slot_specs()
                            .into_iter()
                            .filter(|slot| {
                                !running
                                    .slots
                                    .borrow()
                                    .contains_key(&(slot.pool.clone(), slot.number))
                            })
                            .collect();
                        self.start_slots(lacking, running, stop);
                    }
                }
                Step::Nothing | Step::StopAbove(_) => {}
            }
        }
        let (admitted, waiting): (Vec<_>, Vec<_>) =
            fresh.into_iter().partition(|pool| self.admit(pool));
        running.waiting.borrow_mut().extend(waiting);
        self.prepare_and_start(admitted, running, stop).await;
        Ok(())
    }

    /// what the apply does to a pool, from its effect and whether it runs slots or
    /// waits for its token.
    fn step(&self, effect: &Effect, running: &Running<'a>) -> Step {
        let deferred = running
            .deferred
            .borrow()
            .iter()
            .any(|pool| pool.name == effect.pool);
        if deferred {
            return match effect.change {
                Change::Unchanged => Step::Nothing,
                Change::Removed => Step::Remove,
                _ => Step::Restart,
            };
        }
        let waits = running
            .waiting
            .borrow()
            .iter()
            .any(|pool| pool.name == effect.pool);
        let runs = running
            .slots
            .borrow()
            .keys()
            .any(|(pool, _)| *pool == effect.pool);
        match effect.change {
            Change::Unchanged => Step::Nothing,
            Change::Removed => Step::Remove,
            Change::Added | Change::Restarted => Step::Restart,
            Change::RunnersAdded | Change::RunnersRemoved if waits => Step::Rewait,
            Change::RunnersAdded | Change::RunnersRemoved if !runs => Step::Restart,
            Change::RunnersAdded => Step::StartNew,
            Change::RunnersRemoved => Step::StopAbove(effect.runners_after),
        }
    }

    /// stops `slots` for good: each is marked leaving and its own stop is set, its
    /// containers go through the stop sequence, its diagnostic containers are
    /// finalised, and its worker winds down; then a slot whose worker has ended has its
    /// host released, unless the host still holds evidence to finalise, and leaves the
    /// board. A slot whose worker has not ended in time is finished once it has.
    async fn stop_slots(&self, slots: Vec<(String, u32)>, running: &Running<'a>) {
        if slots.is_empty() {
            return;
        }
        let mut ended = Vec::new();
        for (pool, number) in &slots {
            self.board.set_slot(
                &format!("{pool} slot {number}"),
                SlotState::Leaving(LEAVING.to_string()),
            );
            if let Some(slot) = running.slots.borrow().get(&(pool.clone(), *number)) {
                slot.stop.send_replace(true);
                ended.push(slot.ended.clone());
            }
        }
        let leaving = |name: &str| {
            slots
                .iter()
                .any(|(pool, number)| is_slot_container(name, pool, *number))
        };
        let containers: Vec<String> = self
            .backend
            .running_containers()
            .into_iter()
            .filter(|name| leaving(name))
            .collect();
        let workers_ended = async {
            for mut ended in ended {
                let _ = ended.wait_for(|ended| *ended).await;
            }
        };
        let both = async {
            tokio::join!(
                some_container_steps(&self.steps, containers, leaving),
                workers_ended
            );
        };
        if timeout(self.steps.times.exit_deadline, both).await.is_err() {
            self.log.warn(&format!(
                "The slots the edit stops did not end within {} s.",
                self.steps.times.exit_deadline.as_secs()
            ));
        }
        for key in slots {
            let ended = running
                .slots
                .borrow()
                .get(&key)
                .map(|run| run.ended.clone());
            match ended {
                Some(ended) if !*ended.borrow() => {
                    running.late.borrow_mut().push((key, ended));
                    running.late_added.notify_one();
                }
                _ => {
                    self.finish_slot(&key, running).await;
                }
            }
        }
    }

    /// forgets a slot whose worker has ended and releases its host, unless the host
    /// still holds diagnostic evidence: then the slot stays taken, and listed as
    /// leaving, until the next stop sequence finalises it, so no new runner replaces
    /// that host first. True when the slot was forgotten.
    async fn finish_slot(&self, key: &(String, u32), running: &Running<'a>) -> bool {
        let (pool, number) = key;
        let host = JobHost {
            pool: pool.clone(),
            slot: *number,
        };
        let prepared = self.backend.prepared_hosts().contains(&host);
        let retained = self
            .backend
            .retained()
            .names()
            .iter()
            .any(|name| is_slot_container(name, pool, *number));
        if retained || (prepared && self.backend.host_awaits_finalisation(&host)) {
            self.log.warn(&format!(
                "{pool} slot {number} keeps its job host for its diagnostic evidence until \
                 the runners next stop."
            ));
            return false;
        }
        running.slots.borrow_mut().remove(key);
        self.board.remove_slot(&format!("{pool} slot {number}"));
        if prepared
            && let Err(error) = self
                .backend
                .release_job_host(&host, self.steps.times.release_budget)
                .await
        {
            self.log.warn(&format!(
                "Releasing the job host of {pool} slot {number} failed: {error}"
            ));
        }
        true
    }

    /// finishes each slot whose stop ran late once its worker ends, and starts it
    /// again when the configuration in use still has it, until the stop.
    async fn finish_late(&self, running: &Running<'a>, stop: &watch::Receiver<bool>) {
        let mut stopping = stop.clone();
        loop {
            let next = running.late.borrow_mut().pop();
            let Some((key, mut ended)) = next else {
                tokio::select! {
                    () = wait_for_stop(&mut stopping) => return,
                    () = running.late_added.notified() => continue,
                }
            };
            tokio::select! {
                () = wait_for_stop(&mut stopping) => return,
                _ = ended.wait_for(|ended| *ended) => {}
            }
            let _turn = running.turn.lock().await;
            if !self.finish_slot(&key, running).await {
                continue;
            }
            let deferred = running
                .deferred
                .borrow()
                .iter()
                .position(|pool| pool.name == key.0);
            if let Some(at) = deferred {
                if !holds_slots(running, &key.0) {
                    let pool = running.deferred.borrow_mut().remove(at);
                    if self.admit(&pool) {
                        self.prepare_and_start(vec![pool], running, stop).await;
                    } else {
                        running.waiting.borrow_mut().push(pool);
                    }
                }
                continue;
            }
            let waits = running
                .waiting
                .borrow()
                .iter()
                .any(|pool| pool.name == key.0);
            let slot = running
                .wanted
                .borrow()
                .iter()
                .find(|pool| pool.name == key.0)
                .and_then(|pool| {
                    pool.slot_specs()
                        .into_iter()
                        .find(|slot| slot.number == key.1)
                });
            if let Some(slot) = slot
                && !waits
            {
                self.start_slots(vec![slot], running, stop);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slots_containers_are_told_from_another_slots_and_pools() {
        assert!(is_slot_container("ci-1-20261112091204118", "ci", 1));
        assert!(!is_slot_container("ci-12-20261112091204118", "ci", 1));
        assert!(!is_slot_container("ci-1-2-20261112091204118", "ci", 1));
        assert!(is_slot_container("ci-1-2-20261112091204118", "ci-1", 2));
        assert!(!is_slot_container("ci-1-probe", "ci", 1));
    }
}
