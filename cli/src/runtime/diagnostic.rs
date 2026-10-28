//! finalising diagnostic containers, export then removal, on one clock, as
//! `docs/operations/cli.md` describes; each warning names only the pool, the
//! container and the gap, never evidence.

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use tokio::time::Instant;

use super::backend::{
    Backend, Deadlines, DiagnosticRecord, ExportOutcome, Finalized, REMOVAL_WITHIN, StopRequest,
};
use super::log::Log;
use super::retained::Claim;

/// the grace one `docker stop` gives the diagnostic containers, Docker's default.
pub const STOP_GRACE: Duration = Duration::from_secs(10);

/// from the clock start to the end of that stop.
pub const STOP_WITHIN: Duration = Duration::from_secs(12);

/// the bundles a pool's sink may hold, partial ones included.
pub const BUNDLE_QUOTA: u32 = 10;

/// the PowerShell supervisor's text for each export that has a gap, and `None` for
/// one without.
#[must_use]
pub fn gap_text(outcome: ExportOutcome) -> Option<&'static str> {
    match outcome {
        ExportOutcome::Complete | ExportOutcome::AlreadyExported => None,
        ExportOutcome::Gap => {
            Some("final observation unavailable (abrupt exit or observation limit).")
        }
        ExportOutcome::SinkFull => {
            Some("private sink full (ten bundles); prior evidence retained.")
        }
        ExportOutcome::Failed => Some(
            "private export unavailable, incomplete, unsafe or over budget; prior evidence retained.",
        ),
        ExportOutcome::Skipped => Some("finalization budget spent while waiting; export skipped."),
    }
}

fn warn_gap(log: &dyn Log, record: &DiagnosticRecord, name: &str, text: &str) {
    log.warn(&format!(
        "Diagnostic gap in pool {}, container {name}: {text}",
        record.pool
    ));
}

/// logs what a finalisation did that needs the operator's eye.
pub fn report(log: &dyn Log, record: &DiagnosticRecord, name: &str, finalized: &Finalized) {
    if finalized.kept {
        log.warn(&format!(
            "Diagnostic container {name} of pool {} is ended and kept without its export; \
             the next start exports and removes it.",
            record.pool
        ));
        return;
    }
    if let Some(text) = gap_text(finalized.export) {
        warn_gap(log, record, name, text);
    }
    if !finalized.removed {
        log.warn(&format!(
            "Removing diagnostic container {name} of pool {} failed or ran over budget; \
             it stays retained, and the next finalisation removes it.",
            record.pool
        ));
    }
}

/// exports a claimed container unless a bundle was already admitted, then removes
/// it whatever the export did, by the deadlines; a forced stop, asked for through
/// [`Retained::force`](super::retained::Retained::force), ends the export and keeps
/// the container.
pub async fn finalize<B: Backend>(
    backend: &B,
    claim: &Claim<'_>,
    deadlines: Deadlines,
) -> Finalized {
    let mut forced = backend.retained().forced();
    let mut finalized = Finalized {
        export: ExportOutcome::AlreadyExported,
        removed: false,
        kept: false,
        bundles: None,
    };
    if !claim.exported() {
        if Instant::now() >= deadlines.export {
            finalized.export = ExportOutcome::Skipped;
        } else {
            let exported = tokio::select! {
                biased;
                _ = forced.wait_for(|forced| *forced) => None,
                exported = backend.export_diagnostic(
                    claim.name(),
                    claim.record(),
                    deadlines.export,
                ) => Some(exported),
            };
            let Some(exported) = exported else {
                finalized.kept = true;
                return finalized;
            };
            if exported.admitted {
                claim.mark_exported();
            }
            finalized.export = exported.outcome;
            finalized.bundles = exported.bundles;
        }
    }
    if *forced.borrow() {
        finalized.kept = true;
        return finalized;
    }
    let budget = deadlines.removal.saturating_duration_since(Instant::now());
    if backend
        .remove_containers(&[claim.name().to_string()], budget)
        .await
        .is_ok()
    {
        claim.removed();
        finalized.removed = true;
    }
    finalized
}

/// finalises a container whose job ended at `ended`, unless another path holds it.
pub async fn finalize_at_job_end<B: Backend>(
    backend: &B,
    log: &dyn Log,
    name: &str,
    ended: Instant,
) {
    let Some(claim) = backend.retained().claim(name) else {
        return;
    };
    let finalized = finalize(backend, &claim, Deadlines::from(ended)).await;
    report(log, claim.record(), name, &finalized);
}

