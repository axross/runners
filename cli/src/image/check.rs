//! The check of a built image: its labels, the four outcomes they sort into, and
//! the freshness warnings and `status` lines they give.
//!
//! The backend reads the labels; this module only interprets them, so the same
//! rules apply whichever backend reads them and however the clock is injected.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::json::{self, Value};
use crate::runtime::name::civil_from_days;

use super::contract::{
    BUILT_LABEL, CONTRACT_LABEL, CONTRACT_VERSION, RUNNER_RELEASED_LABEL, RUNNER_VERSION_LABEL,
};

/// The bound on both clients of one image check, together: the inspect and, when
/// it fails, the listing. Re-validation and the check fit in a resuming `start`'s
/// 10 s wait, and the container steps and the check in `restart`'s 35 s.
pub const IMAGE_CHECK_BUDGET: std::time::Duration = std::time::Duration::from_secs(4);

/// The most bytes of an inspect's output read.
pub const MAX_INSPECT_BYTES: usize = 64 * 1024;

const SECONDS_PER_DAY: u64 = 86_400;

/// An age from this many days gives the first warning.
const FIRST_WARNING_DAYS: u64 = 7;

/// An age from this many days gives the second warning.
const SECOND_WARNING_DAYS: u64 = 21;

/// The longest label value `status` shows as it is; a longer one, or one that is
/// not plain text, shows as malformed.
const MAX_SHOWN_VALUE: usize = 32;

/// The labels the check reads, in the order the inspect format prints them.
const READ_LABELS: [&str; 4] = [
    CONTRACT_LABEL,
    BUILT_LABEL,
    RUNNER_VERSION_LABEL,
    RUNNER_RELEASED_LABEL,
];

/// The Go template `docker image inspect` prints the labels with: a JSON array of
/// their values, where a missing label prints as an empty string.
#[must_use]
pub fn labels_format() -> String {
    let values: Vec<String> = READ_LABELS
        .iter()
        .map(|label| format!("{{{{json (index .Config.Labels \"{label}\")}}}}"))
        .collect();
    format!("[{}]", values.join(","))
}

/// The image labels the check reads; `None` for one the image does not carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageLabels {
    pub contract: Option<String>,
    pub built: Option<String>,
    pub runner_version: Option<String>,
    pub runner_released: Option<String>,
}

impl ImageLabels {
    /// Reads the output of an inspect with [`labels_format`], or `None` when it is
    /// not what that format prints.
    #[must_use]
    pub fn from_inspect(output: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(output).ok()?.trim_end();
        let Ok(Value::Array(values)) = json::parse(text.as_bytes()) else {
            return None;
        };
        let mut read = values.into_iter().map(|value| match value {
            Value::String(text) if text.is_empty() => Some(None),
            Value::String(text) => Some(Some(text)),
            _ => None,
        });
        let labels = Self {
            contract: read.next()??,
            built: read.next()??,
            runner_version: read.next()??,
            runner_released: read.next()??,
        };
        read.next().is_none().then_some(labels)
    }
}

/// What one check of the configured image found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageCheck {
    /// The image's contract label is this CLI's.
    Matching(ImageLabels),
    /// The image exists, but its contract label is missing, malformed or another.
    Mismatched(ImageLabels),
    /// No image has the name.
    Missing,
    /// Docker could not say, within the bound; never read as a mismatch.
    Unavailable(String),
}

impl ImageCheck {
    /// Sorts the labels of an image that exists.
    #[must_use]
    pub fn of_labels(labels: ImageLabels) -> Self {
        if labels.contract.as_deref().and_then(contract_number) == Some(CONTRACT_VERSION) {
            Self::Matching(labels)
        } else {
            Self::Mismatched(labels)
        }
    }

    /// Why the image cannot run jobs, naming it and both contract versions, or
    /// `None` when it can, or when Docker could not say.
    #[must_use]
    pub fn refusal(&self, image: &str) -> Option<String> {
        match self {
            Self::Matching(_) | Self::Unavailable(_) => None,
            Self::Mismatched(labels) => Some(format!(
                "the image {image} has image contract {}, but this CLI needs image contract {CONTRACT_VERSION}",
                shown_contract(labels)
            )),
            Self::Missing => Some(format!(
                "the image {image} is missing; this CLI needs one with image contract {CONTRACT_VERSION}"
            )),
        }
    }

    fn labels(&self) -> Option<&ImageLabels> {
        match self {
            Self::Matching(labels) | Self::Mismatched(labels) => Some(labels),
            Self::Missing | Self::Unavailable(_) => None,
        }
    }
}

