//! the value of the `state` topic and the result of `status`: what the agent is
//! doing, from local state it already keeps; it never carries a token or a JIT
//! configuration, and `fit` keeps it inside the agent's message limit.

use serde::{Deserialize, Serialize};

use super::wire::MAX_AGENT_MESSAGE_BYTES;

/// the most configuration problems a snapshot lists.
pub const MAX_PROBLEMS: usize = 100;

/// the most characters a long text of the snapshot keeps once nothing else can be cut.
const TEXT_CHARS: usize = 512;

/// room in a message for what wraps the snapshot: the type, the topic and the
/// sequence number.
const ENVELOPE_BYTES: usize = 256;

/// the most bytes of a snapshot's JSON.
pub const MAX_SNAPSHOT_BYTES: usize = MAX_AGENT_MESSAGE_BYTES - ENVELOPE_BYTES;

/// a reason a state is what it is; `code` is an open set and `message` is plain text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Reason {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// the latest build of the runner image.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuildInfo {
    /// milliseconds since the Unix epoch.
    #[serde(default)]
    pub started: u64,
    /// `building`, `ready`, `failed` or `cancelled`.
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub finished: Option<u64>,
    /// why a build failed or was cancelled.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentInfo {
    #[serde(default)]
    pub version: String,
    /// `running`, `resuming`, `stopping`, `stopped`, `building` or `quitting`.
    #[serde(default)]
    pub state: String,
    /// why the agent is stopped.
    #[serde(default)]
    pub reason: Option<Reason>,
    /// milliseconds since the Unix epoch.
    #[serde(default)]
    pub since: u64,
    #[serde(default)]
    pub build: Option<BuildInfo>,
    /// when the runtime core starts again after a failure, in milliseconds since the
    /// Unix epoch.
    #[serde(default)]
    pub restart_due: Option<u64>,
    /// the drain for an update, while the agent drains; the agent's state stays
    /// `running` meanwhile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateInfo>,
}

/// a drain for an update under way.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UpdateInfo {
    /// the version the agent drains for.
    #[serde(default)]
    pub version: String,
    /// the runners the drain still waits for: those still running a container,
    /// including an idle runner until its container has stopped.
    #[serde(default)]
    pub waiting: u32,
    /// when the drain stops the remaining runners, in milliseconds since the Unix
    /// epoch.
    #[serde(default)]
    pub deadline: u64,
    /// the agent's process ID, which the app waits on.
    #[serde(default)]
    pub pid: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConfigurationInfo {
    #[serde(default)]
    pub path: String,
    /// why the file is not usable, at most [`MAX_PROBLEMS`]; while the agent runs on
    /// its last valid configuration, why the file it read last is not used.
    #[serde(default)]
    pub problems: Vec<String>,
    /// what the latest read of the file found: `valid`, `invalid`, `missing` or
    /// `unreadable`; empty before the first read.
    #[serde(default)]
    pub status: String,
    /// the configuration the agent runs, absent when it holds none.
    #[serde(default)]
    pub in_use: Option<InUseInfo>,
    /// the migration of `config.json` to the file, absent when none applies.
    #[serde(default)]
    pub migration: Option<MigrationInfo>,
    /// the latest apply of an edit, absent until the first since the agent started.
    #[serde(default)]
    pub last_apply: Option<ApplyInfo>,
}

/// an apply of an edit of the configuration, and how it ended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ApplyInfo {
    /// the revision of the file the edit wrote.
    #[serde(default)]
    pub revision: String,
    /// when it started, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub at: u64,
    /// `applying`, `applied`, `abandoned` (by Stop all or Quit) or `failed`.
    #[serde(default)]
    pub outcome: String,
    /// why it failed; plain text.
    #[serde(default)]
    pub reason: Option<String>,
}

/// where the configuration the agent runs came from, and when it was read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InUseInfo {
    /// `file`, `last_valid` (an earlier read of the file, kept because the latest has
    /// problems or found none) or `json` (`config.json`, after a migration that failed).
    #[serde(default)]
    pub source: String,
    /// milliseconds since the Unix epoch.
    #[serde(default)]
    pub read_at: u64,
}

