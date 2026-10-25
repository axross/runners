//! The runtime core `start` runs: it prepares the pools, checks their image once
//! Docker answers, and runs every slot's loop until the stop is asked for.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

use tokio::sync::watch;

use crate::image::check::{ImageCheck, freshness_warnings};
use crate::runtime::backend::Backend;
use crate::runtime::board::Board;
use crate::runtime::clock::Clock;
use crate::runtime::docker::DOCKER_POLL_INTERVAL;
use crate::runtime::image::check_image;
use crate::runtime::log::Log;
use crate::runtime::pool::{
    NoPoolStartable, PoolSpec, admit_pools, pools_from_plan, prepare_admitted,
};
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
    running: Cell<bool>,
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
            running: Cell::new(false),
        }
    }

    /// Admits the pools, waits for Docker, checks the image, and prepares the
    /// pools, in that order, so no pool is prepared for an image that cannot run.
    async fn prepare(
        &self,
        pools: Vec<PoolSpec>,
        image: &str,
    ) -> Result<Vec<PoolSpec>, CoreFailure> {
        let admitted = admit_pools(pools, self.log, &self.board);
        if admitted.is_empty() {
            return Err(failure(NoPoolStartable.to_string(), Ending::NothingToRun));
        }
        self.backend.wait_until_available().await;
        self.verify_image(image).await?;
        prepare_admitted(self.backend, admitted, self.log, &self.board)
            .await
            .map_err(|NoPoolStartable| failure(NoPoolStartable.to_string(), Ending::NothingToRun))
    }

    /// Checks the image until Docker answers the check, then logs its freshness
    /// warnings, or fails when it cannot run jobs.
    async fn verify_image(&self, image: &str) -> Result<(), CoreFailure> {
        loop {
            let check = check_image(self.backend, &self.board, image).await;
            if let Some(refusal) = check.refusal(image) {
                return Err(failure(refusal, Ending::ImageRefused));
            }
            let ImageCheck::Unavailable(reason) = check else {
                for warning in freshness_warnings(&check, self.clock.now()) {
                    self.log.warn(&format!("Image freshness: {warning}."));
                }
                return Ok(());
            };
            self.log.warn(&format!(
                "{reason}, so the image {image} is checked again in {} s.",
                DOCKER_POLL_INTERVAL.as_secs()
            ));
            self.clock.sleep(DOCKER_POLL_INTERVAL).await;
            self.backend.wait_until_available().await;
        }
    }
}

/// Empties the board when a run of the core ends, however it ends.
struct Cleared<'a>(&'a Board, &'a Cell<bool>);

impl Drop for Cleared<'_> {
    fn drop(&mut self) {
        self.0.clear();
        self.1.set(false);
    }
}

/// How a failed run ends, besides its message.
enum Ending {
    Failed,
    NothingToRun,
    ImageRefused,
}

fn failure(message: String, ending: Ending) -> CoreFailure {
    CoreFailure {
        message,
        containers: Vec::new(),
        nothing_to_run: matches!(ending, Ending::NothingToRun),
        image_refused: matches!(ending, Ending::ImageRefused),
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
        self.running.set(true);
        let _cleared = Cleared(&self.board, &self.running);
        let pools =
            pools_from_plan(plan).map_err(|error| failure(error.to_string(), Ending::Failed))?;
        let image = plan.image_name.as_deref().unwrap_or_default();
        self.board.expect_image(image);
        let prepared = tokio::select! {
            prepared = self.prepare(pools, image) => prepared?,
            () = wait_for_stop(&mut stop) => return Ok(()),
        };
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

    async fn check_image(&self, plan: &Plan) -> Result<(), String> {
        let Some(image) = plan.image_name.as_deref() else {
            return Ok(());
        };
        let check = check_image(self.backend, &self.board, image).await;
        if let ImageCheck::Unavailable(reason) = &check {
            self.log.warn(&format!(
                "{reason}, so the image {image} is checked once Docker answers."
            ));
        }
        check.refusal(image).map_or(Ok(()), Err)
    }

    fn report(&self) -> Vec<(&'static str, String)> {
        if !self.running.get() {
            return Vec::new();
        }
        let mut fields = self.board.fields();
        fields.extend(self.board.image_fields(self.clock.now()));
        fields
    }
}
