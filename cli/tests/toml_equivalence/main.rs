//! checks the TOML reader against the JSON one over the shared JSON fixtures: each
//! fixture is translated to a TOML document at test time, and the two readers must give
//! the same outcome, or the fixture has an expectation of its own for TOML. the fixtures
//! that cannot be translated are listed by name.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod conversion;
mod translate;
mod wording;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use marshal_cli::de::Deserializer;
use marshal_cli::json;
use marshal_cli::profile::Profile;
use marshal_cli::validate::{Failure, Validation, validate, validate_toml};

use translate::{NotTranslated, translate};

/// why a fixture has no TOML counterpart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonOnly {
    /// the bytes are not a JSON document: comments, trailing commas, quoting, number
    /// forms, invalid UTF-8.
    NotJson,
    /// the root is not an object.
    RootIsNotAnObject,
    /// the fixture is built on JSON's `null`.
    Null,
    /// a value that TOML cannot hold, which TOML reports as a syntax error.
    TomlCannotHold,
    /// the fixture is about `tokenPath` or a token file, which TOML has none of.
    Token,
}

/// the entries of `expectations.json` that this test reads.
#[derive(Deserialize)]
struct JsonExpectation {
    profile: String,
    outcome: String,
}

/// what a fixture's TOML counterpart gives when it differs from the JSON outcome.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlExpectation {
    outcome: String,
    /// the problems, whole and in order, without their line numbers.
    problems: Vec<String>,
    reason: String,
}

/// a shared JSON fixture with the profile and outcome `expectations.json` gives it.
pub struct Fixture {
    pub name: String,
    pub profile: Profile,
    pub accepted_by_json: bool,
    pub bytes: Vec<u8>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/host-configuration")
}

fn read_map<T: for<'de> Deserialize<'de>>(path: &Path) -> BTreeMap<String, T> {
    let tree = json::parse(&fs::read(path).unwrap()).unwrap();
    BTreeMap::deserialize(Deserializer::new(&tree)).unwrap()
}

/// every shared JSON fixture, in name order.
pub fn fixtures() -> Vec<Fixture> {
    let root = root();
    read_map::<JsonExpectation>(&root.join("expectations.json"))
        .into_iter()
        .map(|(name, expectation)| Fixture {
            profile: Profile::from_name(&expectation.profile).unwrap(),
            accepted_by_json: expectation.outcome == "accept",
            bytes: fs::read(root.join(&name)).unwrap(),
            name,
        })
        .collect()
}

fn toml_expectations() -> BTreeMap<String, TomlExpectation> {
    read_map(&root().join("toml/translated/expectations.json"))
}

/// the fixtures that stay JSON-only, by name. a fixture missing from the list that
/// cannot be translated fails the test, and so does one on the list that can.
const JSON_ONLY: &[(&str, JsonOnly)] = &[
    ("rust/raw/case-variant-keys.json", JsonOnly::NotJson),
    ("rust/raw/comment.json", JsonOnly::NotJson),
    ("rust/raw/empty-key.json", JsonOnly::NotJson),
    ("rust/raw/empty.json", JsonOnly::RootIsNotAnObject),
    ("rust/raw/invalid-json.json", JsonOnly::NotJson),
    ("rust/raw/invalid-utf8.json", JsonOnly::NotJson),
    ("rust/raw/leading-zero-integer.json", JsonOnly::NotJson),
    ("rust/raw/lone-surrogate-escape.json", JsonOnly::NotJson),
    ("rust/raw/null-entry.json", JsonOnly::Null),
    ("rust/raw/root-array.json", JsonOnly::RootIsNotAnObject),
    (
        "rust/raw/root-empty-array.json",
        JsonOnly::RootIsNotAnObject,
    ),
    ("rust/raw/root-null.json", JsonOnly::RootIsNotAnObject),
    ("rust/raw/root-scalar.json", JsonOnly::RootIsNotAnObject),
    ("rust/raw/single-quoted-string.json", JsonOnly::NotJson),
    (
        "rust/raw/token-path-canonically-equivalent.json",
        JsonOnly::Token,
    ),
    (
        "rust/raw/token-path-ignorable-character.json",
        JsonOnly::Token,
    ),
    ("rust/raw/trailing-comma.json", JsonOnly::NotJson),
    ("rust/raw/unquoted-property-name.json", JsonOnly::NotJson),
    (
        "rust/rejected/axmc-budget-errors-come-last.json",
        JsonOnly::Token,
    ),
    (
        "rust/rejected/axmc-budget-null-members.json",
        JsonOnly::Null,
    ),
    (
        "rust/rejected/axmc-different-os-shared-label-different-token.json",
        JsonOnly::Token,
    ),
    ("rust/rejected/axmc-image-name-null.json", JsonOnly::Null),
    ("rust/rejected/axmc-linux-slots-null.json", JsonOnly::Null),
    ("rust/rejected/axmc-macos-slots-null.json", JsonOnly::Null),
    (
        "rust/rejected/axmc-shared-token-other-repository.json",
        JsonOnly::Token,
    ),
    (
        "rust/rejected/axmc-token-path-control-character.json",
        JsonOnly::Token,
    ),
    (
        "rust/rejected/axmc-token-path-dot-components.json",
        JsonOnly::Token,
    ),
    (
        "rust/rejected/axmc-token-path-drive-letter.json",
        JsonOnly::Token,
    ),
    (
        "rust/rejected/axmc-token-path-relative.json",
        JsonOnly::Token,
    ),
    ("rust/rejected/axmc-volumes-null.json", JsonOnly::Null),
    ("rust/rejected/axpc-posix-token-path.json", JsonOnly::Token),
    (
        "shared/pwsh7/cpus-out-of-range-literal.json",
        JsonOnly::TomlCannotHold,
    ),
    ("shared/rejected/labels-null.json", JsonOnly::Null),
    ("shared/rejected/missing-token-path.json", JsonOnly::Token),
    ("shared/rejected/null-direct-fields.json", JsonOnly::Null),
    ("shared/rejected/null-required-fields.json", JsonOnly::Null),
    ("shared/rejected/null-root-fields.json", JsonOnly::Null),
    (
        "shared/rejected/pool-cross-repository-shared-token.json",
        JsonOnly::Token,
    ),
    ("shared/rejected/pool-different-token.json", JsonOnly::Token),
    ("shared/rejected/shared-token-path.json", JsonOnly::Token),
    (
        "shared/rejected/token-path-case-variant-other-repository.json",
        JsonOnly::Token,
    ),
    (
        "shared/rejected/token-path-drive-relative.json",
        JsonOnly::Token,
    ),
    ("shared/rejected/token-path-relative.json", JsonOnly::Token),
    (
        "shared/rejected/token-path-rooted-without-drive.json",
        JsonOnly::Token,
    ),
];

