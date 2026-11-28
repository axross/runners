//! runs the validator over the TOML fixtures under `tests/host-configuration/toml/`
//! and checks it against the `expectations.json` beside them, then builds the inputs
//! whose exact bytes a stored file cannot keep: a byte-order mark, a file of exactly
//! 1 MiB, one that is not UTF-8, and a token pasted into a document.
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

use marshal_cli::command::MAX_CONFIG_BYTES;
use marshal_cli::de::Deserializer;
use marshal_cli::json;
use marshal_cli::profile::Profile;
use marshal_cli::validate::{Failure, Plan, validate_toml};

/// one entry of `expectations.json`; an unknown key fails the read, so a typo cannot
/// silently turn a check off.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expectation {
    profile: String,
    outcome: Outcome,
    /// the problems, whole and in order, for a rejected file.
    problems: Vec<String>,
    /// text the description of an accepted file holds.
    contains: Vec<String>,
    /// text no problem and no description may hold.
    lacks: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Accept,
    Reject,
}

const TOKEN: &str = "ghp_0123456789abcdefghijABCDEFGHIJ012345";

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/host-configuration/toml")
}

fn expectations(root: &Path) -> BTreeMap<String, Expectation> {
    let bytes = fs::read(root.join("expectations.json")).unwrap();
    let tree = json::parse(&bytes).unwrap();
    BTreeMap::deserialize(Deserializer::new(&tree)).unwrap()
}

