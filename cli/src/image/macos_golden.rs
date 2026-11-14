//! what `status` and the agent's snapshot say about the golden macOS VM: its name,
//! golden VM version and runner, the pinned base's publication, and the warnings
//! about the runner's and the base's ages; see `docs/operations/cli.md`.

use std::time::SystemTime;

use crate::macos::name::{GOLDEN_MACOS_VM_VERSION, GoldenMacosName};

use super::check::{age_days, date_seconds};
use super::macos_base::{
    FIRST_WARNING_DAYS as BASE_FIRST_WARNING_DAYS, MacosBase, base_age_days, base_age_warnings,
};
use super::runner::release;

/// the name the snapshot's image list gives the golden macOS VM.
pub const IMAGE_NAME: &str = "golden macOS VM";

const RUNNER_FIRST_WARNING_DAYS: u64 = 7;

const RUNNER_SECOND_WARNING_DAYS: u64 = 21;

/// a warning about the golden macOS VM, as data the snapshot carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacosFreshness {
    /// its runner was released this many days ago, 7 or more.
    RunnerOld { days: u64 },
    /// the runner record holds no release date for its runner's version.
    RunnerAgeUnknown,
    /// the pinned base was published this many days ago, 30 or more.
    BaseOld { days: u64 },
}

/// the warnings about `golden`, the newest golden macOS VM's name, and `base`, as of
/// `now`; a missing golden VM has only the base's.
#[must_use]
pub fn findings(golden: Option<&str>, base: &MacosBase, now: SystemTime) -> Vec<MacosFreshness> {
    let mut findings = Vec::new();
    if let Some(parsed) = golden.and_then(GoldenMacosName::parse) {
        match runner_released(&parsed).map(|released| age_days(now, released)) {
            None => findings.push(MacosFreshness::RunnerAgeUnknown),
            Some(days) if days >= RUNNER_FIRST_WARNING_DAYS => {
                findings.push(MacosFreshness::RunnerOld { days });
            }
            Some(_) => {}
        }
    }
    if let Some(days) = base_age_days(base, now).filter(|days| *days >= BASE_FIRST_WARNING_DAYS) {
        findings.push(MacosFreshness::BaseOld { days });
    }
    findings
}

/// the lines that describe the golden macOS VM, under the snapshot's names, each as
/// `status` shows it under `golden-macos-<name>`: `name`, `golden-vm-version`,
/// `runner-version` and `base-published`.
#[must_use]
pub fn lines(
    golden: Option<&str>,
    base: &MacosBase,
    now: SystemTime,
) -> Vec<(&'static str, String)> {
    let parsed = golden.and_then(GoldenMacosName::parse);
    let version = parsed.as_ref().map_or_else(
        || "missing".to_string(),
        |parsed| {
            if parsed.version() == GOLDEN_MACOS_VM_VERSION {
                GOLDEN_MACOS_VM_VERSION.to_string()
            } else {
                format!(
                    "mismatch (golden VM version {}, CLI {GOLDEN_MACOS_VM_VERSION})",
                    parsed.version()
                )
            }
        },
    );
    let runner = parsed.as_ref().map_or_else(
        || "missing".to_string(),
        |parsed| match runner_released(parsed) {
            Some(released) => format!(
                "{}, released {} days ago",
                parsed.runner_version(),
                age_days(now, released)
            ),
            None => format!("{}, released unknown", parsed.runner_version()),
        },
    );
    let published = base_age_days(base, now).map_or_else(
        || base.published.to_string(),
        |days| format!("{}, {days} days ago", base.published),
    );
    vec![
        ("name", golden.unwrap_or("missing").to_string()),
        ("golden-vm-version", version),
        ("runner-version", runner),
        ("base-published", published),
    ]
}

/// the `status` fields of the golden macOS VM: its lines, then one `warning` per
/// finding.
#[must_use]
pub fn status_fields(
    golden: Option<&str>,
    base: &MacosBase,
    now: SystemTime,
) -> Vec<(&'static str, String)> {
    let mut fields: Vec<_> = lines(golden, base, now)
        .into_iter()
        .map(|(name, value)| (status_name(name), value))
        .collect();
    for finding in findings(golden, base, now) {
        match finding {
            MacosFreshness::RunnerOld { days } => fields.push(("warning", runner_text(golden, days))),
            MacosFreshness::RunnerAgeUnknown => fields.push((
                "warning",
                "the golden macOS VM's runner has no release date in this CLI, so its age is unknown"
                    .to_string(),
            )),
            MacosFreshness::BaseOld { .. } => {
                fields.extend(base_age_warnings(base, now).into_iter().map(|text| ("warning", text)));
            }
        }
    }
    fields
}

