//! the footer: what the image check found, its freshness warnings, where the
//! configuration is and whether it is the one in use, and that closing the window
//! keeps Marshal running.

use marshal_cli::agent::snapshot::{
    ConfigurationInfo, ImageInfo, ImageLine, ImageWarning, WarningKind,
};
use serde::Serialize;

use crate::banner::built_just_before_the_stop;
use crate::host::HostWords;
use crate::state::Context;
use crate::text::{MIGRATE_ON_NEXT_START, plain};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Footer {
    /// the GitHub request budget, or why there is no GitHub status, first; then the
    /// runners whose GitHub state an oversize value left out.
    pub github: Vec<String>,
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

fn image_line(index: usize, image: &ImageInfo, words: &HostWords) -> String {
    if value(&image.lines, "golden-vm-version").is_some() {
        return golden_vm_line(image);
    }
    let label = if index == 0 {
        crate::text::capitalised(words.image)
    } else {
        format!(
            "{} {}",
            crate::text::capitalised(words.image_short),
            plain(&image.name)
        )
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

/// a golden VM that M3's entry names by its own lines, such as the golden macOS VM:
/// "Golden macOS VM: <name> · version 3 · runner 2.330.0 · base published 2026-10-01".
fn golden_vm_line(image: &ImageInfo) -> String {
    let known = [
        "name",
        "golden-vm-version",
        "runner-version",
        "base-published",
    ];
    let mut parts = Vec::new();
    if let Some(name) = value(&image.lines, "name") {
        parts.push(plain(name));
    }
    if let Some(version) = value(&image.lines, "golden-vm-version") {
        parts.push(format!("version {}", plain(version)));
    }
    if let Some(runner) = value(&image.lines, "runner-version") {
        parts.push(format!("runner {}", plain(runner)));
    }
    if let Some(published) = value(&image.lines, "base-published") {
        parts.push(format!("base published {}", plain(published)));
    }
    let other = rest(&image.lines, &known);
    if !other.is_empty() {
        parts.push(other);
    }
    format!(
        "{}: {}",
        crate::text::capitalised(&plain(&image.name)),
        parts.join(" · ")
    )
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

/// a freshness warning in the words of the boards, which name no command; `image` is
/// what the warning is about, such as "golden VM" or M3's "golden macOS VM".
fn warning(kind: &WarningKind, warning: &ImageWarning, image: &str) -> Option<String> {
    Some(match kind {
        WarningKind::NotBuiltByMarshal => {
            format!("Warning: the {image} was not built by Marshal, so its build age is unknown.")
        }
        WarningKind::PackagesOld => format!(
            "Warning: the {image}'s operating-system packages are {} days old; build the {image} again.",
            days(warning)
        ),
        WarningKind::ImageOld => format!(
            "Warning: the {image} is {} days old and should be rebuilt.",
            days(warning)
        ),
        WarningKind::RunnerOld => format!(
            "Warning: {} was released {} days ago, so each job's runner likely updates itself at start; update Marshal to get a newer runner, then build the {image} again.",
            version(warning),
            days(warning)
        ),
        WarningKind::RunnerMayBeNewer => format!(
            "Warning: {} was released {} days ago, and a newer runner may exist.",
            version(warning),
            days(warning)
        ),
        WarningKind::RunnerAgeUnknown => format!(
            "Warning: the {image}'s {} has no known release date, so its age is unknown.",
            version(warning)
        ),
        WarningKind::BaseOld => format!(
            "Warning: the {image}'s macOS base was published {} days ago; update Marshal to get a newer base, then build the {image} again.",
            days(warning)
        ),
        _ => return None,
    })
}

fn migration_state(configuration: &ConfigurationInfo) -> Option<&str> {
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
    let file_state = || {
        let problems: Vec<String> = configuration.problems.iter().map(|p| plain(p)).collect();
        match configuration.status.as_str() {
            "missing" => "is missing".to_string(),
            "unreadable" => format!("cannot be read ({})", problems.join("; ")),
            _ if problems.is_empty() => "cannot be used".to_string(),
            _ => format!("is not valid ({})", problems.join("; ")),
        }
    };
    match in_use.source.as_str() {
        "last_valid" => warnings.push(format!(
            "Warning: config.toml {}, so Marshal runs the configuration it read {read}. \
             Fix the file, then choose Stop All and Start All.",
            file_state()
        )),
        // config.toml appeared after a migration that could not write it
        "json" if configuration.status != "missing" => warnings.push(format!(
            "Warning: config.toml {}, so Marshal runs the configuration of config.json it \
             read {read}. Fix the file, then choose Stop All and Start All.",
            file_state()
        )),
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
        vec![format!(
            "{}: not checked",
            crate::text::capitalised(context.input.words.image)
        )]
    } else {
        images
            .iter()
            .enumerate()
            .map(|(index, image)| image_line(index, image, &context.input.words))
            .collect()
    };
    let mut warnings: Vec<String> = images
        .iter()
        .enumerate()
        .flat_map(|(index, image)| {
            let subject = if index == 0 {
                context.input.words.image_short.to_string()
            } else {
                plain(&image.name)
            };
            image
                .warnings
                .iter()
                .filter_map(move |each| warning(&each.kind(), each, &subject))
        })
        .collect();
    warnings.extend(configuration_warnings(context));
    if built_just_before_the_stop(context, "cancelled") {
        warnings.push(format!(
            "The build was stopped. Check {}: a stopped build may already have replaced it.",
            context.image_name.as_deref().map_or_else(
                || format!("the {}", context.input.words.image),
                |name| format!("the {} {name}", context.input.words.image_short)
            )
        ));
    }
    Footer {
        github: crate::github::footer_lines(context),
        image_lines,
        warnings,
        configuration: configuration_line(context),
        note: context.input.words.close_note.to_string(),
    }
}
