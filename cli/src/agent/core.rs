//! the agent's runtime core: the pools' slots as `start` runs them, except that a
//! pool's token comes from the token store at its admission and at each of its
//! registrations, that a pool skipped for want of a token starts once one is set,
//! without a restart, and that an applied edit of the configuration stops and starts
//! only the slots it changes while the others run on.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::rc::Rc;
use std::task::Poll;

use tokio::sync::{Mutex, Notify, mpsc, watch};
use tokio::time::{Instant, timeout, timeout_at};

use crate::instance::lifecycle::{CoreFailure, Ending, StopTimes};
use crate::instance::runtime_core::{failure, verify_image};
use crate::instance::steps::{Steps, some_container_steps};
use crate::profile::Os;
use crate::runtime::backend::{Backend, JobHost};
use crate::runtime::board::{Board, PoolState, SlotState};
use crate::runtime::clock::Clock;
use crate::runtime::log::Log;
use crate::runtime::name::parse_stamp;
use crate::runtime::pool::{
    NoPoolStartable, PoolSpec, SlotSpec, admit_pool, image_name, pools_from_plan, prepare_admitted,
};
use crate::runtime::registration::Registrar;
use crate::runtime::worker::{Backoff, Services, run_slot_with, wait_for_stop};
use crate::validate::{Plan, PoolTarget};

use super::config::{Change, Effect};
use super::on_demand::{
    Controls, Coordinator, Failure as ListenerFailure, ListenerState, PendingSession, Pool,
    SessionRecovery, View as ListenerView, retry_delay, run_on_demand_slot,
};
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
    pub client: &'a Result<crate::scaleset::Client, crate::scaleset::Error>,
    pub listeners: &'a watch::Sender<BTreeMap<String, ListenerView>>,
    pub(crate) session_recovery: &'a RefCell<SessionRecovery>,
    pub host: &'a str,
}

/// a slot that runs, with its own stop and a flag set when its worker has ended.
struct SlotRun {
    stop: watch::Sender<bool>,
    ended: watch::Receiver<bool>,
    runner_prefix: String,
}

/// what one run of the core keeps while it runs.
struct Running<'a, K> {
    workers: RefCell<Vec<Worker<'a>>>,
    slots: RefCell<BTreeMap<(String, u32), SlotRun>>,
    listeners: RefCell<BTreeMap<String, ListenerRun<'a, K>>>,
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
    late_added: Rc<Notify>,
    /// the pools an apply restarts or adds while slots of theirs are still held, by
    /// a stop that ran late or for their evidence; each is prepared once none is.
    deferred: RefCell<Vec<PoolSpec>>,
}

struct ListenerRun<'a, K> {
    coordinator: Rc<Coordinator<'a, StoreTokens, K>>,
    stop: watch::Sender<bool>,
    capacity: watch::Sender<u32>,
    ended: watch::Receiver<bool>,
}

/// why a pool an apply restarts waits.
pub const DEFERRED: &str = "the edit restarts it once its earlier runners have stopped; one \
     that keeps diagnostic evidence holds it until the next Start all";

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
fn holds_slots<K>(running: &Running<'_, K>, pool: &str) -> bool {
    running.slots.borrow().keys().any(|(name, _)| name == pool)
        || running
            .listeners
            .borrow()
            .get(pool)
            .is_some_and(|listener| !*listener.ended.borrow())
}

/// whether `name` is a container of `pool`'s slot `slot`, as `<pool>-<slot>-<stamp>`.
fn is_slot_container(name: &str, pool: &str, slot: u32) -> bool {
    name.strip_prefix(&format!("{pool}-{slot}-"))
        .is_some_and(|stamp| parse_stamp(stamp).is_some())
}