fn fixture_files(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for directory in ["accepted", "rejected"] {
        for entry in fs::read_dir(root.join(directory)).unwrap() {
            let path = entry.unwrap().path();
            assert_eq!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("toml"),
                "{path:?}"
            );
            found.insert(format!(
                "{directory}/{}",
                path.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    found
}

fn profile(name: &str) -> Profile {
    Profile::from_name(name).unwrap()
}

/// the problems of a rejected document, one per line; a document that cannot be read
/// has the one.
fn problems(bytes: &[u8], profile: Profile) -> Vec<String> {
    match validate_toml(bytes, profile, None).outcome {
        Ok(plan) => panic!(
            "expected a rejected document, got {}",
            describe(&plan).join("\n")
        ),
        Err(Failure::Unreadable(problem)) => vec![problem],
        Err(Failure::Invalid(problems)) => problems,
    }
}

fn describe(plan: &Plan) -> Vec<String> {
    let settings = plan.settings.as_ref().unwrap();
    let show = |setting: Option<bool>| setting.map_or("unset".to_string(), |s| s.to_string());
    let lifecycle = settings.lifecycle;
    let mut lines = vec![
        format!("schema_version: {}", settings.schema_version),
        format!("routing_label: {}", settings.routing_label),
        format!(
            "lifecycle: start_at_login={} start_runners_on_launch={} keep_awake={} stop_on_network_change={} resume_after_stop={}",
            show(lifecycle.start_at_login),
            show(lifecycle.start_runners_on_launch),
            show(lifecycle.keep_awake),
            show(lifecycle.stop_on_network_change),
            show(lifecycle.resume_after_stop)
        ),
    ];
    if let Some(image) = &plan.image_name {
        lines.push(format!("image_name: {image}"));
    }
    if let Some(budget) = &plan.budget {
        lines.push(format!(
            "budget: cpus={} memory_gb={} disk_gb={}",
            budget.cpus, budget.memory_gb, budget.disk_gb
        ));
    }
    for entry in &plan.entries {
        let volumes: Vec<String> = entry
            .volumes
            .iter()
            .map(|volume| format!("{}:{}", volume.name, volume.mount_path))
            .collect();
        lines.push(format!(
            "{}: name={} repository={} runners={} os={} cpus={} memory_gb={} disk_gb={} labels=[{}] volumes=[{}] diagnostics={} diagnostic_directory={} raw_records={} token_path={}",
            entry.path,
            entry.name,
            entry.target,
            entry.slots,
            entry.os.map_or("none", |os| os.name()),
            entry.cpus,
            entry.memory_gb,
            entry.disk_gb.map_or("none".to_string(), |disk| disk.to_string()),
            entry.labels.join(", "),
            volumes.join(", "),
            entry.diagnostics,
            entry.diagnostic_directory.as_deref().unwrap_or("none"),
            entry.diagnostic_raw_records,
            entry.token_path.as_deref().unwrap_or("none"),
        ));
    }
    lines
}

#[test]
fn every_fixture_has_an_expectation_and_every_expectation_a_fixture() {
    let root = fixture_root();
    let expected: BTreeSet<String> = expectations(&root).into_keys().collect();
    let found = fixture_files(&root);
    let without_expectation: Vec<_> = found.difference(&expected).collect();
    let without_fixture: Vec<_> = expected.difference(&found).collect();
    assert!(
        without_expectation.is_empty() && without_fixture.is_empty(),
        "fixtures without an expectation: {without_expectation:?}; expectations without a fixture: {without_fixture:?}"
    );
}

#[test]
fn every_fixture_gets_its_expected_outcome_and_problems() {
    let root = fixture_root();
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, expectation) in &expectations(&root) {
        let bytes = fs::read(root.join(name)).unwrap();
        let validation = validate_toml(&bytes, profile(&expectation.profile), None);
        checked += 1;
        let (outcome, problems, described) = match validation.outcome {
            Ok(plan) => (Outcome::Accept, Vec::new(), describe(&plan)),
            Err(Failure::Unreadable(problem)) => (Outcome::Reject, vec![problem], Vec::new()),
            Err(Failure::Invalid(problems)) => (Outcome::Reject, problems, Vec::new()),
        };
        if outcome != expectation.outcome {
            failures.push(format!(
                "{name}: expected {:?}, got {outcome:?}\n{}",
                expectation.outcome,
                problems.join("\n")
            ));
            continue;
        }
        if problems != expectation.problems {
            failures.push(format!(
                "{name}: expected the problems\n{}\ngot\n{}",
                expectation.problems.join("\n"),
                problems.join("\n")
            ));
        }
        let text = format!("{}\n{}", problems.join("\n"), described.join("\n"));
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
    assert!(checked > 80, "only {checked} fixtures were checked");
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn an_expectation_with_an_unknown_or_misspelled_key_fails_the_read() {
    let valid = r#"{ "profile": "axpc", "outcome": "accept", "problems": [], "contains": [], "lacks": [] }"#;
    let read = |text: &str| {
        Expectation::deserialize(Deserializer::new(&json::parse(text.as_bytes()).unwrap()))
    };
    assert!(read(valid).is_ok());
    for text in [
        r#"{ "profile": "axpc", "outcome": "accept", "problems": [], "contains": [], "lacks": [], "extra": 1 }"#,
        r#"{ "profile": "axpc", "outcome": "accept", "Problems": [], "contains": [], "lacks": [] }"#,
        r#"{ "profile": "axpc", "outcome": "reject", "problems": [], "contains": [] }"#,
    ] {
        assert!(read(text).is_err(), "{text}");
    }
}

fn accepted(name: &str) -> Vec<u8> {
    fs::read(fixture_root().join("accepted").join(name)).unwrap()
}

#[test]
fn a_leading_byte_order_mark_is_ignored_and_does_not_move_a_position() {
    let plain = accepted("windows-minimal.toml");
    let mut marked = vec![0xEF, 0xBB, 0xBF];
    marked.extend_from_slice(&plain);
    let found = validate_toml(&marked, Profile::Axpc, None).outcome.unwrap();
    assert_eq!(
        describe(&found),
        describe(&validate_toml(&plain, Profile::Axpc, None).outcome.unwrap())
    );

    let broken = |prefix: &[u8]| {
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(b"schema_version = 2\nroutine_label = ");
        bytes
    };
    assert_eq!(
        problems(&broken(b""), Profile::Axpc),
        problems(&broken(&[0xEF, 0xBB, 0xBF]), Profile::Axpc)
    );
}

/// pads a document with comment lines to exactly `size` bytes.
fn padded(document: &[u8], size: usize) -> Vec<u8> {
    let mut bytes = document.to_vec();
    while bytes.len() < size {
        let room = size - bytes.len();
        let line = if room >= 80 {
            format!("# {}\n", "x".repeat(77))
        } else if room >= 3 {
            format!("#{}\n", "x".repeat(room - 2))
        } else {
            // two bytes remain: a blank line twice
            "\n".repeat(room)
        };
        bytes.extend_from_slice(line.as_bytes());
    }
    assert_eq!(bytes.len(), size);
    bytes
}

#[test]
fn a_file_of_exactly_one_mebibyte_is_read_and_one_byte_more_is_refused() {
    let size = usize::try_from(MAX_CONFIG_BYTES).unwrap();
    let document = accepted("windows-minimal.toml");

    let exact = padded(&document, size);
    assert!(
        validate_toml(&exact, Profile::Axpc, None).outcome.is_ok(),
        "a file of exactly 1 MiB is read"
    );

    let over = padded(&document, size + 1);
    assert_eq!(
        problems(&over, Profile::Axpc),
        ["line 1: the file is larger than 1048576 bytes (1 MiB)"]
    );

    // the size is checked before the bytes are parsed, so no syntax error appears
    let mut garbage = vec![b'='; size + 1];
    garbage[0] = b'[';
    assert_eq!(
        problems(&garbage, Profile::Axpc),
        ["line 1: the file is larger than 1048576 bytes (1 MiB)"]
    );
}

#[test]
fn bytes_that_are_not_utf8_are_refused_with_the_position_of_the_first_bad_byte() {
    assert_eq!(
        problems(
            b"schema_version = 1\nrouting_label = \"a\xFFb\"\n",
            Profile::Axpc
        ),
        ["line 2, column 19: the file is not valid UTF-8"]
    );
    assert_eq!(
        problems(b"\xFF", Profile::Axmc),
        ["line 1, column 1: the file is not valid UTF-8"]
    );
    assert_eq!(
        problems(b"\xEF\xBB\xBF\xC3", Profile::Axmc),
        ["line 1, column 1: the file is not valid UTF-8"]
    );
}

#[test]
fn a_control_character_is_a_syntax_error_that_does_not_quote_it() {
    let found = problems(
        b"schema_version = 1\nrouting_label = \"a\x07b\"\n",
        Profile::Axpc,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("line 2, column ") && found[0].contains(": syntax error: "),
        "{found:?}"
    );
    assert!(!found[0].contains('\u{7}'), "{found:?}");
}

#[test]
fn crlf_line_endings_give_the_same_problems_at_the_same_lines() {
    let root = fixture_root();
    for name in [
        "order-schema-then-top-level-then-lifecycle-then-pools.toml",
        "missing-keys-name-the-table-header.toml",
        "syntax-table-never-closed.toml",
        "pools-clash-routing.toml",
        "syntax-unterminated-string.toml",
    ] {
        let text = fs::read_to_string(root.join("rejected").join(name)).unwrap();
        let unix = text.replace("\r\n", "\n");
        let windows = unix.replace('\n', "\r\n");
        assert_eq!(
            problems(unix.as_bytes(), Profile::Axpc),
            problems(windows.as_bytes(), Profile::Axpc),
            "{name}"
        );
    }
}

#[test]
fn a_syntax_error_at_a_line_break_has_the_same_column_with_either_ending() {
    for (name, text, expected) in [
        (
            "basic string",
            "a = \"x\n",
            "line 1, column 7: syntax error: invalid basic string, expected `\"`",
        ),
        (
            "literal string",
            "a = 'x\n",
            "line 1, column 7: syntax error: invalid literal string, expected `'`",
        ),
    ] {
        let windows = text.replace('\n', "\r\n");
        assert_eq!(
            problems(text.as_bytes(), Profile::Axpc),
            [expected],
            "{name}"
        );
        assert_eq!(
            problems(windows.as_bytes(), Profile::Axpc),
            [expected],
            "{name}"
        );
    }
}

/// the lines of a problem list that hold the token.
fn echoed(problems: &[String]) -> Vec<&String> {
    problems
        .iter()
        .filter(|problem| problem.contains("0123456789abcdefghij"))
        .collect()
}

#[test]
fn a_token_in_a_syntax_error_or_a_wrong_type_value_is_in_no_problem() {
    let root = fixture_root();
    let mut tested = 0;
    for name in fs::read_dir(root.join("rejected")).unwrap() {
        let path = name.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        if !text.contains(TOKEN) {
            continue;
        }
        for profile in [Profile::Axpc, Profile::Axmc] {
            let found = problems(text.as_bytes(), profile);
            assert!(echoed(&found).is_empty(), "{path:?}: {found:?}");
            tested += 1;
        }
    }
    assert!(tested >= 8, "only {tested} token fixtures were tested");
}

/// the words of `text` shaped like a credential: a GitHub token, a JWT, or a long
/// base64 run with digits and both cases of letters.
///
/// `other_credential_words` in `app/crates/view/tests/view/privacy.rs` repeats the non-GitHub
/// shapes: a helper in a test target cannot be imported by another crate's tests, so a change to a
/// shape belongs in both.
fn credential_words(text: &str) -> Vec<&str> {
    const PREFIXES: [&str; 6] = ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"];
    let words = |extra: &'static str| {
        text.split(move |c: char| !(c.is_ascii_alphanumeric() || extra.contains(c)))
    };
    let mut found: Vec<&str> = words("_")
        .filter(|word| {
            PREFIXES.iter().any(|prefix| {
                word.strip_prefix(prefix)
                    .is_some_and(|rest| rest.len() >= 16)
            })
        })
        .collect();
    found.extend(words("_-").filter(|word| word.starts_with("eyJ") && word.len() >= 19));
    found.extend(words("_-+/=").filter(|word| {
        word.len() >= 40
            && word.chars().any(|c| c.is_ascii_digit())
            && word.chars().any(|c| c.is_ascii_uppercase())
            && word.chars().any(|c| c.is_ascii_lowercase())
    }));
    found
}

#[test]
fn no_host_configuration_fixture_holds_a_credential_but_the_one_fake_token() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/host-configuration");
    let mut scanned = 0;
    let mut fakes = 0;
    let mut pending = vec![root];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let bytes = fs::read(&path).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            scanned += 1;
            for word in credential_words(&text) {
                assert_eq!(word, TOKEN, "{path:?}");
                fakes += 1;
            }
        }
    }
    assert!(scanned > 200, "only {scanned} fixture files were scanned");
    assert!(fakes >= 8, "only {fakes} fake tokens were found");
}

