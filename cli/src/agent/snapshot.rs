//! the value of the `state` topic and the result of `status`: what the agent is
//! doing, from local state it already keeps; it never carries a token or a JIT
//! configuration, and `fit` keeps it inside the agent's message limit.

use serde::{Deserialize, Serialize};

use super::wire::MAX_AGENT_MESSAGE_BYTES;

/// the most configuration problems a snapshot lists.
pub const MAX_PROBLEMS: usize = 100;

/// room in a message for what wraps the snapshot: the type, the topic and the
/// sequence number.
const ENVELOPE_BYTES: usize = 256;

/// the most bytes of a snapshot's JSON.
pub const MAX_SNAPSHOT_BYTES: usize = MAX_AGENT_MESSAGE_BYTES - ENVELOPE_BYTES;

/// a reason a state is what it is; `code` is an open set and `message` is plain text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reason {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

/// the latest build of the runner image.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationInfo {
    #[serde(default)]
    pub path: String,
    /// why the file is not usable, at most [`MAX_PROBLEMS`].
    #[serde(default)]
    pub problems: Vec<String>,
}

/// a repository the configuration names, and whether the store has its token.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct RunnerInfo {
    /// counting from 1.
    #[serde(default)]
    pub slot: u32,
    /// `running`, `registering`, `backing_off` or `stopped`; an open set, so a reader
    /// treats a word it does not know as [`RunnerState::Unknown`].
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
pub enum RunnerState {
    Running,
    Registering,
    BackingOff,
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
pub struct ImageLine {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
}

/// a freshness finding as data, which the app words.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct Leftover {
    /// what it is: today always `container`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
}

/// what a snapshot left out to fit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
        let keep = low;
        let mut cut = self.with_rows(keep);
        self.omitted.runners += u32::try_from(rows - keep).unwrap_or(u32::MAX);
        cut.omitted = self.omitted.clone();
        *self = cut;
        while size(self) > limit && !self.pools.is_empty() {
            self.pools.pop();
            self.omitted.pools += 1;
        }
    }

    /// a copy that keeps the first `keep` runner rows.
    fn with_rows(&self, keep: usize) -> Self {
        let mut left = keep;
        let mut copy = self.clone();
        for pool in &mut copy.pools {
            let take = left.min(pool.runners.len());
            pool.runners.truncate(take);
            left -= take;
        }
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
