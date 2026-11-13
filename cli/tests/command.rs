//! reading a host configuration file with the library's `command::load_plan`, which
//! the agent, `start` and `build` share, and the summary of an accepted plan.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::Path;

use marshal_cli::command::load_plan;
use marshal_cli::profile::Profile;
use marshal_cli::summary::plan_summary;
use support::{TempDir, repository_root};

/// the summary lines of an accepted file, each ending in `\n`, or the message that
/// names why it was refused.
fn check(path: &Path, profile: Profile) -> Result<String, String> {
    load_plan(path, profile)
        .outcome
        .map(|plan| {
            plan_summary(&plan)
                .iter()
                .map(|line| format!("{line}\n"))
                .collect()
        })
        .map_err(|error| error.message(path))
}

fn axpc_example() -> std::path::PathBuf {
    repository_root().join("hosts/windows-docker-desktop/runner-host.example.json")
}

fn axmc_example() -> std::path::PathBuf {
    repository_root().join("hosts/macos/runner-host.example.json")
}

#[test]
fn accepts_the_axpc_example_and_reports_the_default_labels() {
    let summary = check(&axpc_example(), Profile::Axpc).unwrap();
    assert!(
        summary.starts_with("Host configuration is valid: 2 entries, image actions-runner:local\n")
    );
    assert!(
        summary.contains("labels:            self-hosted, linux, x64, axpc, example-label-one\n")
    );
    assert!(!summary.contains('\r'), "the summary uses \\n line endings");
}

#[test]
fn accepts_the_axpc_example_saved_with_a_byte_order_mark() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(fs::read(axpc_example()).unwrap());
    let scratch = TempDir::new();
    let marked = scratch.write("runner-host.example.json", &bytes);
    assert_eq!(
        check(&marked, Profile::Axpc).unwrap(),
        check(&axpc_example(), Profile::Axpc).unwrap()
    );
}

#[test]
fn accepts_the_axmc_example() {
    let summary = check(&axmc_example(), Profile::Axmc).unwrap();
    assert!(summary.contains("macOS VMs:         0 of 2\n"));
    assert!(summary.contains("disk:              120 of 120 GB, including a 40 GB reserve\n"));
    assert!(
        summary.contains("labels:            self-hosted, linux, ARM64, axmc, example-label-one\n")
    );
}

#[test]
fn the_axmc_example_is_not_an_axpc_configuration() {
    let message = check(&axmc_example(), Profile::Axpc).unwrap_err();
    assert!(message.contains("configuration.budget: unknown field"));
}

#[test]
fn rejects_with_every_error_in_the_validation_format() {
    let fixture =
        repository_root().join("tests/host-configuration/shared/rejected/integer-literals.json");
    assert_eq!(
        check(&fixture, Profile::Axpc).unwrap_err(),
        format!(
            "Invalid host configuration ({}):\n  - repositories[0].slots: must be an integer from 1 to 16\n  - repositories[0].memoryGb: must be an integer from 1 to 256",
            fixture.display()
        )
    );
}

#[test]
fn a_missing_file_is_rejected() {
    let scratch = TempDir::new();
    let message = check(&scratch.path().join("absent.json"), Profile::Axpc).unwrap_err();
    assert!(message.starts_with("Host configuration file not found: "));
}

#[test]
fn a_file_over_the_size_limit_is_rejected_without_being_parsed() {
    let limit = usize::try_from(marshal_cli::command::MAX_CONFIG_BYTES).unwrap();
    let mut valid = fs::read(axpc_example()).unwrap();
    let scratch = TempDir::new();

    valid.resize(limit, b' ');
    let at_limit = scratch.write("at-limit.json", &valid);
    check(&at_limit, Profile::Axpc).unwrap();

    valid.push(b' ');
    let over = scratch.write("over.json", &valid);
    let message = check(&over, Profile::Axpc).unwrap_err();
    assert!(
        message.starts_with("Host configuration cannot be read: "),
        "{message}"
    );
    assert!(
        message.contains("the file is larger than 1048576 bytes (1 MiB)"),
        "{message}"
    );
}

#[test]
fn invalid_json_and_invalid_utf8_are_rejected() {
    let scratch = TempDir::new();
    for (name, bytes) in [
        ("json.json", &b"{\"a\": "[..]),
        ("utf8.json", &b"{\"a\": \"\xff\"}"[..]),
    ] {
        let path = scratch.write(name, bytes);
        let message = check(&path, Profile::Axpc).unwrap_err();
        assert!(
            message.starts_with("Host configuration is not valid JSON: "),
            "{name}"
        );
    }
}

#[test]
fn a_root_that_is_not_an_object_keeps_the_path_in_the_header() {
    let scratch = TempDir::new();
    let path = scratch.write("array.json", b"[]");
    assert_eq!(
        check(&path, Profile::Axpc).unwrap_err(),
        format!(
            "Invalid host configuration ({}):\n  - configuration: must be an object",
            path.display()
        )
    );
}

#[test]
fn never_writes_to_the_configuration() {
    let scratch = TempDir::new();
    let bytes = fs::read(axpc_example()).unwrap();
    let path = scratch.write("runner-host.json", &bytes);
    let _ = check(&path, Profile::Axpc);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
}

/// the app's executable dispatches its hidden modes through `command::run`, so a
/// user-facing command there would serve protocol version 1 again: each is a usage
/// error, and a valid `validate` prints no plan.
#[test]
fn the_hidden_mode_dispatch_runs_no_user_facing_command() {
    let example = axpc_example();
    let example = example.to_str().unwrap();
    for words in [
        vec!["validate", "--config", example, "--profile", "axpc"],
        vec!["build", "--config", example, "--profile", "axpc"],
        vec!["start", "--config", example, "--profile", "axpc"],
        vec!["stop"],
        vec!["restart"],
        vec!["status"],
        vec!["diagnostics"],
        vec!["--version"],
    ] {
        let args = std::iter::once("marshal-app")
            .chain(words.iter().copied())
            .map(std::ffi::OsString::from);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = marshal_cli::command::run(args, &mut out, &mut err);
        assert_eq!(code, std::process::ExitCode::from(2), "{words:?}");
        assert!(
            out.is_empty(),
            "{words:?}: {}",
            String::from_utf8_lossy(&out)
        );
    }
}
