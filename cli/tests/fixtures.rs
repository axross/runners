//! runs the validator over every fixture under `tests/host-configuration/` and checks
//! it against `expectations.json`, which the PowerShell test and the parity harness
//! read too.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use marshal_cli::command::invalid_message;
use marshal_cli::de::Deserializer;
use marshal_cli::json;
use marshal_cli::profile::Profile;
use marshal_cli::summary::plan_summary;
use marshal_cli::validate::{Failure, validate};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expectation {
    profile: String,
    outcome: Outcome,
    contains: Vec<String>,
    lacks: Vec<String>,
    // the PowerShell validator's outcome, which the parity harness checks
    #[serde(rename = "powershell")]
    _powershell: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Accept,
    Reject,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/host-configuration")
}

fn expectations(root: &Path) -> BTreeMap<String, Expectation> {
    let bytes = fs::read(root.join("expectations.json")).unwrap();
    let tree = json::parse(&bytes).unwrap();
    BTreeMap::deserialize(Deserializer::new(&tree)).unwrap()
}

fn fixture_files(root: &Path, directory: &Path, found: &mut BTreeSet<String>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fixture_files(root, &path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path
                .file_name()
                .is_some_and(|name| name != "expectations.json")
        {
            let relative = path.strip_prefix(root).unwrap();
            found.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_fixture_has_an_expectation_and_every_expectation_a_fixture() {
    let root = fixture_root();
    let expected: BTreeSet<String> = expectations(&root).into_keys().collect();
    let mut found = BTreeSet::new();
    fixture_files(&root, &root, &mut found);
    let without_expectation: Vec<_> = found.difference(&expected).collect();
    let without_fixture: Vec<_> = expected.difference(&found).collect();
    assert!(
        without_expectation.is_empty() && without_fixture.is_empty(),
        "fixtures without an expectation: {without_expectation:?}; expectations without a fixture: {without_fixture:?}"
    );
}

#[test]
fn every_fixture_gets_its_expected_outcome_and_text() {
    let root = fixture_root();
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, expectation) in &expectations(&root) {
        let profile = Profile::from_name(&expectation.profile).unwrap();
        let path = root.join(name);
        let validation = validate(&fs::read(&path).unwrap(), profile, None);
        let (accepted, text) = match validation.outcome {
            Ok(plan) => (true, plan_summary(&plan).join("\n")),
            Err(Failure::Unreadable(problem)) => (
                false,
                format!("Host configuration is not valid JSON: {problem}"),
            ),
            Err(Failure::Invalid(errors)) => (false, invalid_message(&path, &errors)),
        };
        checked += 1;
        if accepted != (expectation.outcome == Outcome::Accept) {
            failures.push(format!(
                "{name}: expected {:?}, got {}\n{text}",
                expectation.outcome,
                if accepted { "accept" } else { "reject" }
            ));
            continue;
        }
        for wanted in &expectation.contains {
            if !text.contains(wanted) {
                failures.push(format!("{name}: the output lacks {wanted:?}\n{text}"));
            }
        }
        for unwanted in &expectation.lacks {
            if text.contains(unwanted) {
                failures.push(format!("{name}: the output has {unwanted:?}\n{text}"));
            }
        }
    }
    assert!(checked > 100, "only {checked} fixtures were checked");
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn an_expectation_with_an_unknown_or_misspelled_key_fails_the_read() {
    let valid = r#"{ "profile": "axpc", "outcome": "accept", "contains": [], "lacks": [] }"#;
    let read = |text: &str| {
        Expectation::deserialize(Deserializer::new(&json::parse(text.as_bytes()).unwrap()))
    };
    assert!(read(valid).is_ok());
    for text in [
        r#"{ "profile": "axpc", "outcome": "accept", "contains": [], "lacks": [], "extra": 1 }"#,
        r#"{ "profile": "axpc", "outcome": "accept", "Contains": [], "lacks": [] }"#,
        r#"{ "profile": "axpc", "outcome": "accpet", "contains": [], "lacks": [] }"#,
    ] {
        assert!(read(text).is_err(), "{text}");
    }
}
