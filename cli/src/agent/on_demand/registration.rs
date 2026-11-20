use tokio::time::{Instant, timeout_at};

use crate::runtime::clock::Clock;
use crate::runtime::name::{is_own_container_name, slot_of_container_name};
use crate::runtime::registration::{JitRequest, Registrar, RegistrationError};
use crate::runtime::secret::Secret;
use crate::runtime::token::TokenSource;
use crate::runtime::worker::INITIAL_BACKOFF;

use crate::scaleset::Error;

use super::listener::Failure;
use super::{Coordinator, REFRESH_BEFORE, Reservation, View, is_permanent};

const MAX_CREDENTIAL_ROTATIONS: u32 = 3;
const HALTED: &str = "on-demand registration halted";

/// why one cleanup attempt failed, and whether another could change that.
#[derive(Debug)]
pub(crate) struct CleanupFailure {
    pub reason: String,
    pub permanent: bool,
}

impl From<String> for CleanupFailure {
    /// store errors and deadlines: a later attempt reads the store and the clock again.
    fn from(reason: String) -> Self {
        Self {
            reason,
            permanent: false,
        }
    }
}

impl From<Error> for CleanupFailure {
    fn from(error: Error) -> Self {
        Self {
            permanent: is_permanent(&error.kind),
            reason: error.to_string(),
        }
    }
}

impl View {
    /// recovery uses the originating repository even after its configuration entry is gone.
    pub async fn retry_registration_cleanup<T: TokenSource>(
        &mut self,
        client: &crate::scaleset::Client,
        tokens: &T,
    ) -> Result<(), String> {
        while let Some(pending) = self.incomplete_registrations.first_mut() {
            let pat = tokens.token(&self.owner, &self.repository, None)?;
            if pending.runner_id.is_none() {
                pending.runner_id = client
                    .find_runner(&self.owner, &self.repository, &pending.name, &pat)
                    .await
                    .map_err(|error| error.to_string())?
                    .map(|runner| runner.id);
            }
            if let Some(id) = pending.runner_id {
                client
                    .delete_runner(&self.owner, &self.repository, id, &pending.name, &pat)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            self.incomplete_registrations.remove(0);
        }
        Ok(())
    }
}

impl<T: TokenSource, K: Clock> Reservation<'_, '_, T, K> {
    fn halted(&self) -> bool {
        self.finished
            || self.coordinator.halted()
            || *self.stop.borrow()
            || self.stop.has_changed().is_err()
            || self.slot > *self.coordinator.controls.capacity.borrow()
    }

