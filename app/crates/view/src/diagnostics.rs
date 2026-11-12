//! a pool's Diagnostics section: its kept containers, finalisations under way, bundles
//! saved against the limit, and containers left for the manual recovery.

use marshal_cli::agent::snapshot::{
    DiagnosticContainer, DiagnosticFinding, DiagnosticsInfo, PoolInfo,
};
use serde::Serialize;

use crate::text::plain;

const RETRIED: &str =
    "It stays retained, and is retried at the next Stop All and when the agent next starts.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticItem {
    pub container: String,
    /// "Runner 2", when the container's name says which runner it came from.
    pub from: Option<String>,
    pub text: String,
    /// whether it needs attention: a failed export or removal, a gap, a kept container.
    pub problem: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostics {
    pub heading: String,
    /// "9 saved of 10; 1 being saved", or that the pool's diagnostics are off.
    pub summary: String,
    /// lines under the summary: what the limit means, and what a "?" means.
    pub notes: Vec<String>,
    pub items: Vec<DiagnosticItem>,
    /// whether the pool has its diagnostics on.
    pub on: bool,
}

/// the runner a container came from: its name is `<pool>-<slot>-<time>`.
fn runner_of(pool: &str, container: &str) -> Option<String> {
    let rest = container.strip_prefix(pool)?.strip_prefix('-')?;
    let (slot, time) = rest.split_once('-')?;
    let digits = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_digit());
    (digits(slot) && digits(time)).then(|| format!("Runner {slot}"))
}

/// how an export ended, as the start of a sentence; a code this reader does not know is
/// shown as it is.
fn export_text(code: &str, limit: u32) -> String {
    match code {
        "complete" => "Saved".to_string(),
        "gap" => "Saved with a gap, because the final observation is missing".to_string(),
        "sink_full" => {
            format!("Not saved, because the pool's directory already holds {limit} bundles")
        }
        "failed" => "Not saved, because the export failed".to_string(),
        "skipped" => "Not saved, because its export was skipped".to_string(),
        "already_exported" => "Already saved by an earlier run".to_string(),
        other => format!("Export result: {}", plain(other)),
    }
}

fn finding_text(finding: &DiagnosticFinding, limit: u32) -> (String, bool) {
    let base = export_text(&finding.export, limit);
    let bad_export = !matches!(finding.export.as_str(), "complete" | "already_exported");
    if finding.removed {
        (format!("{base}; removed."), bad_export)
    } else if finding.kept {
        (
            format!(
                "{base}. A forced stop ended it, so the container is kept; the next start with this pool's diagnostics on exports and removes it."
            ),
            true,
        )
    } else {
        (format!("{base}. Its removal failed. {RETRIED}"), true)
    }
}

fn container_text(container: &DiagnosticContainer) -> (String, bool) {
    if container.state == "finalising" {
        return (
            "Saving a bundle, then removing the container.".to_string(),
            false,
        );
    }
    let saved = if container.bundle {
        "Saved, but not removed yet"
    } else {
        "Not saved yet"
    };
    (format!("{saved}. {RETRIED}"), true)
}

fn items(pool: &PoolInfo, info: &DiagnosticsInfo) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    for container in info
        .containers
        .iter()
        .filter(|each| each.state != "running")
    {
        let finding = info
            .findings
            .iter()
            .find(|finding| finding.container == container.name);
        let (text, problem) = match finding {
            Some(finding) => finding_text(finding, info.limit),
            None => container_text(container),
        };
        items.push(item(pool, &container.name, text, problem));
    }
    for finding in info.findings.iter().filter(|finding| {
        !info
            .containers
            .iter()
            .any(|each| each.name == finding.container)
    }) {
        let (text, problem) = finding_text(finding, info.limit);
        items.push(item(pool, &finding.container, text, problem));
    }
    for name in &info.manual_recovery {
        items.push(item(
            pool,
            name,
            "Left for the manual recovery in Windows Runner Host: Marshal has no diagnostics settings to save it with."
                .to_string(),
            true,
        ));
    }
    items
}

fn item(pool: &PoolInfo, container: &str, text: String, problem: bool) -> DiagnosticItem {
    DiagnosticItem {
        container: plain(container),
        from: runner_of(&pool.name, container),
        text,
        problem,
    }
}

/// the section of a pool with diagnostics.
pub fn section(pool: &PoolInfo) -> Option<Diagnostics> {
    let info = pool.diagnostics.as_ref()?;
    let only_manual = !info.manual_recovery.is_empty()
        && info.containers.is_empty()
        && info.written == 0
        && info.in_sink.is_none();
    let items = items(pool, info);
    if only_manual {
        return Some(Diagnostics {
            heading: "Diagnostics".to_string(),
            summary: "Off for this pool".to_string(),
            notes: Vec::new(),
            items,
            on: false,
        });
    }
    let saving = info
        .containers
        .iter()
        .filter(|container| container.state == "finalising")
        .count();
    let saved = info
        .in_sink
        .map_or_else(|| "?".to_string(), |count| count.to_string());
    let mut summary = format!("{saved} saved of {}", info.limit);
    if saving > 0 {
        summary.push_str(&format!("; {saving} being saved"));
    }
    let mut notes = vec![format!(
        "When it holds {}, later containers are removed without a bundle.",
        info.limit
    )];
    if info.in_sink.is_none() {
        notes.push("? means the count is not known yet, or it failed.".to_string());
    }
    if info.findings.iter().any(|finding| finding.removed) {
        notes.push("Removed containers stay listed until the next start.".to_string());
    }
    Some(Diagnostics {
        heading: "Diagnostics".to_string(),
        summary,
        notes,
        items,
        on: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_name_says_its_runner() {
        assert_eq!(
            runner_of("app", "app-2-20261104031530871").as_deref(),
            Some("Runner 2")
        );
        assert_eq!(
            runner_of("docs-arm", "docs-arm-1-20261104031530871").as_deref(),
            Some("Runner 1")
        );
        assert_eq!(runner_of("app", "other-2-20261104031530871"), None);
        assert_eq!(runner_of("app", "app-x-20261104031530871"), None);
    }
}