/// polls every future until all have ended, and returns their outputs in order.
pub async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut pending: Vec<Option<Pin<Box<F>>>> = futures
        .into_iter()
        .map(|future| Some(Box::pin(future)))
        .collect();
    let mut outputs: Vec<Option<F::Output>> = pending.iter().map(|_| None).collect();
    std::future::poll_fn(|context| {
        let mut waiting = false;
        for (slot, output) in pending.iter_mut().zip(outputs.iter_mut()) {
            if let Some(future) = slot {
                match future.as_mut().poll(context) {
                    Poll::Ready(value) => {
                        *output = Some(value);
                        *slot = None;
                    }
                    Poll::Pending => waiting = true,
                }
            }
        }
        if waiting {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    outputs.into_iter().flatten().collect()
}

/// one container a run of [`finalize_claimed`] acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acted {
    pub name: String,
    pub record: DiagnosticRecord,
    pub finalized: Finalized,
}

/// stops the claimed containers in one call ended at 12 s, then finalises each on
/// the clock that started at `start`, logging what needs the operator's eye; a
/// failed stop is a gap, and the export and removal are still attempted.
pub async fn finalize_claimed<B: Backend>(
    backend: &B,
    log: &dyn Log,
    claims: Vec<Claim<'_>>,
    start: Instant,
) -> Vec<Acted> {
    if claims.is_empty() {
        return Vec::new();
    }
    let names: Vec<String> = claims
        .iter()
        .map(|claim| claim.name().to_string())
        .collect();
    let budget = (start + STOP_WITHIN).saturating_duration_since(Instant::now());
    let stopped = backend
        .stop_containers(&StopRequest {
            names,
            grace: STOP_GRACE,
            budget,
        })
        .await;
    if stopped.is_err() {
        for claim in &claims {
            warn_gap(
                log,
                claim.record(),
                claim.name(),
                "stop unavailable or over budget; export and removal will still be attempted.",
            );
        }
    }
    let deadlines = Deadlines::from(start);
    let finalized = join_all(
        claims
            .iter()
            .map(|claim| finalize(backend, claim, deadlines))
            .collect(),
    )
    .await;
    claims
        .iter()
        .zip(finalized)
        .map(|(claim, finalized)| {
            report(log, claim.record(), claim.name(), &finalized);
            Acted {
                name: claim.name().to_string(),
                record: claim.record().clone(),
                finalized,
            }
        })
        .collect()
}

/// waits until no other path holds the container, then finalises it on the clock
/// that started at `start` if it is still retained.
async fn finalize_once_released<B: Backend>(
    backend: &B,
    log: &dyn Log,
    name: &str,
    start: Instant,
) {
    let retained = backend.retained();
    retained.released(&[name.to_string()]).await;
    if let Some(claim) = retained.claim(name) {
        finalize_claimed(backend, log, vec![claim], start).await;
    }
}

/// the diagnostic column of the stop sequence's container steps, on the clock that
/// started at `start`; a forced stop, at any point, ends every retained container
/// with `docker kill` within `kill_budget` and keeps it.
pub async fn container_steps<B: Backend>(
    backend: &B,
    log: &dyn Log,
    start: Instant,
    kill_budget: Duration,
) {
    let retained = backend.retained();
    let mut forced = retained.forced();
    let steps = async {
        let (claims, held) = retained.claim_free();
        let finalizing = finalize_claimed(backend, log, claims, start);
        let waiting = join_all(
            held.iter()
                .map(|name| finalize_once_released(backend, log, name, start))
                .collect(),
        );
        tokio::join!(finalizing, waiting);
    };
    let interrupted = tokio::select! {
        biased;
        _ = forced.wait_for(|forced| *forced) => true,
        () = steps => false,
    };
    if !interrupted && !*forced.borrow() {
        return;
    }
    let names = retained.names();
    if names.is_empty() {
        return;
    }
    for name in &names {
        log.warn(&format!(
            "Forced stop: diagnostic container {name} is ended with docker kill and kept, \
             without its export."
        ));
    }
    // a container the forced stop finds already ended answers "not running", which
    // changes nothing, so the error is only logged
    if let Err(error) = backend.kill_containers(&names, kill_budget).await {
        log.warn(&format!("Ending the diagnostic containers failed: {error}"));
    }
}

/// the per-pool line of a `diagnostics` answer, and the remaining count it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolCount {
    pub pool: String,
    pub written: u32,
    /// the pool's bundles in its sink, or `None` when they could not be counted.
    pub bundles: Option<u32>,
}

impl PoolCount {
    /// `pool <name>: <k> bundles written, <r> of 10 remaining`, without `pool `.
    #[must_use]
    pub fn line(&self) -> String {
        let remaining = self.bundles.map_or_else(
            || "unknown".to_string(),
            |bundles| BUNDLE_QUOTA.saturating_sub(bundles).to_string(),
        );
        format!(
            "{}: {} bundles written, {remaining} of {BUNDLE_QUOTA} remaining",
            self.pool, self.written
        )
    }
}

