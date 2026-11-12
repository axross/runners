//! the snapshot, built from what the agent already keeps: its state, its board, the
//! retained record and the token cache; it reads nothing from Docker or GitHub and
//! never holds a token or a JIT configuration.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::host_config::{FileStatus, Loaded, Migration, MigrationFailure, Origin};
use crate::image::check::{Freshness, freshness_findings, status_fields};
use crate::runtime::backend::ExportOutcome;
use crate::runtime::board::{Board, PoolState, Row, SlotState};
use crate::runtime::diagnostic::BUNDLE_QUOTA;
use crate::runtime::retained::Retained;
use crate::token_store::RepositoryKey;
use crate::validate::Entry;

use super::snapshot::{
    AgentInfo, ConfigurationInfo, DiagnosticContainer, DiagnosticFinding, DiagnosticsInfo,
    ImageInfo, ImageLine, ImageWarning, InUseInfo, Leftover, MigrationInfo, PoolInfo,
    RepositoryInfo, RunnerInfo, Snapshot, UpdateInfo,
};
use super::state::{Inner, Phase};
use super::tokens::StoreTokens;

type RowsByName<'a, T> = BTreeMap<&'a str, &'a Row<T>>;

fn by_name<T>(rows: &[Row<T>]) -> RowsByName<'_, T> {
    rows.iter().map(|row| (row.name.as_str(), row)).collect()
}

/// everything a snapshot is read from.
pub struct View<'a> {
    pub version: &'a str,
    pub inner: &'a Inner,
    pub config_path: &'a str,
    pub board: &'a Board,
    pub retained: &'a Retained,
    pub tokens: &'a StoreTokens,
    /// the containers a stop could not remove.
    pub left: &'a [String],
    pub now: SystemTime,
}

#[must_use]
pub fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    })
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn export_word(outcome: ExportOutcome) -> &'static str {
    match outcome {
        ExportOutcome::Complete => "complete",
        ExportOutcome::Gap => "gap",
        ExportOutcome::SinkFull => "sink_full",
        ExportOutcome::Failed => "failed",
        ExportOutcome::Skipped => "skipped",
        ExportOutcome::AlreadyExported => "already_exported",
    }
}

fn warning(finding: &Freshness) -> ImageWarning {
    match finding {
        Freshness::NotBuiltByMarshal => ImageWarning {
            code: "not_built_by_marshal".to_string(),
            ..ImageWarning::default()
        },
        Freshness::PackagesOld { days } => ImageWarning {
            code: "packages_old".to_string(),
            days: Some(*days),
            version: None,
        },
        Freshness::ImageOld { days } => ImageWarning {
            code: "image_old".to_string(),
            days: Some(*days),
            version: None,
        },
        Freshness::RunnerOld { version, days } => ImageWarning {
            code: "runner_old".to_string(),
            days: Some(*days),
            version: Some(version.clone()),
        },
        Freshness::RunnerMayBeNewer { version, days } => ImageWarning {
            code: "runner_may_be_newer".to_string(),
            days: Some(*days),
            version: Some(version.clone()),
        },
    }
}

/// the configuration part of the snapshot: the path, the problems, and what the
/// latest load found.
fn configuration(path: &str, inner: &Inner) -> ConfigurationInfo {
    let mut info = ConfigurationInfo {
        path: path.to_string(),
        problems: inner.problems().to_vec(),
        ..ConfigurationInfo::default()
    };
    let Some(loaded) = &inner.loaded else {
        return info;
    };
    info.status = file_status_word(loaded.file).to_string();
    info.in_use = loaded.in_use.as_ref().map(|in_use| InUseInfo {
        source: match in_use.origin {
            Origin::File => "file",
            Origin::LastValid => "last_valid",
            Origin::Json => "json",
        }
        .to_string(),
        read_at: millis(in_use.read_at),
    });
    info.migration = migration(loaded);
    info
}

fn file_status_word(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Valid => "valid",
        FileStatus::Invalid => "invalid",
        FileStatus::Missing => "missing",
        FileStatus::Unreadable => "unreadable",
    }
}

fn migration(loaded: &Loaded) -> Option<MigrationInfo> {
    let (state, at, reason) = match &loaded.migration {
        Migration::None => return None,
        Migration::Migrated { at } => ("migrated", Some(millis(*at)), None),
        Migration::Pending => ("pending", None, None),
        Migration::Failed(failure) => ("failed", None, Some(migration_failure_text(failure))),
    };
    Some(MigrationInfo {
        state: state.to_string(),
        at,
        reason,
    })
}

