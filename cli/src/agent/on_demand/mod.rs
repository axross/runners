//! pool-scoped listener and registration ownership, independent of display subscriptions.

mod demand;
mod listener;
mod recovery;
mod registration;
mod slot;

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use tokio::sync::{Mutex, Notify, watch};
use tokio::time::Instant;

use crate::runtime::clock::Clock;
use crate::runtime::log::Log;
use crate::runtime::token::TokenSource;
use crate::scaleset::{Client, Connection, Error, ErrorKind, ScaleSet, Session, Statistics};

use demand::Demand;
pub(crate) use listener::{CLOSE_WITHIN, Failure};
pub(crate) use recovery::{PendingSession, PendingSessionCell, SessionRecovery};
pub(crate) use slot::run_on_demand_slot;

/// the admin credential is renewed once this little of its lifetime remains.
pub(super) const REFRESH_BEFORE: Duration = Duration::from_secs(120);

/// two renewals are never closer than this, whatever lifetime or clock the credential shows.
pub(super) const MIN_REFRESH_WAIT: Duration = Duration::from_secs(30);

/// two recreations of the scale set's session are never closer than this: each costs a
/// registration token and exchange, and a service that keeps answering 404 must not drive them.
pub(super) const MIN_RECREATION_WAIT: Duration = Duration::from_secs(30);

/// how long the listener leaves an unresolved message alone before asking the service again; the
/// service redelivers it at once, and the wait ends early when a seat frees.
pub(super) const REDELIVERY_WAIT: Duration = Duration::from_secs(10);

/// logical identity stays distinct from the scale set's routing/runner identity.
pub struct Pool {
    pub owner: String,
    pub repository: String,
    pub logical_name: String,
    pub scale_set: String,
    pub session_owner: String,
}

/// changes take effect at admission boundaries, not at the next broker response.
pub struct Controls {
    pub stop: watch::Receiver<bool>,
    pub drain: watch::Receiver<bool>,
    pub capacity: watch::Receiver<u32>,
}

/// non-secret states for the board and the state topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerState {
    Listening,
    AnotherListener,
    Rejected,
    StoreError,
    BackingOff,
    Stopped,
}

/// exact owned identities whose absence GitHub has not yet confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteRegistration {
    pub slot: u32,
    pub runner_id: Option<u64>,
    pub name: String,
}

/// no service URL, session ID or credential is exposed through this view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub logical_name: String,
    pub owner: String,
    pub repository: String,
    pub scale_set: String,
    pub state: ListenerState,
    pub status: Option<u16>,
    pub refused_host: Option<String>,
    pub reason: Option<String>,
    pub statistics: Option<Statistics>,
    pub active_reservations: usize,
    pub occupied_capacity: usize,
    pub incomplete_registrations: Vec<IncompleteRegistration>,
    pub session_cleanup_incomplete: bool,
}

impl View {
    pub fn has_pending_cleanup(&self) -> bool {
        self.session_cleanup_incomplete || !self.incomplete_registrations.is_empty()
    }

    /// a failed store read stops the pool, with the store's own kind and code as the reason.
    pub(crate) fn store_error(&mut self, reason: String) {
        self.state = ListenerState::StoreError;
        self.status = None;
        self.refused_host = None;
        self.reason = Some(reason);
    }

    pub(crate) fn failed(&mut self, error: &Error, recreated: bool) -> bool {
        let fatal = is_permanent(&error.kind) || (error.kind == ErrorKind::NotFound && recreated);
        self.state = if fatal {
            ListenerState::Rejected
        } else if error.kind == ErrorKind::Conflict {
            ListenerState::AnotherListener
        } else {
            ListenerState::BackingOff
        };
        self.status = error.status;
        self.refused_host = error.refused_host.clone();
        self.reason = Some(error.to_string());
        fatal
    }
}

#[derive(Clone)]
struct Credentials {
    connection: Rc<Connection>,
    scale_set: ScaleSet,
    session: Rc<Session>,
}

#[derive(Default)]
struct Owned {
    names: BTreeMap<String, Option<u64>>,
    incomplete: bool,
}