#[test]
fn the_credential_scan_sees_each_shape_and_only_those() {
    let jwt = format!("eyJ{}", "a1".repeat(12));
    let base64 = "aB3+".repeat(10);
    for shaped in [
        format!("x = \"ghp_{}\"", "Q".repeat(16)),
        format!("x = \"github_pat_{}\"", "q1".repeat(8)),
        format!("Bearer {jwt}"),
        format!("config: {base64}"),
    ] {
        assert_eq!(credential_words(&shaped).len(), 1, "{shaped}");
    }
    for plain in [
        "ghp_short".to_owned(),
        format!("xghp_{}", "Q".repeat(16)),
        "n".repeat(60),
        "C:\\Users\\runner\\diagnostics\\build-hosts".to_owned(),
    ] {
        assert!(credential_words(&plain).is_empty(), "{plain}");
    }
}

/// a document that is valid for Windows, with every kind of key and value in it.
const WELL_FORMED: &str = r#"schema_version = 1
routing_label = "build-hosts"
image_name = "example-image:tag"

[lifecycle]
keep_awake = true

[[pool]]
name = "alpha"
repository = "octo/hello"
runners = 2
os = "linux"
labels = ["gpu", "fast"]
cpus = 1.5
memory_gb = 12
diagnostics = true
diagnostic_directory = "C:\\diagnostics\\alpha"