/// the TOML document of a fixture, or why it has none.
pub fn counterpart(fixture: &Fixture) -> Result<String, JsonOnly> {
    let document = translate(&fixture.bytes, fixture.profile).map_err(|reason| match reason {
        NotTranslated::NotJson => JsonOnly::NotJson,
        NotTranslated::RootIsNotAnObject => JsonOnly::RootIsNotAnObject,
        NotTranslated::Null => JsonOnly::Null,
    })?;
    if matches!(
        validate_toml(document.as_bytes(), fixture.profile, None).outcome,
        Err(Failure::Unreadable(_))
    ) {
        return Err(JsonOnly::TomlCannotHold);
    }
    if let Err(problems) = problems_of(&validate(&fixture.bytes, fixture.profile, None))
        && problems
            .iter()
            .any(|problem| problem.contains("tokenPath") || problem.contains("token file"))
    {
        return Err(JsonOnly::Token);
    }
    Ok(document)
}

/// the problems of a rejected configuration, or `Ok` for an accepted one.
pub fn problems_of(validation: &Validation) -> Result<(), Vec<String>> {
    match &validation.outcome {
        Ok(_) => Ok(()),
        Err(Failure::Unreadable(problem)) => Err(vec![problem.clone()]),
        Err(Failure::Invalid(problems)) => Err(problems.clone()),
    }
}

/// drops the line a TOML problem starts with.
fn without_line(problem: &str) -> &str {
    let (line, rest) = problem.split_once(": ").unwrap();
    assert!(
        line.strip_prefix("line ")
            .is_some_and(|number| number.parse::<usize>().is_ok()),
        "{problem}"
    );
    rest
}

