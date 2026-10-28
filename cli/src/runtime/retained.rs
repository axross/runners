//! the diagnostic containers a backend retains from their start until a removal
//! succeeds, and the claim that lets one path at a time finalise each; a claim ends
//! however its path ends, a panic included, leaving the container retained and free.

use std::cell::RefCell;
use std::collections::BTreeMap;

use tokio::sync::{Notify, watch};

use super::backend::DiagnosticRecord;

#[derive(Debug, Clone)]
struct RetainedEntry {
    record: DiagnosticRecord,
    bundle_admitted: bool,
    finalising: bool,
    run_awaited: bool,
}

#[derive(Debug)]
pub struct Retained {
    entries: RefCell<BTreeMap<String, RetainedEntry>>,
    released: Notify,
    forced: watch::Sender<bool>,
    /// the labelled containers of pools with diagnostics off, by pool: no record,
    /// left to the manual recovery.
    unmanaged: RefCell<BTreeMap<String, Vec<String>>>,
}

impl Default for Retained {
    fn default() -> Self {
        Self {
            entries: RefCell::new(BTreeMap::new()),
            released: Notify::new(),
            forced: watch::Sender::new(false),
            unmanaged: RefCell::new(BTreeMap::new()),
        }
    }
}

/// marks a container's job as ended when its run ends, however it ends.
pub struct JobRun<'a> {
    retained: &'a Retained,
    name: String,
}

impl Drop for JobRun<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.retained.entries.borrow_mut().get_mut(&self.name) {
            entry.run_awaited = false;
        }
    }
}

/// the right to finalise one retained container, given up when dropped.
pub struct Claim<'a> {
    retained: &'a Retained,
    name: String,
    record: DiagnosticRecord,
}

impl Claim<'_> {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// the settings the container ran with.
    #[must_use]
    pub fn record(&self) -> &DiagnosticRecord {
        &self.record
    }

    /// whether a bundle was already admitted for the container.
    #[must_use]
    pub fn exported(&self) -> bool {
        self.retained
            .entries
            .borrow()
            .get(&self.name)
            .is_some_and(|entry| entry.bundle_admitted)
    }

    pub fn mark_exported(&self) {
        if let Some(entry) = self.retained.entries.borrow_mut().get_mut(&self.name) {
            entry.bundle_admitted = true;
        }
    }

    /// forgets the container, once its removal has succeeded.
    pub fn removed(&self) {
        self.retained.entries.borrow_mut().remove(&self.name);
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.retained.entries.borrow_mut().get_mut(&self.name) {
            entry.finalising = false;
        }
        self.retained.released.notify_waiters();
    }
}