/// the state of the migration of `config.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MigrationInfo {
    /// `migrated`, `failed` or `pending` (the next start of the agent migrates it).
    #[serde(default)]
    pub state: String,
    /// when it was migrated, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub at: Option<u64>,
    /// why it failed; plain text.
    #[serde(default)]
    pub reason: Option<String>,
}

/// a repository the configuration names, and whether the store has its token.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RepositoryInfo {
    /// `owner/name` in lower case, the token store's key.
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub has_token: bool,
    #[serde(default)]
    pub pools: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RunnerInfo {
    /// counting from 1.
    #[serde(default)]
    pub slot: u32,
    /// `running`, `registering`, `backing_off`, `held` or `stopped`; an open set, so a
    /// reader treats a word it does not know as [`RunnerState::Unknown`].
    #[serde(default)]
    pub state: String,
    /// the name of what runs the runner's job, which today is a container.
    #[serde(default)]
    pub workload: Option<String>,
    /// plain text that says more about the state, such as what an unknown state means.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub since: u64,
}

/// a runner's state as a reader sorts it; the agent may add states within version 2.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunnerState {
    Running,
    Registering,
    BackingOff,
    Held,
    Stopped,
    /// a state this reader does not know; the runner's reason says what it means.
    Unknown(String),
}

impl RunnerState {
    #[must_use]
    pub fn of(word: &str) -> Self {
        match word {
            "running" => Self::Running,
            "registering" => Self::Registering,
            "backing_off" => Self::BackingOff,
            "held" => Self::Held,
            "stopped" => Self::Stopped,
            other => Self::Unknown(other.to_string()),
        }
    }
}

impl RunnerInfo {
    #[must_use]
    pub fn kind(&self) -> RunnerState {
        RunnerState::of(&self.state)
    }