#[test]
fn a_fixture_is_json_only_exactly_when_it_is_listed_as_such() {
    let listed: BTreeMap<&str, JsonOnly> = JSON_ONLY.iter().copied().collect();
    assert_eq!(listed.len(), JSON_ONLY.len(), "a name is listed twice");
    let all = fixtures();
    let mut failures = Vec::new();
    for fixture in &all {
        match (counterpart(fixture), listed.get(fixture.name.as_str())) {
            (Err(reason), None) => failures.push(format!(
                "{} cannot be translated ({reason:?}) and is not listed",
                fixture.name
            )),
            (Ok(_), Some(_)) => failures.push(format!(
                "{} is listed but would translate; take it off the list",
                fixture.name
            )),
            (Err(reason), Some(listed)) if reason != *listed => failures.push(format!(
                "{} is listed as {listed:?} but cannot be translated for {reason:?}",
                fixture.name
            )),
            _ => {}
        }
    }
    let names: Vec<&str> = all.iter().map(|fixture| fixture.name.as_str()).collect();
    failures.extend(
        listed
            .keys()
            .filter(|name| !names.contains(name))
            .map(|name| format!("{name} is listed but is no fixture")),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// the outcome of the TOML counterpart, in the form an expectation states it.
struct TomlOutcome {
    accepted: bool,
    problems: Vec<String>,
}

fn toml_outcome(document: &str, profile: Profile) -> TomlOutcome {
    let validation = validate_toml(document.as_bytes(), profile, None);
    match problems_of(&validation) {
        Ok(()) => TomlOutcome {
            accepted: true,
            problems: Vec::new(),
        },
        Err(problems) => TomlOutcome {
            accepted: false,
            problems: problems
                .iter()
                .map(|problem| without_line(problem).to_string())
                .collect(),
        },
    }
}

/// how the TOML counterpart of a fixture differs from the fixture, or `None` when the
/// two readers give the same outcome as the plan defines it.
fn difference(fixture: &Fixture, document: &str) -> Option<String> {
    let json = validate(&fixture.bytes, fixture.profile, None);
    let toml = validate_toml(document.as_bytes(), fixture.profile, None);
    match (&json.outcome, &toml.outcome) {
        (Ok(original), Ok(converted)) => {
            marshal_cli::convert::first_difference(original, converted)
                .map(|difference| format!("both accept it, but {difference}"))
        }
        (Err(_), Err(_)) => {
            let expected: Vec<String> = problems_of(&json)
                .unwrap_err()
                .iter()
                .map(|problem| wording::toml_problem(problem, fixture.profile))
                .collect();
            let found = toml_outcome(document, fixture.profile).problems;
            (expected != found).then(|| {
                format!(
                    "both reject it, with other problems\nJSON, in TOML's words:\n{}\nTOML:\n{}",
                    expected.join("\n"),
                    found.join("\n")
                )
            })
        }
        (Ok(_), Err(_)) => Some("JSON accepts it and TOML rejects it".to_string()),
        (Err(_), Ok(_)) => Some("JSON rejects it and TOML accepts it".to_string()),
    }
}

#[test]
fn every_translated_fixture_gives_the_same_outcome_or_has_its_own_expectation() {
    let expectations = toml_expectations();
    let mut failures = Vec::new();
    let mut with_own_expectation = Vec::new();
    let mut translated = 0;
    for fixture in fixtures() {
        let Ok(document) = counterpart(&fixture) else {
            continue;
        };
        translated += 1;
        let differs = difference(&fixture, &document);
        let Some(own) = expectations.get(&fixture.name) else {
            if let Some(differs) = differs {
                failures.push(format!("{}: {differs}", fixture.name));
            }
            continue;
        };
        with_own_expectation.push(fixture.name.clone());
        if differs.is_none() {
            failures.push(format!(
                "{} has an expectation of its own but gives the JSON outcome; remove it",
                fixture.name
            ));
        }
        let found = toml_outcome(&document, fixture.profile);
        if found.accepted != (own.outcome == "accept") || found.problems != own.problems {
            failures.push(format!(
                "{}: expected {} with the problems\n{}\ngot {}\n{}",
                fixture.name,
                own.outcome,
                own.problems.join("\n"),
                if found.accepted { "accept" } else { "reject" },
                found.problems.join("\n")
            ));
        }
        if own.reason.trim().is_empty() {
            failures.push(format!("{} has no reason", fixture.name));
        }
    }
    let unused: Vec<_> = expectations
        .keys()
        .filter(|name| !with_own_expectation.contains(name))
        .collect();
    assert!(
        unused.is_empty(),
        "expectations without a translated fixture: {unused:?}"
    );
    assert!(
        translated > 90,
        "only {translated} fixtures were translated"
    );
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn the_translated_fixtures_cover_accepted_and_rejected_files_of_both_hosts() {
    let mut seen = std::collections::BTreeSet::new();
    for fixture in fixtures() {
        if counterpart(&fixture).is_ok() {
            seen.insert((fixture.profile.name(), fixture.accepted_by_json));
        }
    }
    assert_eq!(seen.len(), 4, "{seen:?}");
}

#[test]
fn the_translator_writes_the_structure_of_a_fixture_as_toml() {
    let json = r#"{
        "imageName": "actions-runner:local",
        "repositories": [{
            "owner": "octo", "repository": "hello", "name": "alpha", "slots": 2,
            "tokenPath": "C:\\tokens\\alpha.token", "labels": ["x"],
            "volumes": [{ "suffix": "npm", "mountPath": "/home/runner/.npm" }],
            "memoryGb": 12, "diagnosticDirectory": "C:\\diag"
        }]
    }"#;
    assert_eq!(
        translate(json.as_bytes(), Profile::Axpc).unwrap(),
        "schema_version = 1
routing_label = \"axpc\"
image_name = \"actions-runner:local\"

[[pool]]
repository = \"octo/hello\"
name = \"alpha\"
runners = 2
labels = [\"x\"]
memory_gb = 12
diagnostic_directory = \"C:\\\\diag\"

[[pool.volumes]]
suffix = \"npm\"
mount_path = \"/home/runner/.npm\"
"
    );
}