[[pool.volumes]]
suffix = "cache"
mount_path = "/home/runner/.cache"

[[pool]]
name = "beta"
repository = "octo/world"
runners = 1
scale_set = "build-hosts-beta"
"#;

#[test]
fn the_well_formed_document_is_valid_for_windows_and_has_an_on_demand_pool() {
    assert!(WELL_FORMED.contains("\nscale_set = "));
    let plan = validate_toml(WELL_FORMED.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap_or_else(|failure| panic!("{}", failure_text(&failure)));
    assert!(
        plan.entries
            .iter()
            .any(|entry| entry.scale_set.as_deref() == Some("build-hosts-beta"))
    );
}

fn failure_text(failure: &Failure) -> String {
    match failure {
        Failure::Unreadable(problem) => problem.clone(),
        Failure::Invalid(problems) => problems.join("\n"),
    }
}

#[test]
fn a_token_pasted_anywhere_in_a_document_is_echoed_only_as_an_unknown_key() {
    for profile in [Profile::Axpc, Profile::Axmc] {
        let mut broken = 0;
        for at in 0..=WELL_FORMED.len() {
            if !WELL_FORMED.is_char_boundary(at) {
                continue;
            }
            let (before, after) = WELL_FORMED.split_at(at);
            let document = format!("{before}{TOKEN}{after}");
            let found = match validate_toml(document.as_bytes(), profile, None).outcome {
                Ok(_) => continue,
                Err(Failure::Unreadable(problem)) => vec![problem],
                Err(Failure::Invalid(problems)) => problems,
            };
            broken += 1;
            for line in echoed(&found) {
                // a key is named as it is written, which is how an unknown key is found
                assert!(line.ends_with(": unknown key"), "{at}: {line}");
            }
        }
        assert!(broken > 100, "only {broken} documents were rejected");
    }
}

#[test]
fn a_token_as_the_value_of_every_key_is_in_no_problem() {
    for profile in [Profile::Axpc, Profile::Axmc] {
        let mut changed = 0;
        let mut keys_rejected = BTreeSet::new();
        for (index, line) in WELL_FORMED.lines().enumerate() {
            let Some((key, _)) = line.split_once(" = ") else {
                continue;
            };
            for value in [
                format!("\"{TOKEN}\""),
                format!("[\"{TOKEN}\"]"),
                format!("{{ token = \"{TOKEN}\" }}"),
            ] {
                let document: Vec<String> = WELL_FORMED
                    .lines()
                    .enumerate()
                    .map(|(at, text)| {
                        if at == index {
                            format!("{key} = {value}")
                        } else {
                            text.to_string()
                        }
                    })
                    .collect();
                let found =
                    match validate_toml(document.join("\n").as_bytes(), profile, None).outcome {
                        Ok(_) => continue,
                        Err(Failure::Unreadable(problem)) => vec![problem],
                        Err(Failure::Invalid(problems)) => problems,
                    };
                changed += 1;
                keys_rejected.insert(key);
                assert!(echoed(&found).is_empty(), "{key}: {found:?}");
            }
        }
        assert!(changed > 20, "only {changed} documents were rejected");
        assert!(
            keys_rejected.contains("scale_set"),
            "no scale_set value was rejected for {profile:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_diagnostic_directory_inside_the_checkout_is_placed_on_its_line() {
    let document = "schema_version = 1\nrouting_label = \"axmc\"\n\n[budget]\ncpus = 4\nmemory_gb = 16\ndisk_gb = 100\n\n[[pool]]\nname = \"alpha\"\nrepository = \"octo/hello\"\nrunners = 1\ndisk_gb = 40\ndiagnostics = true\ndiagnostic_directory = \"/checkout/evidence\"\n";
    let found = validate_toml(
        document.as_bytes(),
        Profile::Axmc,
        Some(Path::new("/checkout")),
    );
    assert!(!found.checkout_rule_skipped);
    let Err(Failure::Invalid(found)) = found.outcome else {
        panic!("expected a rejected document");
    };
    assert_eq!(
        found,
        ["line 15: pool[0].diagnostic_directory: must be outside the checkout"]
    );
    assert!(validate_toml(document.as_bytes(), Profile::Axmc, None).checkout_rule_skipped);
}

#[test]
fn a_file_of_many_undefined_keys_has_every_one_reported() {
    let mut document = String::from("schema_version = 1\nrouting_label = \"axpc\"\n");
    let mut count = 0;
    while document.len() < 1_000_000 {
        document.push_str(&format!("k{count} = 1\n"));
        count += 1;
    }
    let found = problems(document.as_bytes(), Profile::Axpc);
    let unknown = found
        .iter()
        .filter(|problem| problem.ends_with(": unknown key"))
        .count();
    assert_eq!(unknown, count);
    assert_eq!(found[0], "line 3: k0: unknown key");
}

#[test]
fn a_long_line_of_many_inline_values_has_every_one_reported() {
    let mut document = String::from(
        "schema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"example-image:tag\"\n[[pool]]\nname = \"alpha\"\nrepository = \"octo/hello\"\nrunners = 1\nlabels = [",
    );
    let mut count = 0;
    while document.len() < 1_000_000 {
        document.push_str("{ k = 1 },");
        count += 1;
    }
    document.push_str("]\n");
    let found = problems(document.as_bytes(), Profile::Axpc);
    assert_eq!(found.len(), count);
    assert!(
        found[0].starts_with("line 8: pool[0].labels: "),
        "{}",
        found[0]
    );
}

/// each host's example `config.toml` is valid and is the configuration of the host's
/// example JSON file, so the two examples cannot drift apart.
#[test]
fn each_hosts_example_toml_is_valid_and_matches_its_json_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (folder, profile) in [
        ("hosts/windows-docker-desktop", Profile::Axpc),
        ("hosts/macos", Profile::Axmc),
    ] {
        let toml = fs::read(root.join(folder).join("config.example.toml")).unwrap();
        let json = fs::read(root.join(folder).join("runner-host.example.json")).unwrap();
        let from_toml = validate_toml(&toml, profile, None)
            .outcome
            .unwrap_or_else(|failure| panic!("{folder}: {failure:?}"));
        let from_json = marshal_cli::validate::validate(&json, profile, None)
            .outcome
            .unwrap();
        assert_eq!(
            marshal_cli::convert::first_difference(&from_json, &from_toml),
            None,
            "{folder}"
        );
    }
}

#[test]
fn on_demand_configuration_preserves_logical_identity_and_has_one_routing_label() {
    let header = "schema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"runner:1\"\n";
    for (name, scale_set) in [("axpc-linux", "axpc-linux"), ("logical", "AXPC-Linux_1.2")] {
        let source = format!(
            "{header}[[pool]]\nname = \"{name}\"\nrepository = \"o/r\"\nrunners = 3\nscale_set = \"{scale_set}\"\nlabels = []\n"
        );
        let plan = validate_toml(source.as_bytes(), Profile::Axpc, None)
            .outcome
            .unwrap();
        assert_eq!(plan.entries[0].name, name);
        assert_eq!(plan.entries[0].scale_set.as_deref(), Some(scale_set));
        assert_eq!(plan.entries[0].labels, [scale_set]);
        assert_eq!(plan.entries[0].slots, 3);
    }
    let base = format!("{header}[[pool]]\nname = \"logical\"\nrepository = \"o/r\"\nrunners = 1\n");
    let longest = format!("axpc-{}", "x".repeat(40));
    assert!(
        validate_toml(
            format!("{base}scale_set = \"{longest}\"\n").as_bytes(),
            Profile::Axpc,
            None
        )
        .outcome
        .is_ok()
    );
    for value in [
        "\"\"".to_string(),
        "\"axpc-\"".into(),
        "\"other-linux\"".into(),
        "\"axpc-a/b\"".into(),
        "\"axpc-λ\"".into(),
        format!("\"{longest}x\""),
        "9".into(),
    ] {
        let found = problems(
            format!("{base}scale_set = {value}\n").as_bytes(),
            Profile::Axpc,
        );
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("pool[0].scale_set:")),
            "{found:?}"
        );
    }
    let found = problems(
        format!("{base}scale_set = \"axpc-linux\"\nlabels = [\"dedicated\"]\n").as_bytes(),
        Profile::Axpc,
    );
    assert!(
        found
            .iter()
            .any(|problem| problem.contains("pool[0].labels:"))
    );
    // a routing label that is a valid label but outside the scale-set character set
    // reaches the on-demand rule, and is reported once however many pools set the key
    let second = "[[pool]]\nname = \"second\"\nrepository = \"o/other\"\nrunners = 1\nscale_set = \"axpc-other\"\n";
    let found = problems(
        format!("{base}scale_set = \"axpc-linux\"\n{second}")
            .replace("routing_label = \"axpc\"", "routing_label = \"ax:pc\"")
            .as_bytes(),
        Profile::Axpc,
    );
    assert_eq!(
        found,
        [
            "line 2: routing_label: must be ASCII letters, digits, hyphens, underscores or dots for on-demand pools"
        ]
    );
    // an always-on pool beside it is unaffected, and a label the general shape rule
    // refuses never reaches the on-demand rule
    let always_on =
        format!("{header}[[pool]]\nname = \"logical\"\nrepository = \"o/r\"\nrunners = 1\n")
            .replace("routing_label = \"axpc\"", "routing_label = \"ax:pc\"");
    assert!(
        validate_toml(always_on.as_bytes(), Profile::Axpc, None)
            .outcome
            .is_ok()
    );
    let found = problems(
        format!("{base}scale_set = \"axpc-linux\"\n")
            .replace(
                "routing_label = \"axpc\"",
                "routing_label = \"host \u{3bb}\"",
            )
            .as_bytes(),
        Profile::Axpc,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("line 2: routing_label: must be "),
        "{found:?}"
    );
    assert!(!found[0].contains("on-demand"), "{found:?}");
}

