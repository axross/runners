//! The diagnostic containers a backend retains, and who is finalising each.
//!
//! A diagnostic container is retained from its start until its removal succeeds,
//! with the settings it ran with. At most one path finalises it at a time: a path
//! claims it first, and the claim ends however that path ends, a panic included,
//! which leaves the container retained and free for the next path. A backend owns
//! one of these and both its own calls and the runtime above it use it.

use std::cell::RefCell;
use std::collections::BTreeMap;

use tokio::sync::{Notify, watch};

use super::backend::DiagnosticRecord;

/// What is known about one retained container.
#[derive(Debug, Clone)]
struct Entry {
    record: DiagnosticRecord,
    /// A bundle was admitted for it, so no later finalisation exports it again.
    exported: bool,
    /// A path is finalising it.
    owned: bool,
    /// Its `docker run` is still waited for, so its job may still run.
    active: bool,
}

#[derive(Debug)]
pub struct Retained {
    entries: RefCell<BTreeMap<String, Entry>>,
    released: Notify,
    forced: watch::Sender<bool>,
    /// The labelled containers of pools whose diagnostics are off, by pool, which
    /// have no record and are left to the manual recovery.
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

/// Marks a container's job as ended when its run ends, however it ends.
pub struct JobRun<'a> {
    retained: &'a Retained,
    name: String,
}

impl Drop for JobRun<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.retained.entries.borrow_mut().get_mut(&self.name) {
            entry.active = false;
        }
    }
}

/// The right to finalise one retained container, given up when dropped.
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

    /// The settings the container ran with.
    #[must_use]
    pub fn record(&self) -> &DiagnosticRecord {
        &self.record
    }

    /// Whether a bundle was already admitted for the container.
    #[must_use]
    pub fn exported(&self) -> bool {
        self.retained
            .entries
            .borrow()
            .get(&self.name)
            .is_some_and(|entry| entry.exported)
    }

    pub fn mark_exported(&self) {
        if let Some(entry) = self.retained.entries.borrow_mut().get_mut(&self.name) {
            entry.exported = true;
        }
    }

    /// Forgets the container, once its removal has succeeded.
    pub fn removed(&self) {
        self.retained.entries.borrow_mut().remove(&self.name);
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.retained.entries.borrow_mut().get_mut(&self.name) {
            entry.owned = false;
        }
        self.retained.released.notify_waiters();
    }
}

impl Retained {
    /// Retains a container whose run starts now, until the returned guard drops.
    pub fn started(&self, name: &str, record: &DiagnosticRecord) -> JobRun<'_> {
        self.entries.borrow_mut().insert(
            name.to_string(),
            Entry {
                record: record.clone(),
                exported: false,
                owned: false,
                active: true,
            },
        );
        JobRun {
            retained: self,
            name: name.to_string(),
        }
    }

    /// Retains a container an earlier process left, with `record` as its settings.
    /// A container already retained keeps its own record.
    pub fn adopt(&self, name: &str, record: &DiagnosticRecord) {
        self.entries
            .borrow_mut()
            .entry(name.to_string())
            .or_insert_with(|| Entry {
                record: record.clone(),
                exported: false,
                owned: false,
                active: false,
            });
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.borrow().contains_key(name)
    }

    /// Every retained container, by name.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.entries.borrow().keys().cloned().collect()
    }

    /// The record of a retained container.
    #[must_use]
    pub fn record(&self, name: &str) -> Option<DiagnosticRecord> {
        self.entries
            .borrow()
            .get(name)
            .map(|entry| entry.record.clone())
    }

    /// Claims the container, unless it is not retained or another path holds it.
    #[must_use]
    pub fn claim(&self, name: &str) -> Option<Claim<'_>> {
        let mut entries = self.entries.borrow_mut();
        let entry = entries.get_mut(name).filter(|entry| !entry.owned)?;
        entry.owned = true;
        Some(Claim {
            retained: self,
            name: name.to_string(),
            record: entry.record.clone(),
        })
    }

    /// Claims every container nobody holds, and names those another path holds.
    #[must_use]
    pub fn claim_free(&self) -> (Vec<Claim<'_>>, Vec<String>) {
        let names = self.names();
        let held = names
            .iter()
            .filter(|name| {
                self.entries
                    .borrow()
                    .get(*name)
                    .is_some_and(|entry| entry.owned)
            })
            .cloned()
            .collect();
        let claims = names.iter().filter_map(|name| self.claim(name)).collect();
        (claims, held)
    }

    /// Claims every container nobody holds whose job has ended.
    #[must_use]
    pub fn claim_ended(&self) -> Vec<Claim<'_>> {
        let ended: Vec<String> = self
            .entries
            .borrow()
            .iter()
            .filter(|(_, entry)| !entry.owned && !entry.active)
            .map(|(name, _)| name.clone())
            .collect();
        ended.iter().filter_map(|name| self.claim(name)).collect()
    }

    /// Waits until no path holds any of `names`.
    pub async fn released(&self, names: &[String]) {
        loop {
            let notified = self.released.notified();
            let held = {
                let entries = self.entries.borrow();
                names
                    .iter()
                    .any(|name| entries.get(name).is_some_and(|entry| entry.owned))
            };
            if !held {
                return;
            }
            notified.await;
        }
    }

    /// Asks every finalisation to stop exporting and keep its container: a forced
    /// stop ends and keeps the diagnostic containers instead.
    pub fn force(&self) {
        self.forced.send_replace(true);
    }

    #[must_use]
    pub fn forced(&self) -> watch::Receiver<bool> {
        self.forced.subscribe()
    }

    /// Records the labelled containers a pool with diagnostics off left, replacing
    /// what an earlier preparation of that pool recorded.
    pub fn set_unmanaged(&self, pool: &str, names: Vec<String>) {
        let mut unmanaged = self.unmanaged.borrow_mut();
        if names.is_empty() {
            unmanaged.remove(pool);
        } else {
            unmanaged.insert(pool.to_string(), names);
        }
    }

    /// The labelled containers left to the manual recovery, as pool and name.
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
