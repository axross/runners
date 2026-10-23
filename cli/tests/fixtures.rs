//! Runs the validator over every fixture under `tests/host-configuration/` and checks
//! it against `expectations.json`, the file the PowerShell test and the parity
//! harness read too.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use runners_cli::command::invalid_message;
use runners_cli::json::{self, Object, Value};
use runners_cli::profile::Profile;
use runners_cli::summary::plan_summary;
use runners_cli::validate::{Failure, validate};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/host-configuration")
}

fn strings(node: &Object, name: &str) -> Vec<String> {
    match node.get(name) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                other => panic!("{name} holds a non-string: {other:?}"),
            })
            .collect(),
        other => panic!("{name} is not a list: {other:?}"),
    }
}

fn text_of(node: &Object, name: &str) -> String {
    match node.get(name) {
        Some(Value::String(text)) => text.clone(),
        other => panic!("{name} is not a string: {other:?}"),
    }
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
    let expectations = fs::read(root.join("expectations.json")).unwrap();
    let Value::Object(expectations) = json::parse(&expectations).unwrap() else {
        panic!("expectations.json is not an object");
    };
    let expected: BTreeSet<String> = expectations
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
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
    let expectations = fs::read(root.join("expectations.json")).unwrap();
    let Value::Object(expectations) = json::parse(&expectations).unwrap() else {
        panic!("expectations.json is not an object");
    };
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, expectation) in expectations.iter() {
        let Value::Object(expectation) = expectation else {
            panic!("{name}: the expectation is not an object");
        };
        let profile = Profile::from_name(&text_of(expectation, "profile")).unwrap();
        let outcome = text_of(expectation, "outcome");
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
        if accepted != (outcome == "accept") {
            failures.push(format!(
                "{name}: expected {outcome}, got {}\n{text}",
                if accepted { "accept" } else { "reject" }
            ));
            continue;
        }
        for wanted in strings(expectation, "contains") {
            if !text.contains(&wanted) {
                failures.push(format!("{name}: the output lacks {wanted:?}\n{text}"));
            }
        }
        for unwanted in strings(expectation, "lacks") {
            if text.contains(&unwanted) {
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
