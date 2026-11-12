//! converts a JSON host configuration that passed today's rules into the TOML document
//! for the same host, and checks the document before returning its text: it must pass
//! the full TOML rules and describe the configuration the JSON file does.

use std::fmt;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use crate::json::{self, Object, Value};
use crate::profile::{Os, Profile};
use crate::runtime::clock::Clock;
use crate::runtime::name::civil_from_days;
use crate::validate::{
    Failure, Key, Plan, SCHEMA_VERSION, json_key_name, toml_key_name, validate, validate_toml,
};

const SECONDS_PER_DAY: u64 = 86_400;

/// why a configuration was not converted. no text comes with a failure, so a document
/// that failed its check is never written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionFailure {
    /// the JSON file does not pass the host's rules, which the conversion requires.
    SourceRejected(Failure),
    /// the converted document does not pass the TOML rules for the host.
    DocumentRejected(Failure),
    /// the converted document is valid but does not describe the JSON file's
    /// configuration; the text says where.
    NotEquivalent(String),
}

impl fmt::Display for ConversionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceRejected(failure) => {
                write!(
                    f,
                    "the JSON configuration does not pass the host's rules: {}",
                    problems(failure)
                )
            }
            Self::DocumentRejected(failure) => {
                write!(
                    f,
                    "the converted document does not pass the TOML rules: {}",
                    problems(failure)
                )
            }
            Self::NotEquivalent(difference) => write!(
                f,
                "the converted document does not describe the same configuration: {difference}"
            ),
        }
    }
}

impl std::error::Error for ConversionFailure {}

fn problems(failure: &Failure) -> String {
    match failure {
        Failure::Unreadable(problem) => problem.clone(),
        Failure::Invalid(problems) => problems.join("; "),
    }
}

/// converts the JSON configuration `json` of `host` to a TOML document, after
/// `validate` accepted it under `checkout`. the document opens with a comment that
/// names `source` by its file name and the time of `clock`, uses the host's line
/// ending, CRLF on Windows and LF on macOS, and carries no `tokenPath`.
///
/// the text is returned only when it passes the TOML rules for the host under the same
/// `checkout` and [`first_difference`] finds nothing between its configuration and the
/// JSON file's.
pub fn convert_to_toml(
    json: &[u8],
    host: Profile,
    source: &Path,
    checkout: Option<&Path>,
    clock: &impl Clock,
) -> Result<String, ConversionFailure> {
    convert_with(json, host, source, checkout, clock, body)
}

/// [`convert_to_toml`] with the document's body supplied, so a test can make the
/// check fail.
fn convert_with(
    json: &[u8],
    host: Profile,
    source: &Path,
    checkout: Option<&Path>,
    clock: &impl Clock,
    write_body: impl FnOnce(&Object, Profile) -> String,
) -> Result<String, ConversionFailure> {
    let accepted = validate(json, host, checkout)
        .outcome
        .map_err(ConversionFailure::SourceRejected)?;
    let Ok(Value::Object(root)) = json::parse(json) else {
        return Err(ConversionFailure::SourceRejected(Failure::Unreadable(
            "the configuration is not a JSON object".to_string(),
        )));
    };
    let document = format!(
        "{}{}",
        opening_comment(source, clock),
        write_body(&root, host)
    );
    let document = with_line_ending(&document, host);

    let converted = validate_toml(document.as_bytes(), host, checkout)
        .outcome
        .map_err(ConversionFailure::DocumentRejected)?;
    match first_difference(&accepted, &converted) {
        Some(difference) => Err(ConversionFailure::NotEquivalent(difference)),
        None => Ok(document),
    }
}