/// whether both targets are repositories with the same owner and name, in any case.
fn same_repository(a: &PoolTarget, b: &PoolTarget) -> bool {
    match (a, b) {
        (
            PoolTarget::Repository {
                owner: a_owner,
                repository: a_repository,
            },
            PoolTarget::Repository {
                owner: b_owner,
                repository: b_repository,
            },
        ) => {
            a_owner.eq_ignore_ascii_case(b_owner) && a_repository.eq_ignore_ascii_case(b_repository)
        }
        _ => false,
    }
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
        if self.listeners.borrow().values().any(|listener| {
            listener.has_pending_cleanup()
                && (listener.logical_name == pool.name
                    || (same_repository(&listener.target(), &pool.target)
                        && pool
                            .scale_set
                            .as_ref()
                            .is_some_and(|name| listener.scale_set.eq_ignore_ascii_case(name))))
        }) {
            self.board.set_pool(
                &pool.name,
                PoolState::Skipped(
                    "on-demand cleanup is incomplete; Start all retries cleanup".to_string(),
                ),
            );
            return false;
        }
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
        running: &Running<'a, K>,
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
            let listener = if slot.scale_set.is_some() {
                let Some(listener) = self.listener(&slot, running, stop) else {
                    continue;
                };
                Some(listener)
            } else {
                None
            };
            let (slot_stop, slot_stop_receiver) = watch::channel(*stop.borrow());
            let (ended, ended_receiver) = watch::channel(false);
            running.slots.borrow_mut().insert(
                key,
                SlotRun {
                    stop: slot_stop,
                    ended: ended_receiver,
                    runner_prefix: slot.runner_prefix().to_string(),
                },
            );
            running.workers.borrow_mut().push(Box::pin(async move {
                if let Some(listener) = listener {
                    run_on_demand_slot(&slot, &listener, services, slot_stop_receiver).await;
                } else {
                    run_slot_with(&slot, &services, slot_stop_receiver).await;
                }
                ended.send_replace(true);
            }));
        }
    }

    /// one listener spans all slots and capacity-only edits of its logical pool.
    fn listener(
        &self,
        slot: &SlotSpec,
        running: &Running<'a, K>,
        stop: &watch::Receiver<bool>,
    ) -> Option<Rc<Coordinator<'a, StoreTokens, K>>> {
        if let Some(listener) = running.listeners.borrow().get(&slot.pool) {
            if *listener.ended.borrow() && listener.coordinator.view().has_pending_cleanup() {
                return None;
            }
            return Some(Rc::clone(&listener.coordinator));
        }
        let client = match self.client {
            Ok(client) => client,
            Err(error) => {
                self.board
                    .set_pool(&slot.pool, PoolState::Skipped(error.to_string()));
                return None;
            }
        };
        let PoolTarget::Repository { owner, repository } = &slot.target else {
            return None;
        };
        let slots = running
            .wanted
            .borrow()
            .iter()
            .find(|pool| pool.name == slot.pool)
            .map_or(slot.number, |pool| pool.slots);
        let (stopping, receiver) = watch::channel(*stop.borrow());
        let (capacity, capacity_receiver) = watch::channel(slots);
        let pending_session = Rc::new(RefCell::new(None));
        self.session_recovery
            .borrow_mut()
            .insert(slot.pool.clone(), Rc::clone(&pending_session));
        let coordinator = Rc::new(
            Coordinator::new(
                Pool {
                    owner: owner.clone(),
                    repository: repository.clone(),
                    logical_name: slot.pool.clone(),
                    scale_set: slot.scale_set.clone()?,
                    session_owner: self.host.to_string(),
                },
                client,
                self.tokens,
                self.clock,
                self.log,
                Controls {
                    stop: receiver,
                    drain: self.drain.subscribe(),
                    capacity: capacity_receiver,
                },
            )
            .with_session_recovery(pending_session),
        );
        let (ended, ended_receiver) = watch::channel(false);
        running.listeners.borrow_mut().insert(
            slot.pool.clone(),
            ListenerRun {
                coordinator: Rc::clone(&coordinator),
                stop: stopping,
                capacity,
                ended: ended_receiver,
            },
        );
        let listener = Rc::clone(&coordinator);
        let wake = Rc::clone(&running.late_added);
        running.workers.borrow_mut().push(Box::pin(async move {
            listener.run().await;
            ended.send_replace(true);
            wake.notify_one();
        }));
        Some(coordinator)
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
        if admitted.is_empty()
            && !self
                .listeners
                .borrow()
                .values()
                .any(ListenerView::has_pending_cleanup)
        {
            return Err(failure(NoPoolStartable.to_string(), Ending::NothingToRun));
        }
        self.backend.wait_until_available().await;
        verify_image(self.backend, self.clock, self.log, self.board, image).await?;
        if admitted.is_empty() {
            return Ok((Vec::new(), waiting));
        }
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
        self.listeners.send_modify(|views| {
            views.retain(|_, view| view.has_pending_cleanup());
        });
        self.backend.use_plan(plan);
        forget_macos_without_a_macos_pool(self.board, plan);
        // taken before the pools are admitted, so a token set meanwhile is seen
        let changes = self.tokens.changes();
        let pools =
            pools_from_plan(plan).map_err(|error| failure(error.to_string(), Ending::Ordinary))?;
        let image = image_name(plan);
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
            listeners: RefCell::new(BTreeMap::new()),
            waiting: RefCell::new(waiting),
            turn: Mutex::new(()),
            wanted: RefCell::new(wanted),
            late: RefCell::new(Vec::new()),
            late_added: Rc::new(Notify::new()),
            deferred: RefCell::new(Vec::new()),
        };
        for pool in &prepared {
            self.start_slots(pool.slot_specs(), &running, &stop);
        }
        let side = async {
            tokio::join!(
                self.recover_cleanup(&running, &stop),
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
            for listener in running.listeners.borrow().values() {
                let view = listener.coordinator.view();
                self.listeners.send_if_modified(|views| {
                    if views.get(&view.logical_name) == Some(&view) {
                        return false;
                    }
                    views.insert(view.logical_name.clone(), view);
                    true
                });
            }
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

    /// cleanup workers do not hold unrelated pools behind another pool's back-off.
    async fn recover_cleanup(&self, running: &Running<'a, K>, stop: &watch::Receiver<bool>) {
        let pending: Vec<_> = self
            .listeners
            .borrow()
            .values()
            .filter(|view| view.has_pending_cleanup())
            .cloned()
            .collect();
        let mut workers: Vec<Worker<'_>> = pending
            .into_iter()
            .map(|view| Box::pin(self.recover_listener(view, running, stop)) as Worker<'_>)
            .collect();
        poll_fn(|context| {
            workers.retain_mut(|worker| worker.as_mut().poll(context).is_pending());
            if workers.is_empty() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    async fn recover_listener(
        &self,
        mut view: ListenerView,
        running: &Running<'a, K>,
        stop: &watch::Receiver<bool>,
    ) {
        let Ok(client) = self.client else { return };
        let mut stopping = stop.clone();
        let deadline = Instant::now() + self.times.exit_deadline;
        let result = tokio::select! {
            biased;
            () = wait_for_stop(&mut stopping) => None,
            result = timeout_at(deadline, view.retry_registration_cleanup(client, self.tokens)) => Some(result),
        };
        if !matches!(result, Some(Ok(Ok(())))) {
            self.log.warn(&format!(
                "pool {}: incomplete registration cleanup remains in {}",
                view.logical_name,
                view.target()
            ));
        }
        self.publish_cleanup(&view);
        if result.is_none() {
            return;
        }
        let pending = self
            .session_recovery
            .borrow()
            .get(&view.logical_name)
            .cloned();
        let mut backoff = Backoff::default();
        while view.session_cleanup_incomplete {
            let Some(pending) = &pending else { return };
            let result = tokio::select! {
                biased;
                () = wait_for_stop(&mut stopping) => return,
                result = PendingSession::retry_within(
                    pending,
                    client,
                    self.tokens,
                    self.clock.now(),
                ) => result,
            };
            let delay = match result {
                Ok(()) => {
                    view.session_cleanup_incomplete = false;
                    view.state = ListenerState::Stopped;
                    view.status = None;
                    view.refused_host = None;
                    view.reason = None;
                    self.publish_cleanup(&view);
                    break;
                }
                Err(ListenerFailure::Api(error)) => {
                    let fatal = view.failed(&error, true);
                    self.publish_cleanup(&view);
                    self.log.warn(&format!(
                        "pool {}: session cleanup incomplete: {error}",
                        view.logical_name
                    ));
                    if fatal {
                        return;
                    }
                    retry_delay(&error, backoff.fail(), self.clock.now())
                }
                Err(ListenerFailure::Store(reason)) => {
                    view.store_error(reason);
                    self.publish_cleanup(&view);
                    return;
                }
                Err(ListenerFailure::Halted) => return,
            };
            tokio::select! {
                () = wait_for_stop(&mut stopping) => return,
                () = self.clock.sleep(delay) => {},
            }
        }
        let _turn = running.turn.lock().await;
        if *stop.borrow() || *self.drain.borrow() {
            return;
        }
        let ready = self.admit_waiting(running);
        self.prepare_and_start(ready, running, stop).await;
    }

    fn publish_cleanup(&self, view: &ListenerView) {
        self.listeners.send_modify(|views| {
            if view.has_pending_cleanup() {
                views.insert(view.logical_name.clone(), view.clone());
            } else {
                views.remove(&view.logical_name);
            }
        });
    }

    /// a failed edit still replaces the desired configuration before any waiting pool is readmitted.
    fn admit_waiting(&self, running: &Running<'a, K>) -> Vec<PoolSpec> {
        let (ready, waiting): (Vec<_>, Vec<_>) = running
            .waiting
            .take()
            .into_iter()
            .filter_map(|held| {
                running
                    .wanted
                    .borrow()
                    .iter()
                    .find(|pool| pool.name == held.name)
                    .cloned()
            })
            .partition(|pool| self.admit(pool));
        running.waiting.borrow_mut().extend(waiting);
        ready
    }

    /// passes the core's stop to every slot.
    async fn forward_stop(&self, running: &Running<'a, K>, stop: &watch::Receiver<bool>) {
        let mut stop = stop.clone();
        wait_for_stop(&mut stop).await;
        let deadline = Instant::now() + self.times.exit_deadline;
        for listener in running.listeners.borrow().values() {
            listener.coordinator.stop(deadline);
            listener.stop.send_replace(true);
        }
        for slot in running.slots.borrow().values() {
            slot.stop.send_replace(true);
        }
    }

    /// starts each pool that was skipped for want of a token once its repository has
    /// one, until the stop.
    async fn start_waiting(
        &self,
        mut changes: watch::Receiver<u64>,
        running: &Running<'a, K>,
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
            let ready = self.admit_waiting(running);
            self.prepare_and_start(ready, running, stop).await;
        }
    }

    /// prepares `pools` and starts their slots; a pool that cannot be prepared is
    /// skipped with its reason and left.
    async fn prepare_and_start(
        &self,
        pools: Vec<PoolSpec>,
        running: &Running<'a, K>,
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
        running: &Running<'a, K>,
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
    /// was skipped, is admitted and prepared again. A pool whose listener has
    /// cleanup left, or has not ended, is stopped and stays stopped, shown as skipped, until
    /// Start all, and the other pools take their effects all the same.
    async fn apply(
        &self,
        plan: &Plan,
        effects: &[Effect],
        running: &Running<'a, K>,
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
        self.board.expect_image(image_name(plan));
        let spec = |name: &str| pools.iter().find(|pool| pool.name == name).cloned();
        let steps: Vec<(&str, Step)> = effects
            .iter()
            .map(|effect| (effect.pool.as_str(), self.step(effect, running)))
            .collect();

        let deadline = Instant::now() + self.times.exit_deadline;
        let mut ending_listeners = Vec::new();
        for (pool, step) in &steps {
            if let Some(listener) = running.listeners.borrow().get(*pool) {
                match step {
                    Step::Remove | Step::Restart => {
                        listener.coordinator.stop(deadline);
                        listener.stop.send_replace(true);
                        ending_listeners.push((*pool, listener.ended.clone()));
                    }
                    Step::StartNew | Step::StopAbove(_) => {
                        if let Some(new) = spec(pool) {
                            listener.capacity.send_replace(new.slots);
                        }
                    }
                    Step::Nothing | Step::Rewait => {}
                }
            }
        }
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
        let listeners_ended = async {
            let mut late = Vec::new();
            for (pool, mut ended) in ending_listeners {
                if timeout_at(deadline, ended.wait_for(|ended| *ended))
                    .await
                    .is_err()
                {
                    late.push(pool);
                }
            }
            late
        };
        let ((), late) = tokio::join!(self.stop_slots(leaving, running), listeners_ended);
        // a pool whose listener cannot be replaced yet stays stopped, shown as skipped, while
        // the rest of the edit goes on, and the apply reports every such pool at the end
        let mut failures = Vec::new();
        let mut skipped = Vec::new();
        for pool in late {
            self.skip_stopped(
                pool,
                "the edit's listener cleanup did not finish within the stop deadline",
                &mut failures,
            );
            skipped.push(pool);
        }
        for (pool, step) in &steps {
            if matches!(step, Step::Remove | Step::Restart)
                && !skipped.contains(pool)
                && let Some(view) = running
                    .listeners
                    .borrow()
                    .get(*pool)
                    .map(|listener| listener.coordinator.view())
                    .or_else(|| self.listeners.borrow().get(*pool).cloned())
                && view.has_pending_cleanup()
            {
                self.skip_stopped(pool, "on-demand cleanup is incomplete", &mut failures);
                skipped.push(pool);
            }
        }
        running
            .listeners
            .borrow_mut()
            .retain(|_, listener| !*listener.ended.borrow());
        // the slots that leave were stopped under the pools they ran in
        self.backend.use_plan(plan);
        forget_macos_without_a_macos_pool(self.board, plan);

        let mut fresh = Vec::new();
        for (pool, step) in steps {
            if skipped.contains(&pool) {
                continue;
            }
            match step {
                Step::Remove => {
                    self.listeners.send_modify(|listeners| {
                        listeners.remove(pool);
                    });
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
                    self.listeners.send_modify(|listeners| {
                        listeners.remove(pool);
                    });
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
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    /// the edit has stopped this pool, and its listener's cleanup keeps it from starting
    /// again before Start all: the board shows it skipped, and the apply reports it.
    fn skip_stopped(&self, pool: &str, cause: &str, failures: &mut Vec<String>) {
        let why = format!("{cause}; Start all is required before another listener starts");
        self.board
            .set_pool(pool, PoolState::Skipped(format!("stopped; {why}")));
        failures.push(format!("pool {pool}: {why}"));
    }

    /// what the apply does to a pool, from its effect and whether it runs slots or
    /// waits for its token.
    fn step(&self, effect: &Effect, running: &Running<'a, K>) -> Step {
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
    async fn stop_slots(&self, slots: Vec<(String, u32)>, running: &Running<'a, K>) {
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
        let leaving_names: Vec<(String, u32)> = slots
            .iter()
            .filter_map(|key| {
                running
                    .slots
                    .borrow()
                    .get(key)
                    .map(|run| (run.runner_prefix.clone(), key.1))
            })
            .collect();
        let leaving = |name: &str| {
            leaving_names
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
    async fn finish_slot(&self, key: &(String, u32), running: &Running<'a, K>) -> bool {
        let (pool, number) = key;
        let host = JobHost {
            pool: pool.clone(),
            slot: *number,
        };
        let prepared = self.backend.prepared_hosts().contains(&host);
        let runner_prefix = running
            .slots
            .borrow()
            .get(key)
            .map_or_else(|| pool.clone(), |run| run.runner_prefix.clone());
        let retained = self
            .backend
            .retained()
            .names()
            .iter()
            .any(|name| is_slot_container(name, &runner_prefix, *number));
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
    /// again when the configuration in use still has it, until the stop; a pool
    /// deferred over such slots is prepared once none is held and no drain lasts. It
    /// finishes whichever late slot ends first, so a slow one delays no other.
    async fn finish_late(&self, running: &Running<'a, K>, stop: &watch::Receiver<bool>) {
        let mut stopping = stop.clone();
        let mut drain = self.drain.subscribe();
        loop {
            // a worker gone without saying so has ended too
            let has_ended =
                |ended: &watch::Receiver<bool>| *ended.borrow() || ended.has_changed().is_err();
            let next = running
                .late
                .borrow()
                .iter()
                .position(|(_, ended)| has_ended(ended));
            let Some(at) = next else {
                let mut late: Vec<_> = running
                    .late
                    .borrow()
                    .iter()
                    .map(|(_, ended)| ended.clone())
                    .collect();
                let mut waits: Vec<_> = late
                    .iter_mut()
                    .map(|ended| Box::pin(ended.wait_for(|ended| *ended)))
                    .collect();
                let one_ended = poll_fn(|context| {
                    if waits
                        .iter_mut()
                        .any(|wait| wait.as_mut().poll(context).is_ready())
                    {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                });
                tokio::select! {
                    () = wait_for_stop(&mut stopping) => return,
                    () = running.late_added.notified() => continue,
                    () = one_ended => continue,
                    changed = drain.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        let _turn = running.turn.lock().await;
                        self.start_deferred(running, stop).await;
                        continue;
                    }
                }
            };
            let (key, _) = running.late.borrow_mut().remove(at);
            let _turn = running.turn.lock().await;
            if !self.finish_slot(&key, running).await {
                continue;
            }
            if running
                .deferred
                .borrow()
                .iter()
                .any(|pool| pool.name == key.0)
            {
                self.start_deferred(running, stop).await;
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

    /// prepares each deferred pool that no longer holds a slot, or leaves it waiting
    /// for its token; nothing while a drain lasts, as no pool begins its preparation.
    async fn start_deferred(&self, running: &Running<'a, K>, stop: &watch::Receiver<bool>) {
        if *self.drain.borrow() {
            return;
        }
        let (free, held): (Vec<_>, Vec<_>) = running
            .deferred
            .take()
            .into_iter()
            .partition(|pool| !holds_slots(running, &pool.name));
        running.deferred.borrow_mut().extend(held);
        let (ready, waits): (Vec<_>, Vec<_>) = free.into_iter().partition(|pool| self.admit(pool));
        running.waiting.borrow_mut().extend(waits);
        if !ready.is_empty() {
            self.prepare_and_start(ready, running, stop).await;
        }
    }
}

/// drops the board's golden macOS VM record and leftover VMs when `plan` has no
/// macOS pool, as `start` does, so the snapshot does not keep them.
fn forget_macos_without_a_macos_pool(board: &Board, plan: &Plan) {
    if !plan.entries.iter().any(|entry| entry.os == Some(Os::Macos)) {
        board.forget_macos();
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