fn status_name(line: &str) -> &'static str {
    match line {
        "name" => "golden-macos-vm",
        "golden-vm-version" => "golden-macos-vm-version",
        "runner-version" => "golden-macos-runner",
        _ => "macos-base-published",
    }
}

fn runner_text(golden: Option<&str>, days: u64) -> String {
    let version = golden
        .and_then(GoldenMacosName::parse)
        .map(|parsed| parsed.runner_version().to_string())
        .unwrap_or_default();
    if days >= RUNNER_SECOND_WARNING_DAYS {
        format!(
            "the golden macOS VM's runner {version} was released {days} days ago, so each job's \
             runner likely updates itself at start; update the CLI to get a newer runner, then \
             rebuild the golden macOS VM"
        )
    } else {
        format!(
            "the golden macOS VM's runner {version} was released {days} days ago, and a newer \
             runner may exist"
        )
    }
}

/// when the runner the golden VM's name records was released, in Unix seconds.
fn runner_released(parsed: &GoldenMacosName) -> Option<u64> {
    date_seconds(release(parsed.runner_version())?.released)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    const DAY: u64 = 86_400;
    const BASE: MacosBase = MacosBase {
        digest: "sha256:0",
        macos_version: "26.6.2",
        published: "2026-01-01",
        retired: &[],
    };

    fn day(text: &str, extra_days: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(date_seconds(text).unwrap() + extra_days * DAY)
    }

    #[test]
    fn each_age_warns_from_its_first_threshold_inclusive_with_its_days() {
        let released = release("2.337.0").unwrap().released;
        let golden = "marshal-golden-macos-g1-r2.337.0-20260101000000";
        let base = MacosBase {
            published: released,
            ..BASE
        };
        assert_eq!(findings(Some(golden), &base, day(released, 6)), []);
        assert_eq!(
            findings(Some(golden), &base, day(released, 7)),
            [MacosFreshness::RunnerOld { days: 7 }]
        );
        assert_eq!(
            findings(Some(golden), &base, day(released, 30)),
            [
                MacosFreshness::RunnerOld { days: 30 },
                MacosFreshness::BaseOld { days: 30 }
            ]
        );
    }

    #[test]
    fn a_runner_the_record_does_not_hold_has_an_unknown_age_and_a_missing_vm_only_the_base() {
        let golden = "marshal-golden-macos-g1-r9.9.9-20260101000000";
        let now = day(BASE.published, 1);
        assert_eq!(
            findings(Some(golden), &BASE, now),
            [MacosFreshness::RunnerAgeUnknown]
        );
        assert_eq!(
            lines(Some(golden), &BASE, now)[2].1,
            "9.9.9, released unknown"
        );
        assert_eq!(
            findings(None, &BASE, day(BASE.published, 60)),
            [MacosFreshness::BaseOld { days: 60 }]
        );
        let missing = lines(None, &BASE, now);
        assert_eq!(
            missing,
            [
                ("name", "missing".to_string()),
                ("golden-vm-version", "missing".to_string()),
                ("runner-version", "missing".to_string()),
                ("base-published", "2026-01-01, 1 days ago".to_string()),
            ]
        );
    }

    #[test]
    fn status_strengthens_each_warning_at_its_second_threshold_inclusive() {
        let released = release("2.337.0").unwrap().released;
        let golden = Some("marshal-golden-macos-g1-r2.337.0-20260101000000");
        let base = MacosBase {
            published: released,
            ..BASE
        };
        let warnings = |days| -> Vec<String> {
            status_fields(golden, &base, day(released, days))
                .into_iter()
                .filter(|(field, _)| *field == "warning")
                .map(|(_, text)| text)
                .collect()
        };
        assert!(warnings(20)[0].ends_with("and a newer runner may exist"));
        assert!(warnings(21)[0].contains("likely updates itself at start"));
        let base_warning = |days| warnings(days).last().unwrap().clone();
        assert!(!base_warning(59).contains("far behind"));
        assert!(base_warning(60).contains("far behind"));
    }

    #[test]
    fn a_golden_vm_of_another_version_shows_both_versions() {
        let golden = "marshal-golden-macos-g7-r2.337.0-20260101000000";
        let shown = &lines(Some(golden), &BASE, day(BASE.published, 0))[1].1;
        assert_eq!(
            shown,
            &format!("mismatch (golden VM version 7, CLI {GOLDEN_MACOS_VM_VERSION})")
        );
    }
}
