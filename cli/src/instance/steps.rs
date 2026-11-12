//! the container steps of a stop, shared by the foreground instance and the agent:
//! every ordinary container is stopped in one call and the rest removed in another,
//! beside the finalisation of the diagnostic containers; `docs/operations/cli.md`
//! has the times.

use std::cell::{Cell, RefCell};

use tokio::sync::watch;

use crate::runtime::backend::{Backend, StopRequest};
use crate::runtime::diagnostic::{self, StopClock};
use crate::runtime::log::Log;

use super::lifecycle::StopTimes;

/// what the container steps work with.
pub struct Steps<'a, B> {
    pub backend: &'a B,
    pub log: &'a dyn Log,
    pub times: StopTimes,
    /// set when a forced removal is asked for.
    pub force: &'a watch::Sender<bool>,
    /// cleared when a removal fails, and kept cleared.
    pub containers_removed: &'a Cell<bool>,
    /// where the names of containers whose removal failed are kept, when anyone
    /// shows them.
    pub left: Option<&'a RefCell<Vec<String>>>,
}

/// stops and removes `containers`, finalises the diagnostic containers on their own
/// clock, and then releases the hosts still prepared.
pub async fn container_steps<B: Backend>(steps: &Steps<'_, B>, containers: Vec<String>) {
    let start = tokio::time::Instant::now();
    tokio::join!(
        ordinary_steps(steps, containers),
        diagnostic::container_steps(
            steps.backend,
            steps.log,
            StopClock::at(start).shortened_by(steps.times.diagnostic_shortening()),
            steps.times.remove_budget,
        )
    );
    release_hosts(steps).await;
}

/// releases every job host still prepared, all at once, once the containers are
/// stopped: a worker releases its own host when its run ends, so what remains is the
/// host of a worker that is still busy or that a panic took.
async fn release_hosts<B: Backend>(steps: &Steps<'_, B>) {
    let budget = steps.times.release_budget;
    let releases: Vec<_> = steps
        .backend
        .prepared_hosts()
        .into_iter()
        .map(|host| async move {
            if let Err(error) = steps.backend.release_job_host(&host, budget).await {
                steps.log.warn(&format!(
                    "Releasing the job host of {} slot {} failed: {error}",
                    host.pool, host.slot
                ));
                steps.containers_removed.set(false);
            }
        })
        .collect();
    diagnostic::join_all(releases).await;
}

/// stops the containers in one call, and removes whatever remains in another; a
/// forced removal abandons the stop or skips it, and a failed removal is remembered
/// until the process ends.
async fn ordinary_steps<B: Backend>(steps: &Steps<'_, B>, containers: Vec<String>) {
    if containers.is_empty() {
        return;
    }
    let request = StopRequest {
        names: containers.clone(),
        grace: steps.times.grace,
        budget: steps.times.stop_budget,
    };
    let mut force = steps.force.subscribe();
    tokio::select! {
        biased;
        _ = force.wait_for(|forced| *forced) => {
            steps.log.warn("Forced removal: the grace period is skipped.");
        }
        stopped = steps.backend.stop_containers(&request) => {
            if let Err(error) = stopped {
                steps.log
                    .warn(&format!("Stopping the containers failed: {error}"));
            }
        }
    }
    if let Err(error) = steps
        .backend
        .remove_containers(&containers, steps.times.remove_budget)
        .await
    {
        steps
            .log
            .warn(&format!("Removing the containers failed: {error}"));
        steps.containers_removed.set(false);
        if let Some(left) = steps.left {
            left.borrow_mut().extend(containers);
        }
    }
}