#[test]
fn scale_set_aliases_cannot_claim_another_pools_runtime_or_routing_identity() {
    let header = "schema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"runner:1\"\n";
    let first = "[[pool]]\nname = \"axpc-app\"\nrepository = \"o/r\"\nrunners = 1\nscale_set = \"axpc-linux\"\n";
    for other in [
        "axpc-app",
        "AXPC-App",
        "axpc-app-extra",
        "axpc-linux",
        "axpc-linux-extra",
    ] {
        let second = format!(
            "[[pool]]\nname = \"other\"\nrepository = \"o/elsewhere\"\nrunners = 1\nscale_set = \"{other}\"\n"
        );
        for pools in [format!("{first}{second}"), format!("{second}{first}")] {
            let found = problems(format!("{header}{pools}").as_bytes(), Profile::Axpc);
            assert!(
                found.iter().any(|problem| problem
                    .contains(".scale_set: scale-set routing or cleanup identity collides")),
                "{found:?}"
            );
        }
    }
    let second = "[[pool]]\nname = \"other\"\nrepository = \"O/R\"\nrunners = 1\nlabels = [\"AXPC-LINUX\"]\n";
    let found = problems(format!("{header}{first}{second}").as_bytes(), Profile::Axpc);
    assert!(
        found
            .iter()
            .any(|problem| problem.contains("pool[0].scale_set:"))
    );
    let separate = second.replace("O/R", "o/elsewhere");
    assert!(
        validate_toml(
            format!("{header}{first}{separate}").as_bytes(),
            Profile::Axpc,
            None
        )
        .outcome
        .is_ok()
    );
    let safe = second.replace("AXPC-LINUX", "axpc-linuxish");
    assert!(
        validate_toml(
            format!("{header}{first}{safe}").as_bytes(),
            Profile::Axpc,
            None
        )
        .outcome
        .is_ok()
    );
}

#[test]
fn the_windows_slice_rejects_mac_on_demand_but_keeps_native_always_on() {
    let source = "schema_version = 1\nrouting_label = \"axmc\"\n[budget]\ncpus = 8\nmemory_gb = 16\ndisk_gb = 256\n[[pool]]\nname = \"native\"\nrepository = \"o/r\"\nos = \"macos\"\n";
    assert!(
        validate_toml(source.as_bytes(), Profile::Axmc, None)
            .outcome
            .is_ok()
    );
    for os in ["macos", "linux"] {
        let source = source.replace(
            "os = \"macos\"",
            &format!("os = \"{os}\"\ndisk_gb = 64\nscale_set = \"axmc-linux\""),
        );
        let found = problems(source.as_bytes(), Profile::Axmc);
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("pool[0].scale_set: must be absent on macOS")),
            "{found:?}"
        );
    }
}