/// names where `converted`, a TOML file's configuration, differs from `original`, the
/// JSON file's, or `None` when they are the same configuration. the source path, the
/// token path and an `os` that TOML defaults to Linux are not compared, and the file's
/// own settings must be the host's routing label with nothing in `[lifecycle]`.
#[must_use]
pub fn first_difference(original: &Plan, converted: &Plan) -> Option<String> {
    if original.profile != converted.profile {
        return Some("the host differs".to_string());
    }
    let host = original.profile;
    match &converted.settings {
        Some(settings)
            if settings.schema_version == SCHEMA_VERSION
                && settings.routing_label == host.routing_label()
                && settings.lifecycle == Default::default() => {}
        _ => {
            return Some(format!(
                "the file's settings are not schema version {SCHEMA_VERSION}, the routing label {} and an empty lifecycle",
                host.routing_label()
            ));
        }
    }
    if original.image_name != converted.image_name {
        return Some(format!("{} differs", toml_key_name(Key::ImageName)));
    }
    if original.budget != converted.budget {
        return Some(format!("{} differs", toml_key_name(Key::Budget)));
    }
    if original.entries.len() != converted.entries.len() {
        return Some(format!(
            "the JSON file has {} pools, the document {}",
            original.entries.len(),
            converted.entries.len()
        ));
    }
    for (index, (from, to)) in original.entries.iter().zip(&converted.entries).enumerate() {
        let mut to = to.clone();
        to.path.clone_from(&from.path);
        to.token_path.clone_from(&from.token_path);
        if from.os.is_none() && to.os == Some(Os::Linux) {
            to.os = None;
        }
        if *from != to {
            return Some(format!("pool[{index}] ('{}') differs", from.name));
        }
    }
    None
}

/// the comment that opens the document: the source file's name, never its path, and
/// the time in UTC. the name is cut at either separator so that a Windows path gives
/// the same name on any machine, and a control character in it is replaced, so it
/// cannot end the comment.
fn opening_comment(source: &Path, clock: &impl Clock) -> String {
    let path = source.to_string_lossy();
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !matches!(*name, "" | "." | ".."))
        .unwrap_or("the JSON configuration");
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    let seconds = clock
        .now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    format!(
        "# Converted from {name} by Marshal at {}.\n\n",
        utc_time(seconds)
    )
}

/// writes seconds since the Unix epoch as `2026-11-04T22:26:51Z`.
fn utc_time(seconds: u64) -> String {
    let (year, month, day) = civil_from_days(seconds / SECONDS_PER_DAY);
    let in_day = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        in_day / 3600,
        in_day / 60 % 60,
        in_day % 60
    )
}

fn with_line_ending(document: &str, host: Profile) -> String {
    match host {
        Profile::Axpc => document.replace('\n', "\r\n"),
        Profile::Axmc => document.to_string(),
    }
}

/// the document after its opening comment: the keys the JSON file sets, under their
/// TOML names.
fn body(root: &Object, host: Profile) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} = {SCHEMA_VERSION}\n",
        toml_key_name(Key::SchemaVersion)
    ));
    out.push_str(&format!(
        "{} = {}\n",
        toml_key_name(Key::RoutingLabel),
        quote(host.routing_label())
    ));
    set(&mut out, root, Key::ImageName);
    if let Some(Value::Object(budget)) = member(root, json_key_name(Key::Budget)) {
        out.push_str(&format!("\n[{}]\n", toml_key_name(Key::Budget)));
        for key in [Key::Cpus, Key::MemoryGb, Key::DiskGb] {
            set(&mut out, budget, key);
        }
    }
    if let Some(Value::Array(pools)) = member(root, json_key_name(Key::Repositories)) {
        for pool in pools.iter().filter_map(as_object) {
            write_pool(&mut out, pool);
        }
    }
    out
}

fn write_pool(out: &mut String, pool: &Object) {
    out.push_str(&format!("\n[[{}]]\n", toml_key_name(Key::Repositories)));
    set(out, pool, Key::Name);
    if let (Some(Value::String(owner)), Some(Value::String(repository))) = (
        member(pool, json_key_name(Key::Owner)),
        member(pool, json_key_name(Key::Repository)),
    ) {
        out.push_str(&format!(
            "{} = {}\n",
            toml_key_name(Key::Repository),
            quote(&format!("{owner}/{repository}"))
        ));
    }
    for key in [
        Key::Slots,
        Key::Os,
        Key::Labels,
        Key::Cpus,
        Key::MemoryGb,
        Key::DiskGb,
        Key::Diagnostics,
        Key::DiagnosticDirectory,
        Key::DiagnosticRawRecords,
    ] {
        set(out, pool, key);
    }
    if let Some(Value::Array(volumes)) = member(pool, json_key_name(Key::Volumes)) {
        for volume in volumes.iter().filter_map(as_object) {
            out.push_str(&format!(
                "\n[[{}.{}]]\n",
                toml_key_name(Key::Repositories),
                toml_key_name(Key::Volumes)
            ));
            set(out, volume, Key::Suffix);
            set(out, volume, Key::MountPath);
        }
    }
}