struct State {
    view: View,
    demand: Demand,
    owned: BTreeMap<u64, Owned>,
    retry_at: Option<SystemTime>,
    fatal: bool,
    closing: bool,
    recreated: bool,
    recreate_requested: bool,
    last_recreation: Option<SystemTime>,
    queue_refreshed: bool,
    stop_deadline: Option<Instant>,
}

/// one instance is shared by the listener future and all of its slot futures; it spawns nothing.
pub struct Coordinator<'a, T, K> {
    pool: Pool,
    client: &'a Client,
    tokens: &'a T,
    clock: &'a K,
    log: &'a dyn Log,
    controls: Controls,
    credentials: RefCell<Option<Credentials>>,
    pending_session: PendingSessionCell,
    renewal: Mutex<()>,
    state: RefCell<State>,
    changed: watch::Sender<u64>,
    wake: Notify,
}

/// a reservation must be finished even when the runtime future or JIT request was cancelled.
pub struct Reservation<'c, 'a, T, K> {
    coordinator: &'c Coordinator<'a, T, K>,
    id: u64,
    slot: u32,
    stop: watch::Receiver<bool>,
    finished: bool,
}

impl<T, K> Drop for Reservation<'_, '_, T, K> {
    fn drop(&mut self) {
        if !self.finished {
            if let Some(owned) = self.coordinator.state.borrow_mut().owned.get_mut(&self.id) {
                owned.incomplete = true;
            }
            self.coordinator
                .changed
                .send_modify(|version| *version = version.wrapping_add(1));
        }
    }
}

impl<'a, T: TokenSource, K: Clock> Coordinator<'a, T, K> {
    /// the caller polls `run` beside its runtime workers, never after their teardown.
    pub fn new(
        pool: Pool,
        client: &'a Client,
        tokens: &'a T,
        clock: &'a K,
        log: &'a dyn Log,
        controls: Controls,
    ) -> Self {
        let view = View {
            logical_name: pool.logical_name.clone(),
            owner: pool.owner.clone(),
            repository: pool.repository.clone(),
            scale_set: pool.scale_set.clone(),
            state: ListenerState::Stopped,
            status: None,
            refused_host: None,
            reason: None,
            statistics: None,
            active_reservations: 0,
            occupied_capacity: 0,
            incomplete_registrations: Vec::new(),
            session_cleanup_incomplete: false,
        };
        Self {
            pool,
            client,
            tokens,
            clock,
            log,
            controls,
            credentials: RefCell::new(None),
            pending_session: Rc::new(RefCell::new(None)),
            renewal: Mutex::new(()),
            state: RefCell::new(State {
                view,
                demand: Demand::default(),
                owned: BTreeMap::new(),
                retry_at: None,
                fatal: false,
                closing: false,
                recreated: false,
                recreate_requested: false,
                last_recreation: None,
                queue_refreshed: false,
                stop_deadline: None,
            }),
            changed: watch::Sender::new(0),
            wake: Notify::new(),
        }
    }

    pub(crate) fn with_session_recovery(mut self, pending: PendingSessionCell) -> Self {
        self.pending_session = pending;
        self
    }

    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// statistics survive 202s and stops; cleanup uncertainty is never rendered as idle success.
    pub fn view(&self) -> View {
        let state = self.state.borrow();
        let mut view = state.view.clone();
        view.active_reservations = state.demand.leases.len();
        view.occupied_capacity = state.demand.occupied();
        view.incomplete_registrations = state
            .owned
            .iter()
            .filter(|(_, owned)| owned.incomplete || state.closing)
            .flat_map(|(id, owned)| {
                let slot = state.demand.leases.get(id).map_or(0, |lease| lease.slot);
                owned
                    .names
                    .iter()
                    .map(move |(name, runner_id)| IncompleteRegistration {
                        slot,
                        runner_id: *runner_id,
                        name: name.clone(),
                    })
            })
            .collect();
        view
    }

    fn publish(&self) {
        self.changed
            .send_modify(|version| *version = version.wrapping_add(1));
    }

    fn stopped(&self) -> bool {
        *self.controls.stop.borrow()
            || self.controls.stop.has_changed().is_err()
            || self.state.borrow().closing
    }

    fn halted(&self) -> bool {
        self.stopped()
            || *self.controls.drain.borrow()
            || self.controls.drain.has_changed().is_err()
            || self.controls.capacity.has_changed().is_err()
    }

