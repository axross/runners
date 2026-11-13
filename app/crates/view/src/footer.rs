//! the footer: what the image check found, its freshness warnings, where the
//! configuration is, and that closing the window keeps Marshal running.

use marshal_cli::agent::snapshot::{ImageInfo, ImageLine, ImageWarning, WarningKind};
use serde::Serialize;

use crate::banner::built_just_before_the_stop;
use crate::state::Context;
use crate::text::plain;

const NOTE: &str = "Closing this window keeps Marshal running in the notification area.";

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
        github: crate::github::footer_lines(context),
        image_lines,
        warnings,
        configuration: format!("Configuration: {}", context.configuration_path),
        note: NOTE.to_string(),
    }
}
