//! the footer: what the image check found, its freshness warnings, where the
//! configuration is and whether it is the one in use, and that closing the window
//! keeps Marshal running.

use marshal_cli::agent::snapshot::{
    ConfigurationInfo, ImageInfo, ImageLine, ImageWarning, WarningKind,
};
use serde::Serialize;

use crate::banner::built_just_before_the_stop;
use crate::state::Context;
use crate::text::{MIGRATE_ON_NEXT_START, plain};

const NOTE: &str = "Closing this window keeps Marshal running in the notification area.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Footer {
    /// one line for each image the agent tracks.
    pub image_lines: Vec<String>,
    pub warnings: Vec<String>,
    pub configuration: String,
    pub note: String,
}

fn value<'a>(lines: &'a [ImageLine], name: &str) -> Option<&'a str> {
    lines
        .iter()
        .find(|line| line.name == name)
        .map(|line| line.value.as_str())
}

/// what the image check found of the contract, in the words of the boards:
/// "contract 3", "missing", "not checked" or "mismatch (image 2, CLI 3)".
pub fn contract(context: &Context<'_>) -> Option<String> {
    let image = context.snapshot?.images.first()?;
    value(&image.lines, "image-contract").map(contract_text)
}

fn contract_text(value: &str) -> String {
    if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
        format!("contract {}", plain(value))
    } else {
        plain(value)
    }
}

fn image_line(index: usize, image: &ImageInfo) -> String {
    let label = if index == 0 {
        "Runner image".to_string()
    } else {
        format!("Image {}", plain(&image.name))
    };
    let known = ["image", "image-contract", "image-built", "runner"];
    let Some(contract) = value(&image.lines, "image-contract") else {
        return format!("{label}: {}", rest(&image.lines, &[]));
    };
    let mut parts = vec![contract_text(contract)];
    if !matches!(contract, "missing" | "not checked") {
        if let Some(built) = value(&image.lines, "image-built") {
            parts.push(format!("built {}", plain(built)));
        }
        if let Some(runner) = value(&image.lines, "runner") {
            parts.push(format!("runner {}", plain(runner)));
        }
    }
    let other = rest(&image.lines, &known);
    if !other.is_empty() {
        parts.push(other);
    }
    format!("{label}: {}", parts.join(" · "))
}

/// the lines whose names the reader does not know, as "name value".
fn rest(lines: &[ImageLine], known: &[&str]) -> String {
    lines
        .iter()
        .filter(|line| !known.contains(&line.name.as_str()))
        .map(|line| format!("{} {}", plain(&line.name), plain(&line.value)))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn days(warning: &ImageWarning) -> String {
    warning
        .days
        .map_or_else(|| "some".to_string(), |days| days.to_string())
}

fn version(warning: &ImageWarning) -> String {
    warning.version.as_deref().map_or_else(
        || "the runner".to_string(),
        |version| format!("runner {}", plain(version)),
    )
}

/// a freshness warning in the words of the boards, which name no command.
fn warning(kind: &WarningKind, warning: &ImageWarning) -> Option<String> {
    Some(match kind {
        WarningKind::NotBuiltByMarshal => {
            "Warning: the image was not built by Marshal, so its build age is unknown.".to_string()
        }
        WarningKind::PackagesOld => format!(
            "Warning: the image's operating-system packages are {} days old; build the image again.",
            days(warning)
        ),
        WarningKind::ImageOld => format!(
            "Warning: the image is {} days old and should be rebuilt.",
            days(warning)
        ),
        WarningKind::RunnerOld => format!(
            "Warning: {} was released {} days ago, so each job's runner likely updates itself at start; update Marshal to get a newer runner, then build the image again.",
            version(warning),
            days(warning)
        ),
        WarningKind::RunnerMayBeNewer => format!(
            "Warning: {} was released {} days ago, and a newer runner may exist.",
            version(warning),
            days(warning)
        ),
        _ => return None,
    })
}

fn migration_state<'a>(configuration: &'a ConfigurationInfo) -> Option<&'a str> {
    configuration
        .migration
        .as_ref()
        .map(|migration| migration.state.as_str())
}

/// where the configuration is, and when it was migrated from `config.json` by this
/// agent.
fn configuration_line(context: &Context<'_>) -> String {
    let mut line = format!("Configuration: {}", context.configuration_path);
    let migrated = context
        .snapshot
        .and_then(|snapshot| snapshot.configuration.migration.as_ref())
        .filter(|migration| migration.state == "migrated");
    if let Some(migration) = migrated {
        let when = migration.at.map_or_else(String::new, |at| {
            format!(" {}", context.input.clock.when_with_day(at))
        });
        line.push_str(&format!(
            ", migrated from config.json{when}; config.json is no longer read"
        ));
    }
    line
}

/// the warnings about the configuration while the agent holds one that is not the
/// file's: the file's problems, a migration that failed, and one that waits for the
/// next start.
fn configuration_warnings(context: &Context<'_>) -> Vec<String> {
    let Some(configuration) = context.snapshot.map(|snapshot| &snapshot.configuration) else {
        return Vec::new();
    };
    let Some(in_use) = &configuration.in_use else {
        return Vec::new();
    };
    let read = context.input.clock.when_with_day(in_use.read_at);
    let mut warnings = Vec::new();
    match in_use.source.as_str() {
        "last_valid" => {
            let state = if configuration.status == "missing" {
                "is missing".to_string()
            } else {
                let problems: Vec<String> =
                    configuration.problems.iter().map(|p| plain(p)).collect();
                if problems.is_empty() {
                    "cannot be used".to_string()
                } else {
                    format!("is not valid ({})", problems.join("; "))
                }
            };
            warnings.push(format!(
                "Warning: config.toml {state}, so Marshal runs the configuration it read {read}. \
                 Fix the file, then choose Stop All and Start All."
            ));
        }
        "json" => {
            let why = configuration
                .migration
                .as_ref()
                .and_then(|migration| migration.reason.as_deref())
                .map_or_else(|| "config.toml was not written".to_string(), plain);
            warnings.push(format!(
                "Warning: {why}. Marshal runs the configuration of config.json until it \
                 next starts, and tries again then."
            ));
        }
        _ => {}
    }
    if migration_state(configuration) == Some("pending") {
        warnings.push(MIGRATE_ON_NEXT_START.to_string());
    }
    warnings
}

pub fn footer(context: &Context<'_>) -> Footer {
    let images = context
        .snapshot
        .map(|snapshot| snapshot.images.as_slice())
        .unwrap_or_default();
    let image_lines = if images.is_empty() {
        vec!["Runner image: not checked".to_string()]
    } else {
        images
            .iter()
            .enumerate()
            .map(|(index, image)| image_line(index, image))
            .collect()
    };
    let mut warnings: Vec<String> = images
        .iter()
        .flat_map(|image| image.warnings.iter())
        .filter_map(|each| warning(&each.kind(), each))
        .collect();
    warnings.extend(configuration_warnings(context));
    if built_just_before_the_stop(context, "cancelled") {
        warnings.push(format!(
            "The build was stopped. Check {}: a stopped build may already have replaced it.",
            context.image_name.as_deref().map_or_else(
                || "the runner image".to_string(),
                |name| format!("the image {name}")
            )
        ));
    }
    Footer {
        image_lines,
        warnings,
        configuration: configuration_line(context),
        note: NOTE.to_string(),
    }
}
