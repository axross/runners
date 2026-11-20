//! the configuration editor's fields and availability. The agent remains the only
//! reader, validator and writer of the file; an absent value stays absent until set.

use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::profile::Profile;
use serde::Serialize;

use crate::Connection;

/// the negotiated editor's fields and the agent state that prevents a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorAccess {
    pub reason: Option<String>,
    pub read_reason: Option<String>,
    pub stopped: bool,
    pub settings: Vec<EditorField>,
    pub pool: Vec<EditorField>,
    pub volume: Vec<EditorField>,
}

/// one profile's editable key; `absent` explains an unset value without supplying it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorField {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub absent: String,
}

/// the page uses the kind to choose a control, not to duplicate schema validation.
fn field(key: &str, label: &str, kind: &str, absent: &str) -> EditorField {
    EditorField {
        key: key.into(),
        label: label.into(),
        kind: kind.into(),
        absent: absent.into(),
    }
}

/// the fields accepted by this host, and why reading or applying is unavailable.
#[must_use]
pub fn editor_access(
    profile: Profile,
    connection: Connection,
    snapshot: Option<&Snapshot>,
) -> EditorAccess {
    let connected = matches!(connection, Connection::Connected);
    let read_reason =
        (!connected).then(|| "Marshal is not answering. Your changes are kept.".into());
    let mut reason = read_reason.clone();
    if reason.is_none() {
        reason = snapshot.and_then(|snapshot| {
            if snapshot.configuration.in_use.as_ref().is_some_and(|used| used.source == "json")
                || snapshot.configuration.migration.as_ref().is_some_and(|migration| {
                    matches!(migration.state.as_str(), "pending" | "failed")
                })
            {
                return Some("The migration runs at Marshal's next start. Quit Marshal and launch it again.".into());
            }
            if snapshot.agent.update.is_some() {
                return Some("Unavailable while Marshal drains for an update.".into());
            }
            if snapshot.agent.restart_due.is_some() {
                return Some("Unavailable while the runtime waits to restart.".into());
            }
            if snapshot.configuration.last_apply.as_ref().is_some_and(|apply| apply.outcome == "applying") {
                return Some("Marshal is applying the last change.".into());
            }
            (!matches!(snapshot.agent.state.as_str(), "running" | "stopped"))
                .then(|| "Unavailable while Marshal starts, stops, builds or quits.".into())
        }).or_else(|| snapshot.is_none().then(|| "Waiting for the agent's state.".into()));
    }
    let mac = profile == Profile::Axmc;
    let mut settings = vec![field(
        "routing_label",
        "Routing label",
        "text",
        "Not set — required",
    )];
    settings.push(field(
        "image_name",
        "Image name",
        "text",
        if mac {
            "Not set — actions-runner:local"
        } else {
            "Not set — required"
        },
    ));
    if mac {
        settings.extend([
            field("budget.cpus", "CPU budget", "number", "Not set — required"),
            field(
                "budget.memory_gb",
                "Memory budget (GB)",
                "integer",
                "Not set — required",
            ),
            field(
                "budget.disk_gb",
                "Disk budget (GB)",
                "integer",
                "Not set — required",
            ),
        ]);
    }
    let mut pool = vec![
        field("name", "Name", "text", "Not set — required"),
        field(
            "repository",
            "Repository",
            "text",
            "Not set — owner/name required",
        ),
        field(
            "runners",
            "Runners",
            "integer",
            if mac {
                "Not set — 1 for macOS, 2 for Linux"
            } else {
                "Not set — required"
            },
        ),
        field(
            "labels",
            "Labels (one per line)",
            "texts",
            "Not set — platform labels and routing label",
        ),
        field(
            "os",
            "OS",
            if mac { "os" } else { "linux" },
            "Not set — Linux",
        ),
        field(
            "cpus",
            "CPUs per runner",
            if mac { "integer" } else { "number" },
            if mac {
                "Not set — 4 for macOS, 2 for Linux"
            } else {
                "Not set — 2"
            },
        ),
        field(
            "memory_gb",
            "Memory per runner (GB)",
            "integer",
            "Not set — 8",
        ),
    ];
    if mac {
        pool.push(field(
            "disk_gb",
            "Disk per runner (GB)",
            "integer",
            "Not set — 50 for macOS, required for Linux",
        ));
    }
    pool.extend([
        field("diagnostics", "Diagnostics", "boolean", "Not set — off"),
        field(
            "diagnostic_directory",
            "Diagnostic directory",
            "text",
            "Not set — required with diagnostics",
        ),
        field(
            "diagnostic_raw_records",
            "Raw diagnostic records",
            "boolean",
            "Not set — off",
        ),
    ]);
    EditorAccess {
        reason,
        read_reason,
        stopped: snapshot.is_some_and(|snapshot| snapshot.agent.state == "stopped"),
        settings,
        pool,
        volume: if mac {
            Vec::new()
        } else {
            vec![
                field("suffix", "Volume suffix", "text", "Not set — required"),
                field("mount_path", "Mount path", "text", "Not set — required"),
            ]
        },
    }
}
