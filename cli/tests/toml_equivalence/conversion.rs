//! converts every accepted JSON fixture that has a TOML counterpart, for each host it is
//! valid on, and checks the document the conversion writes.

use std::path::Path;

use marshal_cli::convert::{ConversionFailure, convert_to_toml, first_difference};
use marshal_cli::json::{self, Value};
use marshal_cli::profile::Profile;
use marshal_cli::runtime::fake::FakeClock;
use marshal_cli::validate::{CheckoutRule, Failure, validate, validate_toml};

use crate::{Fixture, counterpart, fixtures};

/// 2026-11-04T22:26:51Z
const CLOCK_MILLIS: u64 = 1_793_831_211_000;

const HOSTS: [Profile; 2] = [Profile::Axpc, Profile::Axmc];

fn convert(json: &[u8], host: Profile) -> Result<String, ConversionFailure> {
    convert_to_toml(
        json,
        host,
        Path::new("config.json"),
        CheckoutRule::default(),
        &FakeClock::at(CLOCK_MILLIS),
    )
}

/// the accepted fixtures that have a TOML counterpart.
fn accepted_and_translated() -> Vec<Fixture> {
    fixtures()
        .into_iter()
        .filter(|fixture| fixture.accepted_by_json && counterpart(fixture).is_ok())
        .collect()
}

/// every string a `tokenPath` holds, anywhere in the document.
fn token_paths(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (name, member) in object.iter() {
                match member {
                    Value::String(path) if name == "tokenPath" => found.push(path.clone()),
                    _ => token_paths(member, found),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| token_paths(item, found)),
        _ => {}
    }
}

#[test]
fn every_accepted_fixture_that_maps_converts_on_each_host_it_is_valid_on() {
    let mut converted = 0;
    let mut failures = Vec::new();
    for fixture in accepted_and_translated() {
        let mut hosts = 0;
        for host in HOSTS {
            if validate(&fixture.bytes, host, None).outcome.is_err() {
                continue;
            }
            hosts += 1;
            let text = match convert(&fixture.bytes, host) {
                Ok(text) => text,
                Err(failure) => {
                    failures.push(format!("{} on {host}: {failure}", fixture.name));
                    continue;
                }
            };
            converted += 1;
            let line_ending = if host == Profile::Axpc { "\r\n" } else { "\n" };
            if text.matches('\n').count() != text.matches(line_ending).count() {
                failures.push(format!(
                    "{} on {host}: a line ends the other way",
                    fixture.name
                ));
            }
            // the translator is a second, independent mapping of the same file
            let translated = counterpart(&fixture).unwrap();
            let by_translation = validate_toml(translated.as_bytes(), host, None).outcome;
            let by_conversion = validate_toml(text.as_bytes(), host, None).outcome.unwrap();
            if let Ok(plan) = by_translation
                && let Some(difference) = first_difference(&plan, &by_conversion)
            {
                failures.push(format!("{} on {host}: {difference}", fixture.name));
            }
        }
        if hosts == 0 {
            failures.push(format!("{} is valid on no host", fixture.name));
        }
    }
    assert!(converted >= 20, "only {converted} conversions were made");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_converted_document_carries_no_token_path() {
    let mut checked = 0;
    for fixture in accepted_and_translated() {
        let mut paths = Vec::new();
        token_paths(&json::parse(&fixture.bytes).unwrap(), &mut paths);
        for host in HOSTS {
            let Ok(text) = convert(&fixture.bytes, host) else {
                continue;
            };
            checked += 1;
            for needle in ["tokenPath", "token_path", ".token"] {
                assert!(!text.contains(needle), "{}: {needle}", fixture.name);
            }
            for path in &paths {
                let escaped = path.replace('\\', "\\\\");
                assert!(
                    !text.contains(path.as_str()) && !text.contains(&escaped),
                    "{}: {path}",
                    fixture.name
                );
            }
        }
    }
    assert!(checked >= 20, "only {checked} conversions were checked");
}

const WINDOWS_FILE: &str = r#"{
  "imageName": "actions-runner:local",
  "repositories": [{
    "owner": "octo", "repository": "hello", "name": "alpha", "slots": 2,
    "tokenPath": "C:\\tokens\\alpha.token", "labels": ["gpu"],
    "volumes": [{ "suffix": "npm", "mountPath": "/home/runner/.npm" }],
    "cpus": 1.5, "memoryGb": 12,
    "diagnostics": true, "diagnosticDirectory": "C:\\diagnostics\\alpha"
  }, {
    "owner": "octo", "repository": "world", "name": "beta", "slots": 1,
    "tokenPath": "C:\\tokens\\beta.token", "volumes": []
  }]
}"#;

const MACOS_FILE: &str = r#"{
  "budget": { "cpus": 8, "memoryGb": 32, "diskGb": 200 },
  "repositories": [{
    "owner": "octo", "repository": "hello", "name": "alpha", "slots": 1,
    "os": "macos", "diskGb": 50, "tokenPath": "/path/to/alpha.token"
  }]
}"#;