    /// what to show for a state the reader does not know: the runner's reason.
    #[must_use]
    pub fn unknown_state_text(&self) -> Option<&str> {
        match self.kind() {
            RunnerState::Unknown(_) => self.reason.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DiagnosticContainer {
    #[serde(default)]
    pub name: String,
    /// `running`, `finalising` or `retained`.
    #[serde(default)]
    pub state: String,
    /// a bundle was admitted for the container.
    #[serde(default)]
    pub bundle: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DiagnosticFinding {
    #[serde(default)]
    pub container: String,
    /// `complete`, `gap`, `sink_full`, `failed`, `skipped` or `already_exported`.
    #[serde(default)]
    pub export: String,
    #[serde(default)]
    pub removed: bool,
    /// a forced stop ended it and kept the container.
    #[serde(default)]
    pub kept: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DiagnosticsInfo {
    /// the bundles a pool's sink may hold.
    #[serde(default)]
    pub limit: u32,
    /// bundles admitted since the agent started.
    #[serde(default)]
    pub written: u32,
    /// the pool's bundles in its sink as the latest export counted them, or `None`
    /// before one did.
    #[serde(default)]
    pub in_sink: Option<u32>,
    #[serde(default)]
    pub containers: Vec<DiagnosticContainer>,
    #[serde(default)]
    pub findings: Vec<DiagnosticFinding>,
    /// containers left for manual recovery.
    #[serde(default)]
    pub manual_recovery: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PoolInfo {
    #[serde(default)]
    pub name: String,
    /// `owner/name` in lower case.
    #[serde(default)]
    pub repository: String,
    /// `waiting_for_docker`, `preparing`, `running`, `skipped` or `stopped`.
    #[serde(default)]
    pub state: String,
    /// why the pool is skipped, also while it is listed stopped.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub has_token: bool,
    #[serde(default)]
    pub runners_up: u32,
    /// the slots the configuration asks for.
    #[serde(default)]
    pub slots: u32,
    #[serde(default)]
    pub runners: Vec<RunnerInfo>,
    /// present for a pool with diagnostics on.
    #[serde(default)]
    pub diagnostics: Option<DiagnosticsInfo>,
}

/// one line of what an image check found, such as `image-contract`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImageLine {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
}

/// a freshness finding as data, which the app words.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImageWarning {
    /// `not_built_by_marshal`, `packages_old`, `image_old`, `runner_old` or
    /// `runner_may_be_newer`.
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub days: Option<u64>,
    #[serde(default)]
    pub version: Option<String>,
}

/// a freshness warning as a reader sorts it; the agent may add warnings within
/// version 2, and a reader ignores one it does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarningKind {
    NotBuiltByMarshal,
    PackagesOld,
    ImageOld,
    RunnerOld,
    RunnerMayBeNewer,
    Unknown(String),
}

impl ImageWarning {
    #[must_use]
    pub fn kind(&self) -> WarningKind {
        match self.code.as_str() {
            "not_built_by_marshal" => WarningKind::NotBuiltByMarshal,
            "packages_old" => WarningKind::PackagesOld,
            "image_old" => WarningKind::ImageOld,
            "runner_old" => WarningKind::RunnerOld,
            "runner_may_be_newer" => WarningKind::RunnerMayBeNewer,
            other => WarningKind::Unknown(other.to_string()),
        }
    }
}

/// one image the agent keeps track of; today the runner image.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImageInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub lines: Vec<ImageLine>,
    #[serde(default)]
    pub warnings: Vec<ImageWarning>,
}

impl ImageInfo {
    /// the warnings this reader knows.
    pub fn known_warnings(&self) -> impl Iterator<Item = (WarningKind, &ImageWarning)> {
        self.warnings
            .iter()
            .map(|warning| (warning.kind(), warning))
            .filter(|(kind, _)| !matches!(kind, WarningKind::Unknown(_)))
    }
}

/// something a stop could not remove, until the next start does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Leftover {
    /// what it is: today always `container`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
}

/// what a snapshot left out to fit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Omitted {
    /// runner rows, from the end.
    #[serde(default)]
    pub runners: u32,
    /// configuration problems beyond the first [`MAX_PROBLEMS`].
    #[serde(default)]
    pub problems: u32,
    /// pools, from the end, once no runner row was left to drop.
    #[serde(default)]
    pub pools: u32,
    /// repositories, from the end, once no pool was left to drop.
    #[serde(default)]
    pub repositories: u32,
    /// leftovers, from the end, once no pool was left to drop.
    #[serde(default)]
    pub leftovers: u32,
    /// images, from the end, once no pool was left to drop.
    #[serde(default)]
    pub images: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Snapshot {
    #[serde(default)]
    pub agent: AgentInfo,
    #[serde(default)]
    pub configuration: ConfigurationInfo,
    /// runners whose container runs, in total.
    #[serde(default)]
    pub runners_up: u32,
    /// the slots the configuration asks for, in total.
    #[serde(default)]
    pub runners_wanted: u32,
    #[serde(default)]
    pub repositories: Vec<RepositoryInfo>,
    #[serde(default)]
    pub pools: Vec<PoolInfo>,
    /// what a stop could not remove, until the next start removes it.
    #[serde(default)]
    pub leftovers: Vec<Leftover>,
    /// the images the agent tracks; today one, the runner image.
    #[serde(default)]
    pub images: Vec<ImageInfo>,
    #[serde(default)]
    pub omitted: Omitted,
}

fn size(snapshot: &Snapshot) -> usize {
    serde_json::to_vec(snapshot).map_or(usize::MAX, |bytes| bytes.len())
}

impl Snapshot {
    /// leaves out configuration problems after the first [`MAX_PROBLEMS`], then runner
    /// rows from the end, then pools from the end, until the snapshot is at most
    /// `limit` bytes of JSON, and says how many it left out. A runner row left out
    /// still counts in `runners_up`.
    pub fn fit(&mut self, limit: usize) {
        if self.configuration.problems.len() > MAX_PROBLEMS {
            let left = self.configuration.problems.len() - MAX_PROBLEMS;
            self.configuration.problems.truncate(MAX_PROBLEMS);
            self.omitted.problems += u32::try_from(left).unwrap_or(u32::MAX);
        }
        if size(self) <= limit {
            return;
        }
        let rows: usize = self.pools.iter().map(|pool| pool.runners.len()).sum();
        // the most rows that fit, found by halving, so a snapshot of thousands of rows
        // is serialised a dozen times, not thousands
        let (mut low, mut high) = (0, rows);
        while low < high {
            let middle = (low + high).div_ceil(2);
            if size(&self.with_rows(middle)) <= limit {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        *self = self.with_rows(low);
        while size(self) > limit && !self.pools.is_empty() {
            self.pools.pop();
            self.omitted.pools += 1;
        }
        if size(self) > limit {
            self.fit_lists(limit);
        }
    }

    /// what is left to cut once no pool is: the lists beside the pools from their ends,
    /// then the configuration problems, and last the long texts, so that a snapshot
    /// always fits and says what it left out.
    fn fit_lists(&mut self, limit: usize) {
        self.keep_most(
            limit,
            |s| s.leftovers.len(),
            |s, n, cut| {
                s.leftovers.truncate(n);
                s.omitted.leftovers += cut;
            },
        );
        self.keep_most(
            limit,
            |s| s.repositories.len(),
            |s, n, cut| {
                s.repositories.truncate(n);
                s.omitted.repositories += cut;
            },
        );
        self.keep_most(
            limit,
            |s| s.images.len(),
            |s, n, cut| {
                s.images.truncate(n);
                s.omitted.images += cut;
            },
        );
        self.keep_most(
            limit,
            |s| s.configuration.problems.len(),
            |s, n, cut| {
                s.configuration.problems.truncate(n);
                s.omitted.problems += cut;
            },
        );
        if size(self) > limit {
            for text in [&mut self.configuration.path, &mut self.agent.version] {
                *text = text.chars().take(TEXT_CHARS).collect();
            }
            if let Some(reason) = &mut self.agent.reason {
                reason.message = reason.message.chars().take(TEXT_CHARS).collect();
            }
            if let Some(migration) = &mut self.configuration.migration {
                migration.reason = migration
                    .reason
                    .take()
                    .map(|text| text.chars().take(TEXT_CHARS).collect());
            }
            if let Some(build) = &mut self.agent.build {
                build.message = build
                    .message
                    .take()
                    .map(|text| text.chars().take(TEXT_CHARS).collect());
            }
        }
    }

    /// keeps the most entries of a list, found by halving, that bring the snapshot
    /// within `limit`, or none when none helps. `keep` truncates the list to the
    /// entries kept and adds the count cut to the list's `omitted` field, so that every
    /// trial is measured with the count it will carry.
    fn keep_most(
        &mut self,
        limit: usize,
        count: fn(&Self) -> usize,
        keep: fn(&mut Self, usize, u32),
    ) {
        let total = count(self);
        if total == 0 || size(self) <= limit {
            return;
        }
        let cut = |kept: usize| u32::try_from(total - kept).unwrap_or(u32::MAX);
        let (mut low, mut high) = (0, total);
        while low < high {
            let middle = (low + high).div_ceil(2);
            let mut trial = self.clone();
            keep(&mut trial, middle, cut(middle));
            if size(&trial) <= limit {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        keep(self, low, cut(low));
    }

    /// a copy that keeps the first `keep` runner rows and counts the rest as omitted,
    /// so that measuring it measures the count it will carry.
    fn with_rows(&self, keep: usize) -> Self {
        let mut left = keep;
        let mut copy = self.clone();
        for pool in &mut copy.pools {
            let take = left.min(pool.runners.len());
            pool.runners.truncate(take);
            left -= take;
        }
        let cut = self
            .pools
            .iter()
            .map(|pool| pool.runners.len())
            .sum::<usize>()
            - keep;
        copy.omitted.runners += u32::try_from(cut).unwrap_or(u32::MAX);
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runner_state_words_are_known_except_those_a_reader_has_not_met() {
        assert_eq!(RunnerState::of("held"), RunnerState::Held);
        assert_eq!(RunnerState::of("backing_off"), RunnerState::BackingOff);
        assert_eq!(
            RunnerState::of("booting_vm"),
            RunnerState::Unknown("booting_vm".to_string())
        );
    }

    fn pool(name: &str, runners: usize) -> PoolInfo {
        PoolInfo {
            name: name.to_string(),
            slots: u32::try_from(runners).unwrap(),
            runners: (1..=runners)
                .map(|slot| RunnerInfo {
                    slot: u32::try_from(slot).unwrap(),
                    state: "running".to_string(),
                    workload: Some(format!("{name}-{slot}-20260101000000000")),
                    reason: None,
                    since: 1,
                })
                .collect(),
            ..PoolInfo::default()
        }
    }

    #[test]
    fn a_small_snapshot_is_left_as_it_is() {
        let mut snapshot = Snapshot {
            pools: vec![pool("a", 3)],
            ..Snapshot::default()
        };
        let before = snapshot.clone();
        snapshot.fit(MAX_SNAPSHOT_BYTES);
        assert_eq!(snapshot, before);
    }

    #[test]
    fn problems_after_the_first_hundred_are_counted_and_dropped() {
        let mut snapshot = Snapshot::default();
        snapshot.configuration.problems =
            (0..130).map(|index| format!("problem {index}")).collect();
        snapshot.fit(MAX_SNAPSHOT_BYTES);
        assert_eq!(snapshot.configuration.problems.len(), MAX_PROBLEMS);
        assert_eq!(snapshot.configuration.problems[99], "problem 99");
        assert_eq!(snapshot.omitted.problems, 30);
    }

    #[test]
    fn runner_rows_are_dropped_from_the_end_until_the_snapshot_fits() {
        let mut snapshot = Snapshot {
            pools: vec![pool("first", 600), pool("second", 600)],
            runners_up: 1200,
            runners_wanted: 1200,
            ..Snapshot::default()
        };
        assert!(size(&snapshot) > MAX_SNAPSHOT_BYTES);
        let whole = snapshot.clone();
        snapshot.fit(MAX_SNAPSHOT_BYTES);
        assert!(size(&snapshot) <= MAX_SNAPSHOT_BYTES);
        let kept: usize = snapshot.pools.iter().map(|pool| pool.runners.len()).sum();
        assert!(kept > 0 && kept < 1200, "{kept}");
        assert_eq!(snapshot.omitted.runners as usize, 1200 - kept);
        assert_eq!(
            snapshot.pools[0].runners.len(),
            600,
            "the first pool is whole"
        );
        assert_eq!(
            snapshot.runners_up, 1200,
            "the totals still count every runner"
        );
        let one_more = whole.with_rows(kept + 1);
        assert!(
            size(&one_more) > MAX_SNAPSHOT_BYTES || kept == 1200,
            "one more row would not fit"
        );
    }

    #[test]
    fn a_cut_that_fits_only_without_its_own_omitted_count_drops_one_more_row_not_a_pool() {
        let whole = Snapshot {
            pools: vec![pool("first", 50), pool("second", 50)],
            ..Snapshot::default()
        };
        // the cut of 60 rows, written out with the count it carries: two digits
        let mut sixty = whole.clone();
        sixty.pools[1].runners.truncate(10);
        sixty.omitted.runners = 40;
        let limit = size(&sixty) - 1;
        assert!(size(&whole) > limit);

        let mut snapshot = whole.clone();
        snapshot.fit(limit);

        assert!(size(&snapshot) <= limit);
        assert_eq!(snapshot.omitted.pools, 0, "no pool goes while a row can");
        assert_eq!(snapshot.pools.len(), 2);
        let shown: usize = snapshot.pools.iter().map(|pool| pool.runners.len()).sum();
        assert_eq!(shown, 59, "one row fewer than the cut that is a byte over");
        assert_eq!(shown + snapshot.omitted.runners as usize, 100);
    }

    #[test]
    fn pools_go_only_when_no_runner_row_is_left_to_drop() {
        let mut snapshot = Snapshot {
            pools: (0..2000)
                .map(|index| PoolInfo {
                    name: format!("pool-{index}"),
                    reason: Some("x".repeat(80)),
                    ..PoolInfo::default()
                })
                .collect(),
            ..Snapshot::default()
        };
        snapshot.fit(MAX_SNAPSHOT_BYTES);
        assert!(size(&snapshot) <= MAX_SNAPSHOT_BYTES);
        assert!(snapshot.omitted.pools > 0);
        assert_eq!(snapshot.pools.len() + snapshot.omitted.pools as usize, 2000);
    }

    #[test]
    fn lists_beside_the_pools_are_cut_too_so_a_snapshot_always_fits() {
        let mut snapshot = Snapshot {
            leftovers: (0..4000)
                .map(|index| Leftover {
                    kind: "container".to_string(),
                    name: format!("container-{index}-{}", "x".repeat(40)),
                })
                .collect(),
            repositories: (0..2000)
                .map(|index| RepositoryInfo {
                    repository: format!("owner/repository-{index}"),
                    has_token: true,
                    pools: vec![format!("pool-{index}")],
                })
                .collect(),
            runners_up: 7,
            ..Snapshot::default()
        };
        snapshot.fit(MAX_SNAPSHOT_BYTES);
        assert!(size(&snapshot) <= MAX_SNAPSHOT_BYTES);
        assert_eq!(
            snapshot.leftovers.len() + snapshot.omitted.leftovers as usize,
            4000
        );
        assert!(snapshot.omitted.leftovers > 0);
        assert_eq!(snapshot.runners_up, 7);
    }

    /// fits `whole` to a limit that a cut of one list measured without its omitted
    /// count would just meet, and checks that the snapshot meets it with the count.
    fn fit_to_a_cut_written_without_its_count(
        whole: &Snapshot,
        cut: fn(&mut Snapshot),
        total: usize,
        shown_and_omitted: fn(&Snapshot) -> usize,
    ) {
        let mut without_count = whole.clone();
        cut(&mut without_count);
        let limit = size(&without_count);
        assert!(size(whole) > limit);

        let mut snapshot = whole.clone();
        snapshot.fit(limit);

        assert!(size(&snapshot) <= limit, "{} > {limit}", size(&snapshot));
        assert_eq!(shown_and_omitted(&snapshot), total);
    }

    #[test]
    fn a_leftover_cut_carries_its_own_omitted_count_within_the_limit() {
        let whole = Snapshot {
            leftovers: (0..300)
                .map(|index| Leftover {
                    kind: "container".to_string(),
                    name: format!("container-{index:04}"),
                })
                .collect(),
            ..Snapshot::default()
        };
        fit_to_a_cut_written_without_its_count(
            &whole,
            |s| s.leftovers.truncate(100),
            300,
            |s| s.leftovers.len() + s.omitted.leftovers as usize,
        );
    }

    #[test]
    fn a_repository_cut_carries_its_own_omitted_count_within_the_limit() {
        let whole = Snapshot {
            repositories: (0..300)
                .map(|index| RepositoryInfo {
                    repository: format!("owner/repository-{index:04}"),
                    has_token: true,
                    pools: Vec::new(),
                })
                .collect(),
            ..Snapshot::default()
        };
        fit_to_a_cut_written_without_its_count(
            &whole,
            |s| s.repositories.truncate(100),
            300,
            |s| s.repositories.len() + s.omitted.repositories as usize,
        );
    }

    #[test]
    fn an_image_cut_carries_its_own_omitted_count_within_the_limit() {
        let whole = Snapshot {
            images: (0..300)
                .map(|index| ImageInfo {
                    name: format!("image-{index:04}"),
                    ..ImageInfo::default()
                })
                .collect(),
            ..Snapshot::default()
        };
        fit_to_a_cut_written_without_its_count(
            &whole,
            |s| s.images.truncate(100),
            300,
            |s| s.images.len() + s.omitted.images as usize,
        );
    }

    #[test]
    fn a_problem_cut_carries_its_own_omitted_count_within_the_limit() {
        let whole = Snapshot {
            configuration: ConfigurationInfo {
                problems: (0..MAX_PROBLEMS)
                    .map(|index| format!("problem {index:04}"))
                    .collect(),
                ..ConfigurationInfo::default()
            },
            ..Snapshot::default()
        };
        fit_to_a_cut_written_without_its_count(
            &whole,
            |s| s.configuration.problems.truncate(50),
            MAX_PROBLEMS,
            |s| s.configuration.problems.len() + s.omitted.problems as usize,
        );
    }

    #[test]
    fn every_limit_gives_a_snapshot_within_it_that_accounts_for_each_list() {
        const POOLS: usize = 3;
        const ROWS: usize = 12;
        const LEFTOVERS: usize = 20;
        const REPOSITORIES: usize = 15;
        const IMAGES: usize = 8;
        const PROBLEMS: usize = MAX_PROBLEMS + 12;
        let whole = Snapshot {
            pools: (0..POOLS)
                .map(|index| pool(&format!("pool-{index}"), ROWS / POOLS))
                .collect(),
            leftovers: (0..LEFTOVERS)
                .map(|index| Leftover {
                    kind: "container".to_string(),
                    name: format!("container-{index:04}"),
                })
                .collect(),
            repositories: (0..REPOSITORIES)
                .map(|index| RepositoryInfo {
                    repository: format!("owner/repository-{index:04}"),
                    has_token: true,
                    pools: vec![format!("pool-{}", index % POOLS)],
                })
                .collect(),
            images: (0..IMAGES)
                .map(|index| ImageInfo {
                    name: format!("image-{index:04}"),
                    ..ImageInfo::default()
                })
                .collect(),
            configuration: ConfigurationInfo {
                problems: (0..PROBLEMS)
                    .map(|index| format!("problem {index:04}"))
                    .collect(),
                ..ConfigurationInfo::default()
            },
            ..Snapshot::default()
        };
        let mut floor = whole.clone();
        floor.fit(0);
        let (low, high) = (size(&floor), size(&whole));
        assert!(high - low > 1000, "{low}..{high}");

        for limit in (low..=high).step_by(2) {
            let mut snapshot = whole.clone();
            snapshot.fit(limit);
            let accounted = |shown: usize, omitted: u32| shown + omitted as usize;
            assert!(size(&snapshot) <= limit, "{} > {limit}", size(&snapshot));
            assert_eq!(
                accounted(snapshot.pools.len(), snapshot.omitted.pools),
                POOLS
            );
            assert_eq!(
                accounted(snapshot.leftovers.len(), snapshot.omitted.leftovers),
                LEFTOVERS,
                "limit {limit}"
            );
            assert_eq!(
                accounted(snapshot.repositories.len(), snapshot.omitted.repositories),
                REPOSITORIES,
                "limit {limit}"
            );
            assert_eq!(
                accounted(snapshot.images.len(), snapshot.omitted.images),
                IMAGES,
                "limit {limit}"
            );
            assert_eq!(
                accounted(
                    snapshot.configuration.problems.len(),
                    snapshot.omitted.problems
                ),
                PROBLEMS,
                "limit {limit}"
            );
            let rows: usize = snapshot.pools.iter().map(|pool| pool.runners.len()).sum();
            assert_eq!(
                accounted(rows, snapshot.omitted.runners),
                ROWS,
                "limit {limit}"
            );
            assert!(
                snapshot.omitted.pools == 0 || rows == 0,
                "a pool went while a row was left, at limit {limit}"
            );
        }
    }

    #[test]
    fn a_snapshot_with_huge_texts_falls_back_to_short_ones() {
        let mut snapshot = Snapshot::default();
        snapshot.agent.reason = Some(Reason {
            code: "invalid_configuration".to_string(),
            message: "y".repeat(200_000),
        });
        snapshot.configuration.path = "p".repeat(100_000);
        snapshot.fit(MAX_SNAPSHOT_BYTES);
        assert!(size(&snapshot) <= MAX_SNAPSHOT_BYTES);
        assert_eq!(snapshot.agent.reason.unwrap().message.len(), TEXT_CHARS);
    }

    #[test]
    fn a_snapshot_is_read_back_and_a_field_it_does_not_know_is_ignored() {
        let mut text = serde_json::to_value(Snapshot {
            pools: vec![pool("a", 1)],
            ..Snapshot::default()
        })
        .unwrap();
        text["added_later"] = serde_json::json!({"x": 1});
        text["pools"][0]["also_later"] = serde_json::json!(true);
        let read: Snapshot = serde_json::from_value(text).unwrap();
        assert_eq!(read.pools[0].name, "a");
    }
}