fn as_object(value: &Value) -> Option<&Object> {
    match value {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

fn member<'a>(object: &'a Object, name: &str) -> Option<&'a Value> {
    object
        .iter()
        .find_map(|(member, value)| (member == name).then_some(value))
}

/// writes the key the object sets, under its TOML name; a value of a shape the schema
/// does not give the key is left out, which the check then reports.
fn set(out: &mut String, object: &Object, key: Key) {
    if let Some(text) = member(object, json_key_name(key)).and_then(literal) {
        out.push_str(&format!("{} = {text}\n", toml_key_name(key)));
    }
}

/// a scalar or an array of scalars as TOML writes it; a JSON number's literal is
/// already a TOML integer or float.
fn literal(value: &Value) -> Option<String> {
    match value {
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.literal().to_string()),
        Value::String(text) => Some(quote(text)),
        Value::Array(items) => {
            let items: Option<Vec<String>> = items.iter().map(literal).collect();
            items.map(|items| format!("[{}]", items.join(", ")))
        }
        Value::Null | Value::Object(_) => None,
    }
}

/// a TOML basic string.
fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use crate::runtime::fake::FakeClock;

    use super::*;

    const WINDOWS_FILE: &str = r#"{
        "imageName": "image:tag",
        "repositories": [
            { "owner": "octo", "repository": "hello", "name": "alpha", "slots": 2,
              "tokenPath": "C:\\tokens\\alpha.token", "volumes": [] },
            { "owner": "octo", "repository": "world", "name": "beta", "slots": 1,
              "tokenPath": "C:\\tokens\\beta.token", "volumes": [], "cpus": 1.5 }
        ]
    }"#;

    fn convert_with_body(
        write_body: impl FnOnce(&Object, Profile) -> String,
    ) -> Result<String, ConversionFailure> {
        convert_with(
            WINDOWS_FILE.as_bytes(),
            Profile::Axpc,
            Path::new("config.json"),
            None,
            &FakeClock::at(0),
            write_body,
        )
    }

    #[test]
    fn the_document_the_conversion_writes_passes_its_check() {
        assert!(convert_with_body(body).is_ok());
    }

    #[test]
    fn a_document_that_breaks_the_toml_rules_returns_the_reason_and_no_text() {
        let failure = convert_with_body(|root, host| format!("{}\nstray = 1\n", body(root, host)))
            .unwrap_err();
        assert!(matches!(failure, ConversionFailure::DocumentRejected(_)));
        assert_eq!(
            failure.to_string(),
            "the converted document does not pass the TOML rules: line 18: pool[1].stray: unknown key"
        );
    }

    #[test]
    fn a_syntax_error_in_the_document_returns_the_reason_and_no_text() {
        let failure =
            convert_with_body(|root, host| format!("{}\n[[\n", body(root, host))).unwrap_err();
        assert!(matches!(
            failure,
            ConversionFailure::DocumentRejected(Failure::Unreadable(_))
        ));
    }

    #[test]
    fn a_document_that_drops_a_pool_returns_the_difference_and_no_text() {
        let failure = convert_with_body(|root, host| {
            let text = body(root, host);
            let cut = text.rfind("\n[[pool]]").unwrap_or(text.len());
            text[..cut].to_string() + "\n"
        })
        .unwrap_err();
        assert_eq!(
            failure,
            ConversionFailure::NotEquivalent("the JSON file has 2 pools, the document 1".into())
        );
        assert_eq!(
            failure.to_string(),
            "the converted document does not describe the same configuration: \
             the JSON file has 2 pools, the document 1"
        );
    }

    #[test]
    fn a_document_that_changes_a_value_returns_the_difference_and_no_text() {
        let failure =
            convert_with_body(|root, host| body(root, host).replace("cpus = 1.5", "cpus = 2.5"))
                .unwrap_err();
        assert_eq!(
            failure,
            ConversionFailure::NotEquivalent("pool[1] ('beta') differs".into())
        );
    }

    #[test]
    fn a_document_that_changes_the_routing_label_is_not_the_hosts() {
        let failure = convert_with_body(|root, host| {
            body(root, host).replace("routing_label = \"axpc\"", "routing_label = \"other\"")
        })
        .unwrap_err();
        assert!(
            matches!(failure, ConversionFailure::NotEquivalent(_)),
            "{failure:?}"
        );
    }

    fn plan(json: &str) -> Plan {
        validate(json.as_bytes(), Profile::Axpc, None)
            .outcome
            .unwrap()
    }

    fn converted_plan(json: &str) -> Plan {
        let text = convert_with(
            json.as_bytes(),
            Profile::Axpc,
            Path::new("config.json"),
            None,
            &FakeClock::at(0),
            body,
        )
        .unwrap();
        validate_toml(text.as_bytes(), Profile::Axpc, None)
            .outcome
            .unwrap()
    }

    #[test]
    fn the_comparison_ignores_only_the_path_the_token_path_and_a_defaulted_os() {
        let original = plan(WINDOWS_FILE);
        let converted = converted_plan(WINDOWS_FILE);
        assert_eq!(first_difference(&original, &converted), None);
        // the conversion drops the token path and the TOML pool defaults its os
        assert!(
            original
                .entries
                .iter()
                .all(|entry| entry.token_path.is_some())
        );
        assert!(
            converted
                .entries
                .iter()
                .all(|entry| entry.token_path.is_none())
        );
        assert_eq!(original.entries.first().map(|entry| entry.os), Some(None));
        assert_eq!(
            converted.entries.first().map(|entry| entry.os),
            Some(Some(Os::Linux))
        );
    }

    type Change = fn(&mut Plan);

    #[test]
    fn the_comparison_finds_a_change_to_any_setting_of_a_pool() {
        let original = plan(WINDOWS_FILE);
        let converted = converted_plan(WINDOWS_FILE);
        let changes: [(&str, Change); 14] = [
            ("image name", |plan| {
                plan.image_name = Some("other:tag".into())
            }),
            ("budget", |plan| {
                plan.budget = Some(crate::validate::Budget {
                    cpus: 1.0,
                    memory_gb: 1,
                    disk_gb: 1,
                });
            }),
            ("owner", |plan| plan.entries[1].owner.push('x')),
            ("repository", |plan| plan.entries[1].repository.push('x')),
            ("name", |plan| plan.entries[1].name.push('x')),
            ("runners", |plan| plan.entries[1].slots += 1),
            ("labels", |plan| plan.entries[1].labels.push("x".into())),
            ("cpus", |plan| plan.entries[1].cpus += 1.0),
            ("memory", |plan| plan.entries[1].memory_gb += 1),
            ("volumes", |plan| {
                plan.entries[1]
                    .volumes
                    .push(crate::validate::PlannedVolume {
                        name: "beta-x".into(),
                        mount_path: "/x".into(),
                    });
            }),
            ("diagnostics", |plan| plan.entries[1].diagnostics = true),
            ("diagnostic directory", |plan| {
                plan.entries[1].diagnostic_directory = Some("C:\\d".into());
            }),
            ("raw records", |plan| {
                plan.entries[1].diagnostic_raw_records = true;
            }),
            ("disk", |plan| plan.entries[1].disk_gb = Some(1)),
        ];
        for (what, change) in changes {
            let mut changed = converted.clone();
            change(&mut changed);
            assert!(first_difference(&original, &changed).is_some(), "{what}");
        }
        let mut changed = converted.clone();
        changed.entries[1].os = Some(Os::Macos);
        assert!(first_difference(&original, &changed).is_some(), "os");
        let mut changed = converted;
        changed.settings = None;
        assert!(first_difference(&original, &changed).is_some(), "settings");
    }
}
