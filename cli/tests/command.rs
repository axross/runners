//! The command line, run as the built binary.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;

use support::{TempDir, repository_root, run, run_path, stderr, stdout};

fn axpc_example() -> std::path::PathBuf {
    repository_root().join("hosts/windows-docker-desktop/runner-host.example.json")
}

fn axmc_example() -> std::path::PathBuf {
    repository_root().join("hosts/macos/runner-host.example.json")
}

#[test]
fn accepts_the_axpc_example_and_reports_the_default_labels() {
    let output = run_path(&axpc_example(), Some("axpc"));
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let summary = stdout(&output);
    assert!(
        summary.starts_with("Host configuration is valid: 2 entries, image actions-runner:local\n")
    );
    assert!(
        summary.contains("labels:            self-hosted, linux, x64, axpc, example-label-one\n")
    );
    assert!(!summary.contains('\r'), "the summary uses \\n line endings");
    assert_eq!(stderr(&output), "");
}

#[test]
fn accepts_the_axpc_example_saved_with_a_byte_order_mark() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(fs::read(axpc_example()).unwrap());
    let scratch = TempDir::new();
    let marked = scratch.write("runner-host.example.json", &bytes);
    let plain = run_path(&axpc_example(), Some("axpc"));
    let with_mark = run_path(&marked, Some("axpc"));
    assert_eq!(with_mark.status.code(), Some(0), "{}", stderr(&with_mark));
    assert_eq!(stdout(&with_mark), stdout(&plain));
}

#[test]
fn accepts_the_axmc_example() {
    let output = run_path(&axmc_example(), Some("axmc"));
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let summary = stdout(&output);
    assert!(summary.contains("macOS VMs:         1 of 2\n"));
    assert!(
        summary.contains("labels:            self-hosted, macOS, ARM64, axmc, example-label-two\n")
    );
}

#[test]
fn the_axmc_example_is_not_an_axpc_configuration() {
    let output = run_path(&axmc_example(), Some("axpc"));
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("configuration.budget: unknown field"));
}

#[test]
fn rejects_with_every_error_in_the_powershell_format_and_writes_nothing_to_stdout() {
    let fixture =
        repository_root().join("tests/host-configuration/shared/rejected/integer-literals.json");
    let output = run_path(&fixture, Some("axpc"));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        format!(
            "Invalid host configuration ({}):\n  - repositories[0].slots: must be an integer from 1 to 16\n  - repositories[0].memoryGb: must be an integer from 1 to 256\n",
            fixture.display()
        )
    );
}

#[test]
fn a_missing_file_is_rejected_with_exit_1() {
    let scratch = TempDir::new();
    let output = run_path(&scratch.path().join("absent.json"), Some("axpc"));
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("Host configuration file not found: "));
}

#[test]
fn a_file_over_the_size_limit_is_rejected_without_being_parsed() {
    let limit = usize::try_from(marshal_cli::command::MAX_CONFIG_BYTES).unwrap();
    let mut valid = fs::read(axpc_example()).unwrap();
    let scratch = TempDir::new();

    // exactly at the limit, padded with whitespace after a valid document
    valid.resize(limit, b' ');
    let at_limit = scratch.write("at-limit.json", &valid);
    let output = run_path(&at_limit, Some("axpc"));
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    // one byte more is refused before parsing
    valid.push(b' ');
    let over = scratch.write("over.json", &valid);
    let output = run_path(&over, Some("axpc"));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    let message = stderr(&output);
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
fn invalid_json_and_invalid_utf8_are_rejected_with_exit_1() {
    let scratch = TempDir::new();
    for (name, bytes) in [
        ("json.json", &b"{\"a\": "[..]),
        ("utf8.json", &b"{\"a\": \"\xff\"}"[..]),
    ] {
        let path = scratch.write(name, bytes);
        let output = run_path(&path, Some("axpc"));
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert!(
            stderr(&output).starts_with("Host configuration is not valid JSON: "),
            "{name}"
        );
    }
}

#[test]
fn a_root_that_is_not_an_object_keeps_the_path_in_the_header() {
    let scratch = TempDir::new();
    let path = scratch.write("array.json", b"[]");
    let output = run_path(&path, Some("axpc"));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        format!(
            "Invalid host configuration ({}):\n  - configuration: must be an object\n",
            path.display()
        )
    );
}

#[test]
fn never_writes_to_the_configuration() {
    let scratch = TempDir::new();
    let bytes = fs::read(axpc_example()).unwrap();
    let path = scratch.write("runner-host.json", &bytes);
    let _ = run_path(&path, Some("axpc"));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
}

#[test]
fn a_bad_profile_or_a_missing_option_is_a_usage_error() {
    let example = axpc_example();
    let example = example.to_str().unwrap();
    for args in [
        vec!["validate", "--config", example, "--profile", "other"],
        vec!["validate", "--profile", "axpc"],
        vec!["validate", "--config"],
        vec!["unknown"],
    ] {
        let output = run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn help_exits_0() {
    let output = run(&["validate", "--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("--config"));
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[test]
fn without_a_profile_on_another_os_it_stops_and_asks_for_one() {
    let output = run_path(&axpc_example(), None);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("--profile axpc or --profile axmc is required"));
}

#[cfg(target_os = "windows")]
#[test]
fn without_a_profile_on_windows_it_checks_for_axpc() {
    let output = run_path(&axpc_example(), None);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}

#[cfg(target_os = "macos")]
#[test]
fn without_a_profile_on_macos_it_checks_for_axmc() {
    let output = run_path(&axmc_example(), None);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}