/// why a migration failed, in the words the window shows.
#[must_use]
pub fn migration_failure_text(failure: &MigrationFailure) -> String {
    match failure {
        MigrationFailure::JsonRejected(_) => {
            "config.json is not valid, so it was not migrated".to_string()
        }
        MigrationFailure::CheckFailed(why) => {
            format!("config.toml was not written: the converted file failed its check: {why}")
        }
        MigrationFailure::WriteFailed(why) => format!("config.toml was not written: {why}"),
    }
}

impl View<'_> {
    /// the snapshot, not yet fitted to the message limit.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner;
        let reason = match &inner.phase {
            Phase::Stopped(reason) => Some(reason.wire()),
            _ => None,
        };
        let mut snapshot = Snapshot {
            agent: AgentInfo {
                version: self.version.to_string(),
                state: inner.phase.name().to_string(),
                reason,
                since: millis(inner.since),
                build: inner.build.clone(),
                restart_due: inner.restart_due.map(millis),
                update: inner.drain.as_ref().map(|drain| UpdateInfo {
                    version: drain.version.clone(),
                    waiting: drain.waiting,
                    deadline: millis(drain.deadline),
                    pid: std::process::id(),
                }),
            },
            configuration: configuration(self.config_path, inner),
            leftovers: self
                .left
                .iter()
                .map(|name| Leftover {
                    kind: "container".to_string(),
                    name: name.clone(),
                })
                .collect(),
            images: self.image().into_iter().collect(),
            ..Snapshot::default()
        };
        if let Some(plan) = inner.plan() {
            snapshot.repositories = self.repositories(&plan.entries);
            if inner.restart_due.is_none() {
                let pool_rows = self.board.pool_rows();
                let slot_rows = self.board.slot_rows();
                let (pools, slots) = (by_name(&pool_rows), by_name(&slot_rows));
                snapshot.pools = plan
                    .entries
                    .iter()
                    .map(|entry| self.pool(entry, &pools, &slots))
                    .collect();
            }
        }
        snapshot.runners_up = snapshot.pools.iter().map(|pool| pool.runners_up).sum();
        snapshot.runners_wanted = snapshot.pools.iter().map(|pool| pool.slots).sum();
        snapshot
    }

    fn repositories(&self, entries: &[Entry]) -> Vec<RepositoryInfo> {
        let mut by_key: BTreeMap<RepositoryKey, Vec<String>> = BTreeMap::new();
        for entry in entries {
            by_key
                .entry(RepositoryKey::of(&entry.owner, &entry.repository))
                .or_default()
                .push(entry.name.clone());
        }
        by_key
            .into_iter()
            .map(|(key, pools)| RepositoryInfo {
                has_token: self.tokens.has_token(&key),
                repository: key.to_string(),
                pools,
            })
            .collect()
    }

    fn pool(
        &self,
        entry: &Entry,
        pools: &RowsByName<'_, PoolState>,
        slots: &RowsByName<'_, SlotState>,
    ) -> PoolInfo {
        let key = RepositoryKey::of(&entry.owner, &entry.repository);
        let (state, reason) = match pools.get(entry.name.as_str()).map(|row| &row.state) {
            Some(PoolState::WaitingForEngine(_)) => ("waiting_for_docker", None),
            Some(PoolState::Preparing) => ("preparing", None),
            Some(PoolState::Running) => ("running", None),
            Some(PoolState::Skipped(reason)) => ("skipped", Some(reason.clone())),
            None => ("stopped", self.inner.skips.get(&entry.name).cloned()),
        };
        let runners: Vec<RunnerInfo> = (1..=entry.slots)
            .map(|number| {
                let label = format!("{} slot {number}", entry.name);
                match slots.get(label.as_str()) {
                    Some(row) => {
                        let (state, workload, reason) = match &row.state {
                            SlotState::Registering => ("registering", None, None),
                            SlotState::BackingOff(why) => ("backing_off", None, Some(why.clone())),
                            SlotState::Held(why) => ("held", None, Some(why.clone())),
                            SlotState::Running(name) => ("running", Some(name.clone()), None),
                            SlotState::Drained => {
                                ("stopped", None, Some("stopped for an update".to_string()))
                            }
                        };
                        RunnerInfo {
                            slot: number,
                            state: state.to_string(),
                            workload,
                            reason,
                            since: millis(row.since),
                        }
                    }
                    None => RunnerInfo {
                        slot: number,
                        state: "stopped".to_string(),
                        workload: None,
                        reason: None,
                        since: millis(self.inner.since),
                    },
                }
            })
            .collect();
        PoolInfo {
            name: entry.name.clone(),
            repository: key.to_string(),
            state: state.to_string(),
            reason,
            has_token: self.tokens.has_token(&key),
            runners_up: count(
                runners
                    .iter()
                    .filter(|runner| runner.state == "running")
                    .count(),
            ),
            slots: entry.slots,
            runners,
            diagnostics: self.diagnostics(entry),
        }
    }

    fn diagnostics(&self, entry: &Entry) -> Option<DiagnosticsInfo> {
        let manual: Vec<String> = self
            .retained
            .unmanaged()
            .into_iter()
            .filter(|(pool, _)| *pool == entry.name)
            .map(|(_, name)| name)
            .collect();
        if !entry.diagnostics && manual.is_empty() {
            return None;
        }
        let ledger = self.retained.ledger(&entry.name);
        Some(DiagnosticsInfo {
            limit: BUNDLE_QUOTA,
            written: ledger.written,
            in_sink: ledger.bundles,
            containers: self
                .retained
                .views()
                .into_iter()
                .filter(|view| view.pool == entry.name)
                .map(|view| DiagnosticContainer {
                    state: if view.finalising {
                        "finalising"
                    } else if view.running {
                        "running"
                    } else {
                        "retained"
                    }
                    .to_string(),
                    bundle: view.bundle_admitted,
                    name: view.name,
                })
                .collect(),
            findings: self
                .retained
                .findings()
                .into_iter()
                .filter(|(_, finding)| finding.pool == entry.name)
                .map(|(container, finding)| DiagnosticFinding {
                    container,
                    export: export_word(finding.export).to_string(),
                    removed: finding.removed,
                    kept: finding.kept,
                })
                .collect(),
            manual_recovery: manual,
        })
    }

    fn image(&self) -> Option<ImageInfo> {
        let (name, check) = self.inner.image.as_ref()?;
        let lines = status_fields(name, check.as_ref(), self.now)
            .into_iter()
            .filter(|(field, _)| *field != "warning")
            .map(|(name, value)| ImageLine {
                name: name.to_string(),
                value,
            })
            .collect();
        let warnings = check
            .as_ref()
            .map(|check| {
                freshness_findings(check, self.now)
                    .iter()
                    .map(warning)
                    .collect()
            })
            .unwrap_or_default();
        Some(ImageInfo {
            name: name.clone(),
            lines,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use crate::validate::Plan;

    fn entry() -> Entry {
        Entry {
            path: String::new(),
            index: 0,
            owner: "owner".to_string(),
            repository: "repository".to_string(),
            name: "pool".to_string(),
            slots: 2,
            token_path: None,
            labels: Vec::new(),
            cpus: 1.0,
            memory_gb: 8,
            volumes: Vec::new(),
            diagnostics: false,
            diagnostic_directory: None,
            diagnostic_raw_records: false,
            os: None,
            disk_gb: None,
        }
    }

    fn runners(board: &Board) -> Vec<RunnerInfo> {
        let mut inner = Inner::new(Phase::Running);
        inner.loaded = Some(Loaded {
            file: FileStatus::Valid,
            in_use: Some(crate::host_config::InUse {
                origin: Origin::File,
                plan: Plan {
                    profile: Profile::Axmc,
                    image_name: None,
                    entries: vec![entry()],
                    budget: None,
                    settings: None,
                },
                read_at: SystemTime::now(),
                checkout_rule_skipped: false,
            }),
            migration: Migration::None,
            problems: Vec::new(),
        });
        let retained = Retained::default();
        let tokens = StoreTokens::new(None);
        let snapshot = View {
            version: "test",
            inner: &inner,
            config_path: "config.toml",
            board,
            retained: &retained,
            tokens: &tokens,
            left: &[],
            now: SystemTime::now(),
        }
        .snapshot();
        snapshot.pools.into_iter().next().unwrap().runners
    }

    #[test]
    fn a_held_slot_is_the_runner_state_held_with_its_reason() {
        let board = Board::default();
        board.set_slot("pool slot 1", SlotState::Held("no disk".to_string()));
        board.set_slot("pool slot 2", SlotState::BackingOff("failed".to_string()));
        let runners = runners(&board);
        assert_eq!(runners[0].state, "held");
        assert_eq!(runners[0].reason.as_deref(), Some("no disk"));
        assert_eq!(runners[0].workload, None);
        assert_eq!(runners[1].state, "backing_off");
        assert_eq!(runners[1].reason.as_deref(), Some("failed"));
    }
}