    /// call after runtime release, even on cancellation; failed cleanup retains its capacity lease.
    pub(crate) async fn finish(
        &mut self,
        completed_job: bool,
        deadline: Instant,
    ) -> Result<(), CleanupFailure> {
        if self.finished {
            return Ok(());
        }
        let pool = self.coordinator;
        let deadline = pool
            .state
            .borrow()
            .stop_deadline
            .map_or(deadline, |stop| stop.min(deadline));
        let cleanup = timeout_at(deadline, pool.cleanup(self.id)).await;
        match cleanup {
            Ok(Ok(())) => {
                let mut state = pool.state.borrow_mut();
                state.owned.remove(&self.id);
                state.demand.finish(self.id, completed_job);
                self.finished = true;
                drop(state);
                pool.publish();
                pool.wake.notify_waiters();
                Ok(())
            }
            result => {
                let failure = match result {
                    Ok(Err(failure)) => failure,
                    Err(_) => CleanupFailure::from(
                        "registration cleanup deadline reached; outcome unconfirmed".to_owned(),
                    ),
                    Ok(Ok(())) => return Ok(()),
                };
                pool.cleanup_failed(self.id, &failure.reason);
                Err(failure)
            }
        }
    }
}

impl<T: TokenSource, K: Clock> Reservation<'_, '_, T, K> {
    /// ends `work` when a stop arrives, however long its requests would retry or wait. Only the
    /// steps before the JIT request go through it: they record no ownership, so none is lost.
    async fn unless_stopped<R>(
        &self,
        work: impl Future<Output = Result<R, String>>,
    ) -> Result<R, String> {
        let mut pool_stop = self.coordinator.controls.stop.clone();
        let mut slot_stop = self.stop.clone();
        tokio::select! {
            biased;
            _ = pool_stop.wait_for(|stopped| *stopped) => Err(HALTED.to_owned()),
            _ = slot_stop.wait_for(|stopped| *stopped) => Err(HALTED.to_owned()),
            result = work => result,
        }
    }
}

impl<T: TokenSource, K: Clock> Registrar for Reservation<'_, '_, T, K> {
    /// ignores the runtime's token copy: the store is authoritative at every JIT attempt.
    async fn request_jit(&self, request: &JitRequest<'_>) -> Result<Secret, RegistrationError> {
        self.register(request).await.map_err(RegistrationError::new)
    }
}

impl<T: TokenSource, K: Clock> Reservation<'_, '_, T, K> {
    /// exact ownership is recorded before HTTP so dropping this future cannot lose an uncertain attempt.
    async fn register(&self, request: &JitRequest<'_>) -> Result<Secret, String> {
        let pool = self.coordinator;
        if self.halted() {
            return Err(HALTED.to_owned());
        }
        if request.owner != pool.pool.owner
            || request.repository != pool.pool.repository
            || !is_own_container_name(&pool.pool.scale_set, request.name)
            || slot_of_container_name(request.name).is_none_or(|(_, slot)| slot != self.slot)
        {
            return Err("on-demand registration identity mismatch".to_owned());
        }
        let epoch = pool.admission_epoch();
        let mut slot_epoch = self.stop.clone();
        slot_epoch.borrow_and_update();
        let _renewal = self
            .unless_stopped(async { Ok(pool.renewal.lock().await) })
            .await?;
        if self.halted() || epoch.changed() || slot_epoch.has_changed().unwrap_or(true) {
            return Err(HALTED.to_owned());
        }
        let Some(mut current) = pool.credentials() else {
            return Err("on-demand listener has no session".to_owned());
        };
        let refreshing = current
            .connection
            .expires_at()
            .duration_since(pool.clock.now())
            .unwrap_or_default()
            <= REFRESH_BEFORE;
        // a previous response may have been lost or cancelled; absence must be confirmed before retrying.
        self.unless_stopped(async {
            pool.cleanup(self.id)
                .await
                .map_err(|failure| failure.reason)
        })
        .await?;
        let mut pat = pool.token(refreshing)?;
        let mut renewals = 0;
        while current.connection.needs_renewal(&pat, pool.clock.now()) {
            if renewals == MAX_CREDENTIAL_ROTATIONS {
                return Err(
                    "stored token changed during credential renewal; retry required".to_owned(),
                );
            }
            renewals += 1;
            current = self
                .unless_stopped(async {
                    pool.renew_for_pat(current, &pat)
                        .await
                        .map_err(|failure| pool.registration_failed(failure))
                })
                .await?;
            // a token set during the renewal asks for the next one
            pat = pool.token(refreshing)?;
        }
        if self.halted() || epoch.changed() || slot_epoch.has_changed().unwrap_or(true) {
            return Err(HALTED.to_owned());
        }
        {
            let mut state = pool.state.borrow_mut();
            if state.fatal
                || state.recreate_requested
                || state.retry_at.is_some_and(|at| pool.clock.now() < at)
            {
                return Err("on-demand listener is not admitting registrations".to_owned());
            }
            let Some(owned) = state.owned.get_mut(&self.id) else {
                return Err("on-demand reservation is no longer active".to_owned());
            };
            owned.names.insert(request.name.to_owned(), None);
        }
        let jit = pool
            .client
            .request_jit(
                &current.connection,
                &current.scale_set,
                request.name,
                &pat,
                pool.clock.now(),
            )
            .await
            .map_err(|error| pool.registration_failed(Failure::Api(error)))?;
        {
            let mut state = pool.state.borrow_mut();
            if let Some(owned) = state.owned.get_mut(&self.id) {
                owned.names.insert(jit.name, Some(jit.id));
            }
        }
        pool.publish();
        if self.halted() || epoch.changed() || slot_epoch.has_changed().unwrap_or(true) {
            return Err("on-demand registration halted; cleanup required".to_owned());
        }
        Ok(jit.config)
    }
}

impl<T: TokenSource, K: Clock> Coordinator<'_, T, K> {
    fn registration_failed(&self, failure: Failure) -> String {
        match failure {
            Failure::Api(error) => {
                self.failed(&error, INITIAL_BACKOFF);
                if error.kind == crate::scaleset::ErrorKind::NotFound
                    && !self.state.borrow().recreated
                {
                    self.state.borrow_mut().recreate_requested = true;
                    self.wake.notify_waiters();
                }
                error.to_string()
            }
            Failure::Store(reason) => reason,
            Failure::Halted => HALTED.to_owned(),
        }
    }

    /// the current token is required for cleanup too; unreadable stores do not become a cache bypass.
    async fn cleanup(&self, id: u64) -> Result<(), CleanupFailure> {
        let empty = self
            .state
            .borrow()
            .owned
            .get(&id)
            .is_none_or(|owned| owned.names.is_empty());
        if empty {
            return Ok(());
        }
        let pat = self.token(false)?;
        self.cleanup_with_pat(id, &pat).await
    }

    /// response-unknown deletion keeps the exact name and ID until a later read settles it.
    async fn cleanup_with_pat(&self, id: u64, pat: &Secret) -> Result<(), CleanupFailure> {
        let names = self
            .state
            .borrow()
            .owned
            .get(&id)
            .map(|owned| owned.names.clone())
            .unwrap_or_default();
        for (name, runner_id) in names {
            let runner_id = if let Some(runner_id) = runner_id {
                Some(runner_id)
            } else {
                let runner = self
                    .client
                    .find_runner(&self.pool.owner, &self.pool.repository, &name, pat)
                    .await?;
                runner.map(|runner| runner.id)
            };
            if let Some(runner_id) = runner_id {
                if let Some(owned) = self.state.borrow_mut().owned.get_mut(&id) {
                    owned.names.insert(name.clone(), Some(runner_id));
                }
                self.client
                    .delete_runner(
                        &self.pool.owner,
                        &self.pool.repository,
                        runner_id,
                        &name,
                        pat,
                    )
                    .await?;
            }
            if let Some(owned) = self.state.borrow_mut().owned.get_mut(&id) {
                owned.names.remove(&name);
            }
        }
        if let Some(owned) = self.state.borrow_mut().owned.get_mut(&id) {
            owned.incomplete = false;
        }
        Ok(())
    }

    fn cleanup_failed(&self, id: u64, reason: &str) {
        if let Some(owned) = self.state.borrow_mut().owned.get_mut(&id) {
            owned.incomplete = true;
        }
        let identities = self
            .state
            .borrow()
            .owned
            .get(&id)
            .map(|owned| {
                owned
                    .names
                    .iter()
                    .map(|(name, id)| format!("{name} ({id:?})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        self.log.warn(&format!(
            "pool {}: incomplete registration cleanup: {identities}: {reason}",
            self.pool.logical_name
        ));
        self.publish();
    }
}