    fn admitting(&self) -> bool {
        let state = self.state.borrow();
        !self.halted()
            && !state.fatal
            && !state.recreate_requested
            && state.view.state == ListenerState::Listening
            && state.retry_at.is_none_or(|at| self.clock.now() >= at)
    }

    /// stopping a removed pool is immediate; pass the existing absolute stop deadline, not a fresh budget.
    pub fn stop(&self, deadline: Instant) {
        let mut state = self.state.borrow_mut();
        state.closing = true;
        state.stop_deadline = Some(
            state
                .stop_deadline
                .map_or(deadline, |old| old.min(deadline)),
        );
        drop(state);
        self.publish();
        self.wake.notify_waiters();
    }

    /// waits without holding a worker or credential lock while the pool has no demand.
    pub async fn reserve(
        &self,
        slot: u32,
        mut slot_stop: watch::Receiver<bool>,
    ) -> Option<Reservation<'_, 'a, T, K>> {
        let mut changes = self.changes();
        let mut capacity = self.controls.capacity.clone();
        let mut drain = self.controls.drain.clone();
        let mut stop = self.controls.stop.clone();
        loop {
            if self.stopped() || *slot_stop.borrow() || slot_stop.has_changed().is_err() {
                return None;
            }
            if self.admitting() {
                let id = self
                    .state
                    .borrow_mut()
                    .demand
                    .reserve(slot, *capacity.borrow());
                if let Some(id) = id {
                    self.state.borrow_mut().owned.insert(id, Owned::default());
                    self.publish();
                    self.wake.notify_waiters();
                    return Some(Reservation {
                        coordinator: self,
                        id,
                        slot,
                        stop: slot_stop,
                        finished: false,
                    });
                }
            }
            tokio::select! {
                _ = changes.changed() => {},
                _ = capacity.changed() => {},
                _ = drain.changed() => {},
                _ = stop.changed() => {},
                _ = slot_stop.changed() => {},
            }
        }
    }

    fn credentials(&self) -> Option<Credentials> {
        self.credentials.borrow().clone()
    }

    /// the source already limits store errors to its public kind/code description.
    fn token(&self, fatal: bool) -> Result<crate::runtime::secret::Secret, String> {
        self.tokens
            .token(&self.pool.owner, &self.pool.repository, None)
            .inspect_err(|reason| {
                let mut state = self.state.borrow_mut();
                state.view.store_error(reason.clone());
                state.fatal |= fatal;
                drop(state);
                self.publish();
            })
    }

    /// classifies only the client's sanitized failure fields; rate hints are never shortened.
    fn failed(&self, error: &Error, delay: Duration) {
        let mut state = self.state.borrow_mut();
        let recreated = state.recreated;
        let fatal = state.view.failed(error, recreated);
        state.fatal |= fatal;
        let delay = retry_delay(error, delay, self.clock.now());
        state.retry_at = self.clock.now().checked_add(delay);
        drop(state);
        self.log
            .warn(&format!("pool {}: {error}", self.pool.logical_name));
        self.publish();
    }

    fn listening(&self) {
        let mut state = self.state.borrow_mut();
        if !state.fatal {
            state.view.state = ListenerState::Listening;
            state.view.status = None;
            state.view.refused_host = None;
            state.view.reason = None;
            state.retry_at = None;
        }
        drop(state);
        self.publish();
    }
}

/// a retry cannot change these: the token lacks the permission, or the identity is not ours.
pub(crate) fn is_permanent(kind: &ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::Rejected
            | ErrorKind::RefusedHost
            | ErrorKind::LabelMismatch
            | ErrorKind::InvalidInput
            | ErrorKind::Setup
            | ErrorKind::OwnershipMismatch
    )
}

pub(crate) fn retry_delay(error: &Error, delay: Duration, now: SystemTime) -> Duration {
    let reset = error
        .rate_reset
        .and_then(|seconds| std::time::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
        .and_then(|reset| reset.duration_since(now).ok())
        .unwrap_or_default();
    delay.max(error.retry_after.unwrap_or_default()).max(reset)
}

pub(crate) fn protocol_error() -> Error {
    error(ErrorKind::Protocol)
}

fn error(kind: ErrorKind) -> Error {
    Error {
        kind,
        status: None,
        retry_after: None,
        rate_reset: None,
        refused_host: None,
    }
}
