use std::rc::Rc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::{Instant, timeout, timeout_at};

use crate::runtime::clock::Clock;
use crate::runtime::name::is_own_container_name;
use crate::runtime::secret::Secret;
use crate::runtime::token::TokenSource;
use crate::runtime::worker::Backoff;
use crate::scaleset::{Error, ErrorKind, Message};

use super::{
    Coordinator, Credentials, ListenerState, MIN_REFRESH_WAIT, PendingSession, REDELIVERY_WAIT,
    REFRESH_BEFORE,
};

pub(crate) const CLOSE_WITHIN: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) enum Failure {
    Api(Error),
    Store(String),
    Halted,
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Api(error)
    }
}

impl<T: TokenSource, K: Clock> Coordinator<'_, T, K> {
    /// session closure starts on the stop edge, independently of every runtime worker.
    pub async fn run(&self) {
        let mut stop = self.controls.stop.clone();
        let mut drain = self.controls.drain.clone();
        let mut capacity = self.controls.capacity.clone();
        let mut backoff = Backoff::default();
        let mut pending = None;
        let mut last_processed = None;
        let mut cleaned = false;
        let mut session_id = String::new();
        loop {
            if self.stopped() {
                break;
            }
            let recreate = self.state.borrow().recreate_requested;
            if recreate {
                self.state.borrow_mut().recreated = true;
                self.state.borrow_mut().recreate_requested = false;
                self.close().await;
                self.credentials.borrow_mut().take();
                pending = None;
                last_processed = None;
                session_id.clear();
            }
            let wait = {
                let state = self.state.borrow();
                state
                    .retry_at
                    .and_then(|at| at.duration_since(self.clock.now()).ok())
            };
            if self.state.borrow().fatal || self.halted() {
                tokio::select! {
                    _ = stop.changed() => {},
                    _ = drain.changed() => {},
                    _ = capacity.changed() => {},
                    () = self.wake.notified() => {},
                }
                continue;
            }
            if let Some(wait) = wait.filter(|wait| !wait.is_zero()) {
                tokio::select! {
                    () = self.clock.sleep(wait) => {},
                    _ = stop.changed() => {},
                    _ = drain.changed() => {},
                    _ = capacity.changed() => {},
                    () = self.wake.notified() => {},
                }
                continue;
            }
            let result = tokio::select! {
                biased;
                _ = stop.changed() => None,
                _ = drain.changed() => None,
                _ = capacity.changed() => None,
                () = self.wake.notified() => None,
                result = self.cycle(&mut pending, &mut last_processed, &mut session_id, &mut cleaned) => Some(result),
            };
            match result {
                None | Some(Err(Failure::Halted)) => {}
                Some(Ok(())) => backoff.reset(),
                Some(Err(Failure::Store(reason))) => {
                    let mut state = self.state.borrow_mut();
                    state.view.state = ListenerState::StoreError;
                    state.view.status = None;
                    state.view.refused_host = None;
                    state.view.reason = Some(reason);
                    state.fatal = true;
                    drop(state);
                    self.publish();
                }
                Some(Err(Failure::Api(error))) => {
                    if error.kind == ErrorKind::NotFound && !self.state.borrow().recreated {
                        self.state.borrow_mut().recreate_requested = true;
                        continue;
                    }
                    self.failed(&error, backoff.fail());
                }
            }
        }
        self.state.borrow_mut().closing = true;
        self.close().await;
        self.state.borrow_mut().view.state = ListenerState::Stopped;
        self.publish();
    }

    /// neither session refresh nor 202 creates a synthetic zero-demand update.
    pub(super) async fn cycle(
        &self,
        pending: &mut Option<Message>,
        last_processed: &mut Option<u64>,
        session_id: &mut String,
        cleaned: &mut bool,
    ) -> Result<(), Failure> {
        if self.credentials().is_none() {
            self.open(cleaned).await?;
        }
        let mut credentials = self.renew(true).await?;
        if session_id.as_str() != credentials.session.id() {
            *session_id = credentials.session.id().to_owned();
            *last_processed = None;
            *pending = None;
            self.state
                .borrow_mut()
                .demand
                .initial(credentials.session.statistics);
            self.state.borrow_mut().view.statistics = Some(credentials.session.statistics);
        }
        self.listening();
        if self.halted() {
            return Err(Failure::Halted);
        }
        let uncertain = self.state.borrow().demand.uncertain() > 0;
        if uncertain {
            // an acquire may have succeeded before its response vanished; 202 cannot reveal its assigned count.
            self.refresh_queue(&credentials).await?;
            credentials = self.credentials().ok_or(Failure::Halted)?;
            self.state
                .borrow_mut()
                .demand
                .update_statistics(credentials.session.statistics);
            self.state.borrow_mut().view.statistics = Some(credentials.session.statistics);
        }
        let released = self.state.borrow().demand.released;
        {
            let ids = self
                .state
                .borrow_mut()
                .demand
                .acquire(*self.controls.capacity.borrow());
            if !ids.is_empty() {
                let admission = self.admission_epoch();
                if self.halted() {
                    return Err(Failure::Halted);
                }
                let acquired = match self
                    .client
                    .acquire_jobs(&credentials.connection, &credentials.session, &ids)
                    .await
                {
                    Err(error) if error.status == Some(401) => {
                        self.queue_unauthorized(&credentials, error).await?;
                        return Ok(());
                    }
                    other => other?,
                };
                self.state.borrow_mut().queue_refreshed = false;
                self.state.borrow_mut().demand.acquired(&ids, &acquired);
                self.publish();
                if self.halted() || admission.changed() {
                    return Err(Failure::Halted);
                }
            }
        }
        let processed = self.state.borrow().demand.processed();
        if let Some(message) = pending.as_ref()
            && processed
        {
            match self
                .client
                .acknowledge(&credentials.session, message.id)
                .await
            {
                Err(error) if error.status == Some(401) => {
                    self.queue_unauthorized(&credentials, error).await?;
                    return Ok(());
                }
                other => other?,
            }
            self.state.borrow_mut().queue_refreshed = false;
            *last_processed = Some(message.id);
            self.state.borrow_mut().demand.acknowledged();
            *pending = None;
        }
        if self.halted() {
            return Err(Failure::Halted);
        }
        if pending.is_some() && self.await_release(released).await {
            return Ok(());
        }
        let refresh_in = credentials
            .connection
            .expires_at()
            .duration_since(self.clock.now())
            .unwrap_or_default()
            .saturating_sub(REFRESH_BEFORE)
            .max(MIN_REFRESH_WAIT);
        let max_capacity = *self.controls.capacity.borrow();
        let result = tokio::select! {
            result = self.client.poll(&credentials.session, *last_processed, max_capacity) => result,
            () = self.clock.sleep(refresh_in) => return Ok(()),
        };
        let message = match result {
            Err(error) if error.status == Some(401) => {
                self.queue_unauthorized(&credentials, error).await?;
                return Ok(());
            }
            other => other?,
        };
        {
            let mut state = self.state.borrow_mut();
            state.queue_refreshed = false;
            // a later, independent 404 may recreate the set once more; only a consecutive one is a rejection
            state.recreated = false;
        }
        if let Some(message) = message {
            if last_processed.is_some_and(|id| message.id <= id) {
                self.clock.sleep(REDELIVERY_WAIT).await;
                return Ok(());
            }
            // progress cannot skip an unresolved overflow message even if a broken service advances delivery.
            if pending.as_ref().is_some_and(|old| old.id != message.id) {
                return Err(Failure::Api(super::protocol_error()));
            }
            self.state.borrow_mut().demand.receive(&message);
            self.state.borrow_mut().view.statistics = Some(message.statistics);
            *pending = Some(message);
            self.publish();
        }
        Ok(())
    }

    /// an overflow message stays unacknowledged, and the service hands it back at once, so the
    /// listener waits for a seat to free instead; false when the wait ran out and the statistics
    /// are due. `released` is what `Demand::released` read before this cycle's acquire.
    async fn await_release(&self, released: u64) -> bool {
        let mut changes = self.changes();
        let freed = async {
            while self.state.borrow().demand.released == released {
                if changes.changed().await.is_err() {
                    return std::future::pending().await;
                }
            }
        };
        tokio::select! {
            () = freed => true,
            () = self.clock.sleep(REDELIVERY_WAIT) => false,
        }
    }

    /// cleanup precedes the first session, and only offline whole-name matches are adopted.
    async fn open(&self, cleaned: &mut bool) -> Result<(), Failure> {
        let _renewal = self.renewal.lock().await;
        if self.halted() {
            return Err(Failure::Halted);
        }
        if self.pending_session.borrow().is_some() {
            timeout(
                CLOSE_WITHIN,
                PendingSession::retry(
                    &self.pending_session,
                    self.client,
                    self.tokens,
                    self.clock.now(),
                ),
            )
            .await
            .map_err(|_| Failure::Api(super::protocol_error()))??;
            self.state.borrow_mut().view.session_cleanup_incomplete = false;
        }
        let pat = self.token(true).map_err(Failure::Store)?;
        let connection = Rc::new(
            self.client
                .connect(&self.pool.owner, &self.pool.repository, &pat)
                .await?,
        );
        let set = self
            .client
            .find_or_create(&connection, &self.pool.scale_set)
            .await?;
        if self.halted() {
            return Err(Failure::Halted);
        }
        if !*cleaned {
            let runners = self
                .client
                .list_runners(&self.pool.owner, &self.pool.repository, &pat)
                .await?;
            for runner in runners.into_iter().filter(|runner| {
                runner.status.eq_ignore_ascii_case("offline")
                    && is_own_container_name(&self.pool.scale_set, &runner.name)
            }) {
                self.state
                    .borrow_mut()
                    .owned
                    .entry(0)
                    .or_default()
                    .names
                    .insert(runner.name.clone(), Some(runner.id));
                if let Some(owned) = self.state.borrow_mut().owned.get_mut(&0) {
                    owned.incomplete = true;
                }
            }
            *cleaned = true;
            self.publish();
        }
        let leftovers = self
            .state
            .borrow()
            .owned
            .get(&0)
            .map(|owned| owned.names.clone())
            .unwrap_or_default();
        for (name, id) in leftovers {
            let Some(id) = id else {
                return Err(Failure::Api(super::protocol_error()));
            };
            self.client
                .delete_runner(&self.pool.owner, &self.pool.repository, id, &name, &pat)
                .await?;
            if let Some(owned) = self.state.borrow_mut().owned.get_mut(&0) {
                owned.names.remove(&name);
            }
        }
        self.state.borrow_mut().owned.remove(&0);
        if self.halted() {
            return Err(Failure::Halted);
        }
        self.state.borrow_mut().view.session_cleanup_incomplete = true;
        *self.pending_session.borrow_mut() = Some(PendingSession::creating(
            Rc::clone(&connection),
            set.clone(),
            self.pool.session_owner.clone(),
        ));
        let session = match self
            .client
            .create_session(&connection, &set, &self.pool.session_owner)
            .await
        {
            Ok(session) => Rc::new(session),
            Err(error) => {
                if matches!(
                    error.kind,
                    ErrorKind::Conflict
                        | ErrorKind::Rejected
                        | ErrorKind::NotFound
                        | ErrorKind::RateLimited
                ) {
                    self.state.borrow_mut().view.session_cleanup_incomplete = false;
                    self.pending_session.borrow_mut().take();
                }
                return Err(Failure::Api(error));
            }
        };
        *self.credentials.borrow_mut() = Some(Credentials {
            connection,
            scale_set: set,
            session,
        });
        self.pending_session.borrow_mut().take();
        self.state.borrow_mut().view.session_cleanup_incomplete = false;
        Ok(())
    }

    /// an idle listener reads the store only when renewing, not for every poll.
    pub(super) async fn renew(&self, fatal_store: bool) -> Result<Credentials, Failure> {
        let _renewal = self.renewal.lock().await;
        let Some(current) = self.credentials() else {
            return Err(Failure::Halted);
        };
        if current
            .connection
            .expires_at()
            .duration_since(self.clock.now())
            .unwrap_or_default()
            > REFRESH_BEFORE
        {
            return Ok(current);
        }
        let pat = self.token(fatal_store).map_err(Failure::Store)?;
        let renewed = self.renew_for_pat(current, &pat).await?;
        // a credential that is due again at once cannot register a runner, and renewing it
        // each cycle would spend the shared token's budget; it backs off like any failure.
        if renewed
            .connection
            .expires_at()
            .duration_since(self.clock.now())
            .unwrap_or_default()
            <= REFRESH_BEFORE
        {
            return Err(Failure::Api(super::error(
                ErrorKind::ConnectionRenewalRequired,
            )));
        }
        Ok(renewed)
    }

    /// PATCH keeps the queue/session identity when an unexpired credential was produced by an old PAT.
    pub(super) async fn renew_for_pat(
        &self,
        current: Credentials,
        pat: &Secret,
    ) -> Result<Credentials, Failure> {
        if self.halted() {
            return Err(Failure::Halted);
        }
        let connection = Rc::new(
            self.client
                .connect(&self.pool.owner, &self.pool.repository, pat)
                .await?,
        );
        let scale_set = self
            .client
            .find_or_create(&connection, &self.pool.scale_set)
            .await?;
        if scale_set.id() != current.scale_set.id() {
            return Err(Failure::Api(super::protocol_error()));
        }
        if self.halted() {
            return Err(Failure::Halted);
        }
        let session = Rc::new(
            self.client
                .refresh_session(&connection, &current.session)
                .await?,
        );
        let updated = Credentials {
            connection,
            scale_set,
            session,
        };
        *self.credentials.borrow_mut() = Some(updated.clone());
        Ok(updated)
    }

    /// only queue 401 gets a PATCH; a second failure is classified normally by the run loop.
    async fn queue_unauthorized(&self, current: &Credentials, error: Error) -> Result<(), Failure> {
        if self.state.borrow().queue_refreshed {
            return Err(Failure::Api(error));
        }
        self.state.borrow_mut().queue_refreshed = true;
        self.refresh_queue(current).await
    }

    /// refresh reads the store even when admin credentials have not expired.
    async fn refresh_queue(&self, current: &Credentials) -> Result<(), Failure> {
        let _renewal = self.renewal.lock().await;
        let pat = self.token(true).map_err(Failure::Store)?;
        if current.connection.needs_renewal(&pat, self.clock.now()) {
            self.renew_for_pat(current.clone(), &pat).await?;
        } else {
            if self.halted() {
                return Err(Failure::Halted);
            }
            let session = Rc::new(
                self.client
                    .refresh_session(&current.connection, &current.session)
                    .await?,
            );
            *self.credentials.borrow_mut() = Some(Credentials {
                session,
                ..current.clone()
            });
        }
        Ok(())
    }

    /// closure's timeout is also capped by the already-running stop deadline.
    async fn close(&self) {
        let Some(current) = self.credentials.borrow_mut().take() else {
            return;
        };
        *self.pending_session.borrow_mut() = Some(PendingSession::known(
            current.connection,
            current.scale_set,
            current.session,
            self.pool.session_owner.clone(),
        ));
        self.state.borrow_mut().view.session_cleanup_incomplete = true;
        self.publish();
        let deadline = self
            .state
            .borrow()
            .stop_deadline
            .unwrap_or_else(|| Instant::now() + CLOSE_WITHIN)
            .min(Instant::now() + CLOSE_WITHIN);
        let result = timeout_at(
            deadline,
            PendingSession::retry(
                &self.pending_session,
                self.client,
                self.tokens,
                self.clock.now(),
            ),
        )
        .await;
        let complete = matches!(result, Ok(Ok(())));
        self.state.borrow_mut().view.session_cleanup_incomplete = !complete;
        if !complete {
            self.log.warn(&format!(
                "pool {}: session cleanup incomplete",
                self.pool.logical_name
            ));
        }
        self.publish();
    }
}

pub(super) struct AdmissionEpoch {
    stop: watch::Receiver<bool>,
    drain: watch::Receiver<bool>,
    capacity: watch::Receiver<u32>,
}

impl AdmissionEpoch {
    pub fn changed(&self) -> bool {
        self.stop.has_changed().unwrap_or(true)
            || self.drain.has_changed().unwrap_or(true)
            || self.capacity.has_changed().unwrap_or(true)
    }
}

impl<T: TokenSource, K: Clock> Coordinator<'_, T, K> {
    /// a drain toggled on and off during a request still invalidates that admission.
    pub(super) fn admission_epoch(&self) -> AdmissionEpoch {
        let mut stop = self.controls.stop.clone();
        let mut drain = self.controls.drain.clone();
        let mut capacity = self.controls.capacity.clone();
        stop.borrow_and_update();
        drain.borrow_and_update();
        capacity.borrow_and_update();
        AdmissionEpoch {
            stop,
            drain,
            capacity,
        }
    }
}