#[test]
fn a_windows_document_opens_with_the_file_name_and_the_time_and_ends_lines_with_crlf() {
    let text = convert(WINDOWS_FILE.as_bytes(), Profile::Axpc).unwrap();
    let expected = [
        "# Converted from config.json by Marshal at 2026-11-04T22:26:51Z.",
        "",
        "schema_version = 1",
        "routing_label = \"axpc\"",
        "image_name = \"actions-runner:local\"",
        "",
        "[[pool]]",
        "name = \"alpha\"",
        "repository = \"octo/hello\"",
        "runners = 2",
        "labels = [\"gpu\"]",
        "cpus = 1.5",
        "memory_gb = 12",
        "diagnostics = true",
        "diagnostic_directory = \"C:\\\\diagnostics\\\\alpha\"",
        "",
        "[[pool.volumes]]",
        "suffix = \"npm\"",
        "mount_path = \"/home/runner/.npm\"",
        "",
        "[[pool]]",
        "name = \"beta\"",
        "repository = \"octo/world\"",
        "runners = 1",
        "",
    ]
    .join("\r\n");
    assert_eq!(text, expected);
}

#[test]
fn a_macos_document_ends_lines_with_lf_and_carries_the_budget() {
    let text = convert(MACOS_FILE.as_bytes(), Profile::Axmc).unwrap();
    let expected = [
        "# Converted from config.json by Marshal at 2026-11-04T22:26:51Z.",
        "",
        "schema_version = 1",
        "routing_label = \"axmc\"",
        "",
        "[budget]",
        "cpus = 8",
        "memory_gb = 32",
        "disk_gb = 200",
        "",
        "[[pool]]",
        "name = \"alpha\"",
        "repository = \"octo/hello\"",
        "runners = 1",
        "os = \"macos\"",
        "disk_gb = 50",
        "",
    ]
    .join("\n");
    assert_eq!(text, expected);
    assert!(!text.contains('\r'));
}

#[test]
fn the_line_ending_follows_the_host_and_not_the_machine_the_test_runs_on() {
    assert!(
        convert(WINDOWS_FILE.as_bytes(), Profile::Axpc)
            .unwrap()
            .contains("\r\n")
    );
    assert!(
        !convert(MACOS_FILE.as_bytes(), Profile::Axmc)
            .unwrap()
            .contains('\r')
    );
}

#[test]
fn the_opening_comment_names_the_file_and_never_its_path() {
    for source in [
        "config.json",
        "/Users/me/Library/Application Support/marshal/config.json",
        "C:\\Users\\me\\AppData\\Local\\marshal\\config.json",
        "C:\\Users\\me\\AppData\\Local\\marshal\\config.json",
    ] {
        let text = convert_to_toml(
            WINDOWS_FILE.as_bytes(),
            Profile::Axpc,
            Path::new(source),
            CheckoutRule::default(),
            &FakeClock::at(CLOCK_MILLIS),
        )
        .unwrap();
        assert_eq!(
            text.lines().next(),
            Some("# Converted from config.json by Marshal at 2026-11-04T22:26:51Z."),
            "{source}"
        );
        assert!(!text.contains("Users"), "{source}");
    }
}

#[test]
fn a_file_name_cannot_end_the_comment() {
    let text = convert_to_toml(
        MACOS_FILE.as_bytes(),
        Profile::Axmc,
        Path::new("a\nschema_version = 2\u{7}.json"),
        CheckoutRule::default(),
        &FakeClock::at(CLOCK_MILLIS),
    )
    .unwrap();
    assert_eq!(
        text.lines().next(),
        Some("# Converted from a?schema_version = 2?.json by Marshal at 2026-11-04T22:26:51Z.")
    );
}

#[test]
fn the_time_comes_from_the_clock_the_caller_passes() {
    let at = |millis| {
        let clock = FakeClock::at(millis);
        convert_to_toml(
            MACOS_FILE.as_bytes(),
            Profile::Axmc,
            Path::new("config.json"),
            CheckoutRule::default(),
            &clock,
        )
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string()
    };
    assert_eq!(
        at(0),
        "# Converted from config.json by Marshal at 1970-01-01T00:00:00Z."
    );
    assert_eq!(
        at(951_782_400_000 + 86_399_000),
        "# Converted from config.json by Marshal at 2000-02-29T23:59:59Z."
    );
}

#[test]
fn a_file_that_breaks_the_hosts_rules_is_refused_with_the_reason_and_no_text() {
    let failure = convert(br#"{ "repositories": [] }"#, Profile::Axpc).unwrap_err();
    let ConversionFailure::SourceRejected(Failure::Invalid(problems)) = &failure else {
        panic!("{failure:?}");
    };
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert_eq!(
        failure.to_string(),
        "the JSON configuration does not pass the host's rules: \
         imageName: required field is missing; repositories: must list at least one repository"
    );

    assert!(matches!(
        convert(WINDOWS_FILE.as_bytes(), Profile::Axmc),
        Err(ConversionFailure::SourceRejected(_))
    ));
}