impl Retained {
    /// retains a container whose run starts now, until the returned guard drops; one
    /// already retained keeps its claim and its export.
    pub fn started(&self, name: &str, record: &DiagnosticRecord) -> JobRun<'_> {
        self.entries
            .borrow_mut()
            .entry(name.to_string())
            .and_modify(|entry| {
                entry.record = record.clone();
                entry.run_awaited = true;
            })
            .or_insert_with(|| RetainedEntry {
                record: record.clone(),
                bundle_admitted: false,
                finalising: false,
                run_awaited: true,
            });
        JobRun {
            retained: self,
            name: name.to_string(),
        }
    }

    /// retains a container an earlier process left, with `record` as its settings;
    /// one already retained keeps its own record.
    pub fn adopt(&self, name: &str, record: &DiagnosticRecord) {
        self.entries
            .borrow_mut()
            .entry(name.to_string())
            .or_insert_with(|| RetainedEntry {
                record: record.clone(),
                bundle_admitted: false,
                finalising: false,
                run_awaited: false,
            });
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.borrow().contains_key(name)
    }

    /// every retained container, by name.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.entries.borrow().keys().cloned().collect()
    }

    /// the record of a retained container.
    #[must_use]
    pub fn record(&self, name: &str) -> Option<DiagnosticRecord> {
        self.entries
            .borrow()
            .get(name)
            .map(|entry| entry.record.clone())
    }

    /// claims the container, unless it is not retained or another path holds it.
    #[must_use]
    pub fn claim(&self, name: &str) -> Option<Claim<'_>> {
        let mut entries = self.entries.borrow_mut();
        let entry = entries.get_mut(name).filter(|entry| !entry.finalising)?;
        entry.finalising = true;
        Some(Claim {
            retained: self,
            name: name.to_string(),
            record: entry.record.clone(),
        })
    }

    /// claims every container nobody holds, and names those another path holds.
    #[must_use]
    pub fn claim_free(&self) -> (Vec<Claim<'_>>, Vec<String>) {
        let names = self.names();
        let held = names
            .iter()
            .filter(|name| {
                self.entries
                    .borrow()
                    .get(*name)
                    .is_some_and(|entry| entry.finalising)
            })
            .cloned()
            .collect();
        let claims = names.iter().filter_map(|name| self.claim(name)).collect();
        (claims, held)
    }

    /// claims every container nobody holds whose job has ended.
    #[must_use]
    pub fn claim_ended(&self) -> Vec<Claim<'_>> {
        let ended: Vec<String> = self
            .entries
            .borrow()
            .iter()
            .filter(|(_, entry)| !entry.finalising && !entry.run_awaited)
            .map(|(name, _)| name.clone())
            .collect();
        ended.iter().filter_map(|name| self.claim(name)).collect()
    }

    /// waits until no path holds any of `names`.
    pub async fn released(&self, names: &[String]) {
        loop {
            let notified = self.released.notified();
            let held = {
                let entries = self.entries.borrow();
                names
                    .iter()
                    .any(|name| entries.get(name).is_some_and(|entry| entry.finalising))
            };
            if !held {
                return;
            }
            notified.await;
        }
    }

    /// asks every finalisation to stop exporting and keep its container: a forced
    /// stop ends and keeps the diagnostic containers instead.
    pub fn force(&self) {
        self.forced.send_replace(true);
    }

    #[must_use]
    pub fn forced(&self) -> watch::Receiver<bool> {
        self.forced.subscribe()
    }

    /// records the labelled containers a pool with diagnostics off left, replacing
    /// what an earlier preparation recorded.
    pub fn set_unmanaged(&self, pool: &str, names: Vec<String>) {
        let mut unmanaged = self.unmanaged.borrow_mut();
        if names.is_empty() {
            unmanaged.remove(pool);
        } else {
            unmanaged.insert(pool.to_string(), names);
        }
    }

    /// forgets the labelled containers recorded for each pool `keep` refuses.
    pub fn retain_unmanaged(&self, keep: impl Fn(&str) -> bool) {
        self.unmanaged.borrow_mut().retain(|pool, _| keep(pool));
    }

    /// the labelled containers left to the manual recovery, as pool and name.
    #[must_use]
    pub fn unmanaged(&self) -> Vec<(String, String)> {
        self.unmanaged
            .borrow()
            .iter()
            .flat_map(|(pool, names)| names.iter().map(|name| (pool.clone(), name.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> DiagnosticRecord {
        DiagnosticRecord {
            pool: "d".to_string(),
            directory: "C:\\diag".to_string(),
            raw_records: false,
        }
    }

    #[test]
    fn one_path_holds_a_container_at_a_time_and_a_dropped_claim_frees_it() {
        let retained = Retained::default();
        let run = retained.started("d-1-x", &record());
        assert!(retained.claim_ended().is_empty(), "its job still runs");
        drop(run);
        let claim = retained.claim("d-1-x").unwrap();
        assert!(retained.claim("d-1-x").is_none());
        let (free, held) = retained.claim_free();
        assert!(free.is_empty());
        assert_eq!(held, ["d-1-x"]);
        drop(claim);
        assert!(retained.contains("d-1-x"), "the record outlives the claim");
        let claim = retained.claim("d-1-x").unwrap();
        claim.mark_exported();
        drop(claim);
        let claim = retained.claim("d-1-x").unwrap();
        assert!(claim.exported());
        claim.removed();
        drop(claim);
        assert!(retained.names().is_empty());
    }

    #[test]
    fn a_start_of_a_retained_container_keeps_its_claim_and_its_export() {
        let retained = Retained::default();
        retained.adopt("d-1-x", &record());
        let claim = retained.claim("d-1-x").unwrap();
        claim.mark_exported();
        let run = retained.started("d-1-x", &record());
        assert!(
            retained.claim("d-1-x").is_none(),
            "the claim still holds it"
        );
        assert!(claim.exported());
        drop((run, claim));
        assert!(retained.claim("d-1-x").unwrap().exported());
    }

    #[test]
    fn an_adopted_container_keeps_the_record_it_already_has() {
        let retained = Retained::default();
        drop(retained.started("d-1-x", &record()));
        let other = DiagnosticRecord {
            directory: "C:\\other".to_string(),
            ..record()
        };
        retained.adopt("d-1-x", &other);
        retained.adopt("d-1-y", &other);
        assert_eq!(retained.record("d-1-x"), Some(record()));
        assert_eq!(retained.record("d-1-y"), Some(other));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn waiting_for_a_release_ends_when_the_claim_drops() {
        let retained = Retained::default();
        retained.adopt("d-1-x", &record());
        let claim = retained.claim("d-1-x").unwrap();
        let names = ["d-1-x".to_string()];
        let wait = retained.released(&names);
        let release = async {
            tokio::task::yield_now().await;
            drop(claim);
        };
        tokio::join!(wait, release);
    }
}