/// The reference the check reads: the name as `docker run` resolves it, so a name
/// whose last path part has no tag or digest is `<name>:latest`.
#[must_use]
pub fn checked_reference(image: &str) -> String {
    let last = image.rsplit('/').next().unwrap_or(image);
    if last.contains([':', '@']) {
        image.to_string()
    } else {
        format!("{image}:latest")
    }
}

fn contract_number(text: &str) -> Option<u32> {
    let digits = text.bytes().all(|byte| byte.is_ascii_digit()) && !text.starts_with('0');
    digits.then(|| text.parse().ok()).flatten()
}

/// A label value as `status` and a refusal show it.
fn shown(value: &str) -> &str {
    let plain = value.len() <= MAX_SHOWN_VALUE && value.bytes().all(|byte| byte.is_ascii_graphic());
    if plain { value } else { "malformed" }
}

fn shown_contract(labels: &ImageLabels) -> &str {
    labels.contract.as_deref().map_or("none", shown)
}

/// The build time of a `built` label, in Unix seconds.
fn built_seconds(labels: &ImageLabels) -> Option<u64> {
    let text = labels.built.as_deref()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The runner version and the Unix seconds of its release day's start, when both
/// labels are well formed.
fn runner_release(labels: &ImageLabels) -> Option<(&str, u64)> {
    let version = labels.runner_version.as_deref()?;
    if shown(version) != version {
        return None;
    }
    let date = labels.runner_released.as_deref()?;
    Some((version, date_seconds(date)?))
}

/// The start of a `YYYY-MM-DD` day in UTC, in Unix seconds, from 1970 on.
fn date_seconds(text: &str) -> Option<u64> {
    let mut parts = text.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let number = |part: &str, width: usize| -> Option<u64> {
        (part.len() == width && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let date = (number(year, 4)?, number(month, 2)?, number(day, 2)?);
    let (year, month, day) = date;
    if year < 1970 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // a day past its month's end lands in the next month, so the round trip
    // refuses 2026-02-30.
    let days = days_from_civil(year, month, day);
    (civil_from_days(days) == date).then_some(days * SECONDS_PER_DAY)
}

/// Converts a proleptic Gregorian date from 1970 on to days since 1970-01-01
/// (Howard Hinnant's `days_from_civil`), the inverse of `civil_from_days`.
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146_097 + day_of_era).saturating_sub(719_468)
}

/// Whole days from `then` to `now`, rounded down; none when `then` is later.
#[must_use]
pub fn age_days(now: SystemTime, then: u64) -> u64 {
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    now.saturating_sub(then) / SECONDS_PER_DAY
}

/// The freshness warnings for the image a check found, as of `now`.
#[must_use]
pub fn freshness_warnings(check: &ImageCheck, now: SystemTime) -> Vec<String> {
    let Some(labels) = check.labels() else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    match built_seconds(labels).map(|built| age_days(now, built)) {
        None => warnings.push(
            "the image was not built by `runners build`, so its build age is unknown".to_string(),
        ),
        Some(days) if days >= SECOND_WARNING_DAYS => warnings.push(format!(
            "the image's operating-system packages are {days} days old; rebuild it with `runners build`"
        )),
        Some(days) if days >= FIRST_WARNING_DAYS => warnings.push(format!(
            "the image is {days} days old and should be rebuilt with `runners build`"
        )),
        Some(_) => {}
    }
    if let Some((version, released)) = runner_release(labels) {
        let days = age_days(now, released);
        if days >= SECOND_WARNING_DAYS {
            warnings.push(format!(
                "runner {version} was released {days} days ago, so each job's runner likely updates \
                 itself at start; update the CLI to get a newer runner, then rebuild the image"
            ));
        } else if days >= FIRST_WARNING_DAYS {
            warnings.push(format!(
                "runner {version} was released {days} days ago, and a newer runner may exist"
            ));
        }
    }
    warnings
}

/// The `status` lines for `image`, as of its latest check, or of none.
#[must_use]
pub fn status_fields(
    image: &str,
    check: Option<&ImageCheck>,
    now: SystemTime,
) -> Vec<(&'static str, String)> {
    let contract = match check {
        None | Some(ImageCheck::Unavailable(_)) => "not checked".to_string(),
        Some(ImageCheck::Matching(_)) => CONTRACT_VERSION.to_string(),
        Some(ImageCheck::Mismatched(labels)) => format!(
            "mismatch (image {}, CLI {CONTRACT_VERSION})",
            shown_contract(labels)
        ),
        Some(ImageCheck::Missing) => "missing".to_string(),
    };
    let labels = check.and_then(ImageCheck::labels);
    let built = labels.and_then(built_seconds).map_or_else(
        || "unknown".to_string(),
        |built| format!("{} days ago", age_days(now, built)),
    );
    let runner = labels.and_then(runner_release).map_or_else(
        || "unknown".to_string(),
        |(version, released)| format!("{version}, released {} days ago", age_days(now, released)),
    );
    let mut fields = vec![
        ("image", image.to_string()),
        ("image-contract", contract),
        ("image-built", built),
        ("runner", runner),
    ];
    if let Some(check) = check {
        fields.extend(
            freshness_warnings(check, now)
                .into_iter()
                .map(|warning| ("warning", warning)),
        );
    }
    fields
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn labels(contract: &str) -> ImageLabels {
        ImageLabels {
            contract: Some(contract.to_string()),
            built: Some("1000".to_string()),
            runner_version: Some("2.337.0".to_string()),
            runner_released: Some("2026-08-26".to_string()),
        }
    }

    #[test]
    fn the_format_prints_one_json_value_per_label() {
        assert_eq!(
            labels_format(),
            "[{{json (index .Config.Labels \"com.github.axross.runners.contract\")}},\
             {{json (index .Config.Labels \"com.github.axross.runners.built\")}},\
             {{json (index .Config.Labels \"com.github.axross.runners.runner-version\")}},\
             {{json (index .Config.Labels \"com.github.axross.runners.runner-released\")}}]"
        );
    }

    #[test]
    fn the_inspect_output_reads_as_labels_with_empty_ones_missing() {
        assert_eq!(
            ImageLabels::from_inspect(b"[\"1\",\"1000\",\"2.337.0\",\"2026-08-26\"]\n"),
            Some(labels("1"))
        );
        assert_eq!(
            ImageLabels::from_inspect(b"[\"\",\"\",\"\",\"\"]"),
            Some(ImageLabels::default())
        );
        for unreadable in [
            &b"[\"1\",\"1000\",\"2.337.0\"]"[..],
            b"[\"1\",\"1000\",\"2.337.0\",\"x\",\"y\"]",
            b"[1,\"1000\",\"2.337.0\",\"x\"]",
            b"{}",
            b"",
            b"\xff",
        ] {
            assert_eq!(ImageLabels::from_inspect(unreadable), None);
        }
    }

    #[test]
    fn only_this_clis_contract_matches() {
        assert!(matches!(
            ImageCheck::of_labels(labels(&CONTRACT_VERSION.to_string())),
            ImageCheck::Matching(_)
        ));
        let other = (CONTRACT_VERSION + 1).to_string();
        let leading_zero = format!("0{CONTRACT_VERSION}");
        for contract in [other.as_str(), leading_zero.as_str(), "", "one", " 1"] {
            assert!(
                matches!(
                    ImageCheck::of_labels(labels(contract)),
                    ImageCheck::Mismatched(_)
                ),
                "{contract:?}"
            );
        }
        assert!(matches!(
            ImageCheck::of_labels(ImageLabels::default()),
            ImageCheck::Mismatched(_)
        ));
    }

    #[test]
    fn a_refusal_names_the_image_and_both_contracts_or_says_it_is_missing() {
        let mismatched = ImageCheck::of_labels(labels("7"));
        assert_eq!(
            mismatched.refusal("runner:local").unwrap(),
            format!(
                "the image runner:local has image contract 7, but this CLI needs image contract {CONTRACT_VERSION}"
            )
        );
        let unlabelled = ImageCheck::of_labels(ImageLabels::default());
        assert!(
            unlabelled
                .refusal("x")
                .unwrap()
                .contains("image contract none,")
        );
        assert!(
            ImageCheck::Missing
                .refusal("runner:local")
                .unwrap()
                .starts_with("the image runner:local is missing")
        );
        assert_eq!(
            ImageCheck::Unavailable("slow".to_string()).refusal("x"),
            None
        );
    }

    #[test]
    fn a_name_without_a_tag_is_checked_as_latest() {
        assert_eq!(checked_reference("runner"), "runner:latest");
        assert_eq!(checked_reference("runner:local"), "runner:local");
        assert_eq!(
            checked_reference("localhost:5000/team/runner"),
            "localhost:5000/team/runner:latest"
        );
        assert_eq!(
            checked_reference("localhost:5000/team/runner:v2"),
            "localhost:5000/team/runner:v2"
        );
        assert_eq!(checked_reference("runner@sha256:abc"), "runner@sha256:abc");
    }

    #[test]
    fn a_release_date_is_its_day_at_midnight_utc() {
        assert_eq!(date_seconds("1970-01-01"), Some(0));
        assert_eq!(date_seconds("2026-08-26"), Some(1_787_702_400));
        assert_eq!(date_seconds("2024-02-29"), Some(1_709_164_800));
        for refused in [
            "2026-02-30",
            "2025-02-29",
            "2026-13-01",
            "2026-8-26",
            "26-08-26",
            "1969-12-31",
            "2026-08-26x",
            "",
        ] {
            assert_eq!(date_seconds(refused), None, "{refused}");
        }
    }

    /// The warning an age gives, as `none`, `7-day` or `21-day`, for the build age
    /// and the runner release age, each measured while the other is a day old.
    fn warning_kinds(days: u64) -> (String, String) {
        let released = date_seconds("2026-08-26").unwrap();
        // a moment before the next whole day, so the age rounds down
        let age = days * SECONDS_PER_DAY + SECONDS_PER_DAY - 1;
        let warnings = |built: u64, now: u64| {
            let mut found = labels(&CONTRACT_VERSION.to_string());
            found.built = Some(built.to_string());
            freshness_warnings(&ImageCheck::of_labels(found), at(now))
        };
        let kind = |warnings: &[String], prefix: &str| -> String {
            match warnings.iter().find(|warning| warning.starts_with(prefix)) {
                None => "none".to_string(),
                Some(warning)
                    if warning.contains("operating-system") || warning.contains("updates") =>
                {
                    "21-day".to_string()
                }
                Some(_) => "7-day".to_string(),
            }
        };
        let now = released + SECONDS_PER_DAY;
        let build = kind(&warnings(now - age, now), "the image");
        let now = released + age;
        let runner = kind(&warnings(now - SECONDS_PER_DAY, now), "runner ");
        (build, runner)
    }

    #[test]
    fn ages_of_6_7_20_and_21_days_give_none_7_7_and_21_day_warnings() {
        for (days, expected) in [(6, "none"), (7, "7-day"), (20, "7-day"), (21, "21-day")] {
            let (build, runner) = warning_kinds(days);
            assert_eq!(build, expected, "build age {days}");
            assert_eq!(runner, expected, "runner release age {days}");
        }
    }

    #[test]
    fn the_warnings_name_their_ages() {
        let mut found = labels(&CONTRACT_VERSION.to_string());
        found.built = Some("0".to_string());
        found.runner_released = Some("1970-01-01".to_string());
        let warnings = freshness_warnings(&ImageCheck::of_labels(found), at(8 * SECONDS_PER_DAY));
        assert_eq!(
            warnings,
            [
                "the image is 8 days old and should be rebuilt with `runners build`",
                "runner 2.337.0 was released 8 days ago, and a newer runner may exist",
            ]
        );
    }

    #[test]
    fn an_image_without_a_build_time_has_an_unknown_build_age_and_its_warning() {
        let mut found = labels(&CONTRACT_VERSION.to_string());
        found.built = None;
        let check = ImageCheck::of_labels(found);
        let now = at(date_seconds("2026-08-27").unwrap());
        let fields = status_fields("runner:local", Some(&check), now);
        assert_eq!(
            fields,
            [
                ("image", "runner:local".to_string()),
                ("image-contract", CONTRACT_VERSION.to_string()),
                ("image-built", "unknown".to_string()),
                ("runner", "2.337.0, released 1 days ago".to_string()),
                (
                    "warning",
                    "the image was not built by `runners build`, so its build age is unknown"
                        .to_string()
                ),
            ]
        );
    }

    #[test]
    fn before_the_first_check_the_contract_is_not_checked_and_the_ages_unknown() {
        assert_eq!(
            status_fields("runner:local", None, at(0)),
            [
                ("image", "runner:local".to_string()),
                ("image-contract", "not checked".to_string()),
                ("image-built", "unknown".to_string()),
                ("runner", "unknown".to_string()),
            ]
        );
    }

    #[test]
    fn a_mismatch_and_a_missing_image_read_as_status_shows_them() {
        let mismatched = ImageCheck::of_labels(labels("7"));
        let fields = status_fields("x", Some(&mismatched), at(1000));
        assert_eq!(
            fields[1],
            (
                "image-contract",
                format!("mismatch (image 7, CLI {CONTRACT_VERSION})")
            )
        );
        assert_eq!(fields[2], ("image-built", "0 days ago".to_string()));
        let missing = status_fields("x", Some(&ImageCheck::Missing), at(1000));
        assert_eq!(missing[1], ("image-contract", "missing".to_string()));
        assert_eq!(missing[2], ("image-built", "unknown".to_string()));
        assert_eq!(missing[3], ("runner", "unknown".to_string()));
        assert_eq!(missing.len(), 4);
        let mut odd = labels(&"9".repeat(40));
        odd.runner_version = Some("bad version".to_string());
        let fields = status_fields("x", Some(&ImageCheck::of_labels(odd)), at(1000));
        assert_eq!(
            fields[1],
            (
                "image-contract",
                format!("mismatch (image malformed, CLI {CONTRACT_VERSION})")
            )
        );
        assert_eq!(fields[3], ("runner", "unknown".to_string()));
    }
}
