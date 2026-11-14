//! the snapshot, built from what the agent already keeps: its state, its board, the
//! retained record and the token cache; it reads nothing from Docker or GitHub and
//! never holds a token or a JIT configuration.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::host_config::{FileStatus, Loaded, Migration, MigrationFailure, Origin};
use crate::image::check::{Freshness, freshness_findings, status_fields};
use crate::image::macos_base::MACOS_BASE;
use crate::image::macos_golden::{self, MacosFreshness};
use crate::runtime::backend::{ExportOutcome, HostWait};
use crate::runtime::board::{Board, PoolState, Row, SlotState};
use crate::runtime::diagnostic::BUNDLE_QUOTA;
use crate::runtime::retained::Retained;
use crate::token_store::TokenKey;
use crate::validate::Entry;

use super::config::Change;
use super::core::LEAVING;
use super::snapshot::{
    AgentInfo, ConfigurationInfo, DiagnosticContainer, DiagnosticFinding, DiagnosticsInfo,
    ImageInfo, ImageLine, ImageWarning, InUseInfo, Leftover, MigrationInfo, PoolInfo,
    RepositoryInfo, RunnerInfo, Snapshot, UpdateInfo,
};
use super::state::{Inner, Phase};
use super::tokens::StoreTokens;

/// why a pool an apply restarts is preparing.
pub const RESTARTING: &str = "the edit of the configuration restarts it";

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
    info.last_apply.clone_from(&inner.last_apply);
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
                .chain(self.board.vm_leftovers().into_iter().map(|name| Leftover {
                    kind: "vm_unconfigured_pool".to_string(),
                    name,
                }))
                .collect(),
            images: self.image().into_iter().chain(self.macos_image()).collect(),
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
                // a pool an apply removes is listed until its slots have stopped, the
                // apply's end included; one the file names again is listed as that
                let removed = inner
                    .leaving
                    .iter()
                    .filter(|entry| !plan.entries.iter().any(|named| named.name == entry.name));
                for entry in removed {
                    let mut pool = self.pool(entry, &pools, &slots);
                    pool.runners
                        .retain(|runner| runner.reason.as_deref() == Some(LEAVING));
                    if !pool.runners.is_empty() {
                        pool.state = "stopped".to_string();
                        pool.reason = Some(LEAVING.to_string());
                        pool.slots = 0;
                        snapshot.pools.push(pool);
                    }
                }
            }
        }
        snapshot.runners_up = snapshot.pools.iter().map(|pool| pool.runners_up).sum();
        snapshot.runners_wanted = snapshot.pools.iter().map(|pool| pool.slots).sum();
        snapshot
    }

    fn repositories(&self, entries: &[Entry]) -> Vec<RepositoryInfo> {
        let mut by_key: BTreeMap<TokenKey, Vec<String>> = BTreeMap::new();
        for entry in entries {
            by_key
                .entry(TokenKey::of(&entry.owner, &entry.repository))
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
        let key = TokenKey::of(&entry.owner, &entry.repository);
        let restarting = self.inner.applying.as_ref().is_some_and(|applying| {
            applying
                .effects
                .iter()
                .any(|effect| effect.pool == entry.name && effect.change == Change::Restarted)
        });
        let (state, reason) = match pools.get(entry.name.as_str()).map(|row| &row.state) {
            Some(PoolState::WaitingForEngine(_)) => ("waiting_for_docker", None),
            Some(PoolState::Preparing | PoolState::Running) if restarting => {
                ("preparing", Some(RESTARTING.to_string()))
            }
            Some(PoolState::Preparing) => ("preparing", None),
            Some(PoolState::Running) => ("running", None),
            Some(PoolState::Skipped(reason)) => ("skipped", Some(reason.clone())),
            None => ("stopped", self.inner.skips.get(&entry.name).cloned()),
        };
        // a slot an apply stops is listed until its stop sequence ends, whichever of
        // the slots above the pool's count are still on the board
        let prefix = format!("{} slot ", entry.name);
        let mut above: Vec<u32> = slots
            .keys()
            .filter_map(|label| label.strip_prefix(prefix.as_str())?.parse().ok())
            .filter(|number| *number > entry.slots)
            .collect();
        above.sort_unstable();
        let runners: Vec<RunnerInfo> = (1..=entry.slots)
            .chain(above)
            .map(|number| {
                let label = format!("{} slot {number}", entry.name);
                match slots.get(label.as_str()) {
                    Some(row) => {
                        let (state, workload, reason) = match &row.state {
                            SlotState::Registering => ("registering", None, None),
                            SlotState::BackingOff(why) => ("backing_off", None, Some(why.clone())),
                            SlotState::Held(why) => ("held", None, Some(why.clone())),
                            SlotState::Waiting(HostWait::VmLimit) => {
                                ("waiting_for_vm_limit", None, Some(row.state.to_string()))
                            }
                            SlotState::Waiting(HostWait::VmDeletion(vm)) => (
                                "waiting_for_vm_deletion",
                                Some(vm.clone()),
                                Some(row.state.to_string()),
                            ),
                            SlotState::Running(name) => ("running", Some(name.clone()), None),
                            SlotState::RunningVm(vm) => ("running", Some(vm.clone()), None),
                            SlotState::Drained => {
                                ("stopped", None, Some("stopped for an update".to_string()))
                            }
                            SlotState::Leaving(why) => ("stopped", None, Some(why.clone())),
                        };
                        RunnerInfo {
                            slot: number,
                            state: state.to_string(),
                            workload_kind: matches!(
                                row.state,
                                SlotState::RunningVm(_)
                                    | SlotState::Waiting(HostWait::VmDeletion(_))
                            )
                            .then(|| "vm".to_string()),
                            workload,
                            reason,
                            since: millis(row.since),
                        }
                    }
                    None => RunnerInfo {
                        slot: number,
                        state: "stopped".to_string(),
                        workload: None,
                        workload_kind: None,
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

    /// the golden macOS VM's entry, after the Linux one, once a check found it or
    /// found none.
    fn macos_image(&self) -> Option<ImageInfo> {
        let golden = self.inner.macos_golden.as_ref()?.as_deref();
        let lines = macos_golden::lines(golden, &MACOS_BASE, self.now)
            .into_iter()
            .map(|(name, value)| ImageLine {
                name: name.to_string(),
                value,
            })
            .collect();
        let warnings = macos_golden::findings(golden, &MACOS_BASE, self.now)
            .into_iter()
            .map(|finding| match finding {
                MacosFreshness::RunnerOld { days } => ImageWarning {
                    code: "runner_old".to_string(),
                    days: Some(days),
                    version: None,
                },
                MacosFreshness::RunnerAgeUnknown => ImageWarning {
                    code: "runner_age_unknown".to_string(),
                    ..ImageWarning::default()
                },
                MacosFreshness::BaseOld { days } => ImageWarning {
                    code: "base_old".to_string(),
                    days: Some(days),
                    version: None,
                },
            })
            .collect();
        Some(ImageInfo {
            name: macos_golden::IMAGE_NAME.to_string(),
            lines,
            warnings,
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
            os: Some(crate::profile::Os::Macos),
            disk_gb: Some(50),
        }
    }

    fn runners(board: &Board) -> Vec<RunnerInfo> {
        snapshot(board, None)
            .pools
            .into_iter()
            .next()
            .unwrap()
            .runners
    }

    fn snapshot(board: &Board, macos_golden: Option<Option<String>>) -> Snapshot {
        let mut inner = Inner::new(Phase::Running);
        inner.macos_golden = macos_golden;
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
            }),
            migration: Migration::None,
            problems: Vec::new(),
        });
        let retained = Retained::default();
        let tokens = StoreTokens::new(None);
        View {
            version: "test",
            inner: &inner,
            config_path: "config.toml",
            board,
            retained: &retained,
            tokens: &tokens,
            left: &[],
            now: SystemTime::now(),
        }
        .snapshot()
    }

    #[test]
    fn a_found_golden_macos_vm_is_an_image_entry_with_its_lines_and_a_check_without_one_is_none() {
        let board = Board::default();
        assert!(snapshot(&board, None).images.is_empty());
        let images = snapshot(
            &board,
            Some(Some(
                "marshal-golden-macos-g1-r9.9.9-20260101000000".to_string(),
            )),
        )
        .images;
        let [image] = images.as_slice() else {
            panic!("{images:?}")
        };
        assert_eq!(image.name, "golden macOS VM");
        let names: Vec<_> = image.lines.iter().map(|line| line.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "name",
                "golden-vm-version",
                "runner-version",
                "base-published"
            ]
        );
        assert_eq!(
            image.lines[0].value,
            "marshal-golden-macos-g1-r9.9.9-20260101000000"
        );
        assert!(
            image
                .warnings
                .iter()
                .any(|warning| warning.code == "runner_age_unknown")
        );
        let missing = snapshot(&board, Some(None)).images;
        assert_eq!(missing[0].lines[0].value, "missing");
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

    #[test]
    fn a_vm_slot_names_its_wait_and_a_vm_workload_carries_its_kind() {
        let board = Board::default();
        board.set_slot("pool slot 1", SlotState::Waiting(HostWait::VmLimit));
        board.set_slot(
            "pool slot 2",
            SlotState::Waiting(HostWait::VmDeletion("marshal-job-pool-2-x".to_string())),
        );
        let waiting = runners(&board);
        assert_eq!(waiting[0].state, "waiting_for_vm_limit");
        assert_eq!(
            waiting[0].reason.as_deref(),
            Some("waiting for the macOS VM limit")
        );
        assert_eq!(
            (
                waiting[0].workload.as_deref(),
                waiting[0].workload_kind.as_deref()
            ),
            (None, None)
        );
        assert_eq!(waiting[1].state, "waiting_for_vm_deletion");
        assert_eq!(waiting[1].workload.as_deref(), Some("marshal-job-pool-2-x"));
        assert_eq!(waiting[1].workload_kind.as_deref(), Some("vm"));

        board.set_slot(
            "pool slot 1",
            SlotState::RunningVm("marshal-job-pool-1-x".to_string()),
        );
        let running = &runners(&board)[0];
        assert_eq!(running.workload.as_deref(), Some("marshal-job-pool-1-x"));
        assert_eq!(running.workload_kind.as_deref(), Some("vm"));
        // a Linux job on axmc runs in a container inside its VM
        board.set_slot("pool slot 1", SlotState::Running("pool-1-x".to_string()));
        assert_eq!(runners(&board)[0].workload_kind, None);
    }
}