/// what a `diagnostics` request did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flushed {
    pub pools: Vec<PoolCount>,
    /// one line per container whose export had a gap, whose removal failed, or
    /// which was left to the manual recovery.
    pub containers: Vec<String>,
    /// every container acted on was removed, and none was left to the manual
    /// recovery.
    pub complete: bool,
    /// there was nothing to act on and no pool has diagnostics on.
    pub nothing: bool,
}

/// finalises every retained container nobody holds whose job has ended, on one 24 s
/// clock, and counts the bundles of their pools and of the pools in `enabled`; a
/// pool with only a container left to the manual recovery gets a line without a
/// count.
pub async fn flush<B: Backend>(
    backend: &B,
    log: &dyn Log,
    enabled: &[DiagnosticRecord],
) -> Flushed {
    let start = Instant::now();
    let retained = backend.retained();
    let acted = finalize_claimed(backend, log, retained.claim_ended(), start).await;
    let unmanaged = retained.unmanaged();
    if acted.is_empty() && unmanaged.is_empty() && enabled.is_empty() {
        return Flushed {
            complete: true,
            nothing: true,
            ..Flushed::default()
        };
    }
    let mut containers = Vec::new();
    for acted in &acted {
        if let Some(text) = gap_text(acted.finalized.export) {
            containers.push(format!("{}: export gap: {text}", acted.name));
        }
        if !acted.finalized.removed {
            containers.push(format!("{}: removal failed; it stays retained", acted.name));
        }
    }
    for (pool, name) in &unmanaged {
        containers.push(format!(
            "{name}: left to the manual recovery, because pool {pool} has diagnostics off"
        ));
    }
    struct PoolSink<'a> {
        count: PoolCount,
        // a pool left to the manual recovery has no record, so no sink to count
        record: Option<&'a DiagnosticRecord>,
    }
    let mut pools: Vec<PoolSink> = Vec::new();
    let known = acted
        .iter()
        .map(|acted| (&acted.record.pool, Some(&acted.record)))
        .chain(enabled.iter().map(|record| (&record.pool, Some(record))))
        .chain(unmanaged.iter().map(|(pool, _)| (pool, None)));
    for (pool, record) in known {
        if !pools.iter().any(|sink| sink.count.pool == *pool) {
            pools.push(PoolSink {
                count: PoolCount {
                    pool: pool.clone(),
                    written: 0,
                    bundles: None,
                },
                record,
            });
        }
    }
    for acted in &acted {
        if let Some(count) = pools
            .iter_mut()
            .map(|sink| &mut sink.count)
            .find(|count| count.pool == acted.record.pool)
        {
            if matches!(
                acted.finalized.export,
                ExportOutcome::Complete | ExportOutcome::Gap
            ) {
                count.written += 1;
            }
            if acted.finalized.bundles.is_some() {
                count.bundles = acted.finalized.bundles;
            }
        }
    }
    let deadline = start + REMOVAL_WITHIN;
    let counts = join_all(
        pools
            .iter()
            .map(|sink| async move {
                match (sink.count.bundles, sink.record) {
                    (Some(bundles), _) => Some(bundles),
                    (None, Some(record)) => backend.count_bundles(record, deadline).await,
                    (None, None) => None,
                }
            })
            .collect(),
    )
    .await;
    Flushed {
        pools: pools
            .into_iter()
            .zip(counts)
            .map(|(sink, bundles)| PoolCount {
                bundles,
                ..sink.count
            })
            .collect(),
        complete: acted.iter().all(|acted| acted.finalized.removed) && unmanaged.is_empty(),
        containers,
        nothing: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stop_has_ten_seconds_of_grace_and_ends_at_twelve() {
        assert_eq!(STOP_GRACE, Duration::from_secs(10));
        assert_eq!(STOP_WITHIN, Duration::from_secs(12));
        assert_eq!(BUNDLE_QUOTA, 10);
    }

    #[test]
    fn a_pool_line_shows_the_remaining_quota_or_unknown() {
        let mut count = PoolCount {
            pool: "d".to_string(),
            written: 1,
            bundles: Some(3),
        };
        assert_eq!(count.line(), "d: 1 bundles written, 7 of 10 remaining");
        count.bundles = None;
        assert_eq!(
            count.line(),
            "d: 1 bundles written, unknown of 10 remaining"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn join_all_keeps_the_order_of_its_futures() {
        let outputs = join_all(vec![
            Box::pin(async {
                tokio::task::yield_now().await;
                1
            }) as Pin<Box<dyn Future<Output = i32>>>,
            Box::pin(async { 2 }),
        ])
        .await;
        assert_eq!(outputs, [1, 2]);
    }
}
