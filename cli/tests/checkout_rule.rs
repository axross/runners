//! The rule that `diagnosticDirectory` lies outside the project checkout, found by
//! walking up from the configuration file to the first directory that holds both
//! `.git` and `cli/Cargo.toml`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use support::{TempDir, run_path, stderr, stdout};

const NOTE: &str = "no checkout of this project was found";

/// A temporary tree shaped like a checkout, with the configuration under it.
fn checkout() -> TempDir {
    let tree = TempDir::new();
    tree.write("checkout/.git/HEAD", b"ref: refs/heads/main\n");
    tree.write("checkout/cli/Cargo.toml", b"[package]\n");
    tree
}

fn escape(path: &str) -> String {
    path.replace('\\', "\\\\")
}

#[cfg(unix)]
fn axmc_configuration(directory: &str) -> String {
    format!(
        r#"{{
  "budget": {{ "cpus": 8, "memoryGb": 32, "diskGb": 200 }},
  "repositories": [
    {{
      "owner": "example-owner",
      "repository": "example-repo",
      "name": "example",
      "os": "linux",
      "slots": 1,
      "tokenPath": "/path/to/example.token",
      "diskGb": 40,
      "diagnostics": true,
      "diagnosticDirectory": "{}"
    }}
  ]
}}"#,
        escape(directory)
    )
}

#[cfg(unix)]
#[test]
fn axmc_rejects_a_directory_inside_the_checkout() {
    let tree = checkout();
    let root = tree.path().join("checkout");
    for directory in [root.join("evidence"), root.join("a/b"), root.clone()] {
        let directory = directory.to_str().unwrap().to_string();
        let config = tree.write(
            "checkout/config/host.json",
            axmc_configuration(&directory).as_bytes(),
        );
        let output = run_path(&config, Some("axmc"));
        assert_eq!(
            output.status.code(),
            Some(1),
            "{directory}: {}",
            stdout(&output)
        );
        assert!(
            stderr(&output)
                .contains("repositories[0].diagnosticDirectory: must be outside the checkout"),
            "{directory}: {}",
            stderr(&output)
        );
        assert!(!stderr(&output).contains(NOTE));
    }
}

#[cfg(unix)]
#[test]
fn axmc_accepts_a_directory_outside_the_checkout_and_does_not_confuse_a_sibling_prefix() {
    let tree = checkout();
    let root = tree.path().join("checkout");
    for directory in [
        tree.path().join("evidence"),
        tree.path().join("checkout-evidence"),
        root.with_extension("d"),
    ] {
        let directory = directory.to_str().unwrap().to_string();
        let config = tree.write(
            "checkout/config/host.json",
            axmc_configuration(&directory).as_bytes(),
        );
        let output = run_path(&config, Some("axmc"));
        assert_eq!(
            output.status.code(),
            Some(0),
            "{directory}: {}",
            stderr(&output)
        );
        assert!(!stderr(&output).contains(NOTE));
    }
}

#[cfg(unix)]
#[test]
fn axmc_compares_case_sensitively() {
    let tree = checkout();
    let directory = tree.path().join("CHECKOUT/evidence");
    let config = tree.write(
        "checkout/config/host.json",
        axmc_configuration(directory.to_str().unwrap()).as_bytes(),
    );
    assert_eq!(run_path(&config, Some("axmc")).status.code(), Some(0));
}

#[cfg(unix)]
#[test]
fn a_directory_holding_only_dot_git_is_not_a_checkout_and_the_rule_reports_it() {
    let tree = TempDir::new();
    tree.write("repository/.git/HEAD", b"ref: refs/heads/main\n");
    let directory = tree.path().join("repository/evidence");
    let config = tree.write(
        "repository/config/host.json",
        axmc_configuration(directory.to_str().unwrap()).as_bytes(),
    );
    let output = run_path(&config, Some("axmc"));
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stderr(&output).contains(NOTE), "{}", stderr(&output));
    assert!(
        !stdout(&output).contains(NOTE),
        "the summary on standard output is unchanged"
    );
}

#[cfg(unix)]
#[test]
fn another_repository_between_the_file_and_the_checkout_does_not_count() {
    let tree = checkout();
    tree.write("checkout/dotfiles/.git/HEAD", b"ref: refs/heads/main\n");
    let directory = tree.path().join("checkout/evidence");
    let config = tree.write(
        "checkout/dotfiles/config/host.json",
        axmc_configuration(directory.to_str().unwrap()).as_bytes(),
    );
    let output = run_path(&config, Some("axmc"));
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(stderr(&output).contains("must be outside the checkout"));
}

#[cfg(unix)]
#[test]
fn a_git_file_marks_a_worktree_checkout() {
    let tree = TempDir::new();
    tree.write("worktree/.git", b"gitdir: /elsewhere\n");
    tree.write("worktree/cli/Cargo.toml", b"[package]\n");
    let directory = tree.path().join("worktree/evidence");
    let config = tree.write(
        "worktree/config/host.json",
        axmc_configuration(directory.to_str().unwrap()).as_bytes(),
    );
    assert_eq!(run_path(&config, Some("axmc")).status.code(), Some(1));
}

#[cfg(unix)]
#[test]
fn no_note_is_printed_when_no_entry_enables_diagnostics() {
    let tree = TempDir::new();
    let config = tree.write(
        "host.json",
        axmc_configuration("/path/to/evidence")
            .replace("\"diagnostics\": true", "\"diagnostics\": false")
            .replace(
                ",\n      \"diagnosticDirectory\": \"/path/to/evidence\"",
                "",
            )
            .as_bytes(),
    );
    let output = run_path(&config, Some("axmc"));
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stderr(&output), "");
}

#[cfg(unix)]
#[test]
fn axpc_on_a_unix_machine_cannot_bite_because_its_paths_are_windows_paths() {
    let tree = checkout();
    let config = tree.write(
        "checkout/config/host.json",
        br#"{
  "imageName": "actions-runner:local",
  "repositories": [
    {
      "owner": "example-owner",
      "repository": "example-repo",
      "name": "example",
      "slots": 1,
      "tokenPath": "C:\\path\\to\\example.token",
      "volumes": [],
      "diagnostics": true,
      "diagnosticDirectory": "C:\\evidence"
    }
  ]
}"#,
    );
    let output = run_path(&config, Some("axpc"));
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
}

#[cfg(windows)]
fn axpc_configuration(directory: &str) -> String {
    format!(
        r#"{{
  "imageName": "actions-runner:local",
  "repositories": [
    {{
      "owner": "example-owner",
      "repository": "example-repo",
      "name": "example",
      "slots": 1,
      "tokenPath": "C:\\path\\to\\example.token",
      "volumes": [],
      "diagnostics": true,
      "diagnosticDirectory": "{}"
    }}
  ]
}}"#,
        escape(directory)
    )
}

#[cfg(windows)]
#[test]
fn axpc_rejects_a_directory_inside_the_checkout_whatever_its_case() {
    let tree = checkout();
    let root = tree.path().join("checkout");
    let root = root.to_str().unwrap().to_string();
    for directory in [
        format!("{root}\\evidence"),
        format!("{root}\\"),
        root.to_uppercase(),
        root.to_lowercase() + "\\Evidence",
        format!("{root}\\\\a\\b"),
    ] {
        let config = tree.write(
            "checkout/config/host.json",
            axpc_configuration(&directory).as_bytes(),
        );
        let output = run_path(&config, Some("axpc"));
        assert_eq!(
            output.status.code(),
            Some(1),
            "{directory}: {}",
            stdout(&output)
        );
        assert!(
            stderr(&output)
                .contains("repositories[0].diagnosticDirectory: must be outside the checkout"),
            "{directory}: {}",
            stderr(&output)
        );
    }
}

#[cfg(windows)]
#[test]
fn axpc_accepts_a_directory_outside_the_checkout() {
    let tree = checkout();
    let outside = tree.path().join("evidence");
    let sibling = tree.path().join("checkout-evidence");
    for directory in [outside, sibling] {
        let config = tree.write(
            "checkout/config/host.json",
            axpc_configuration(directory.to_str().unwrap()).as_bytes(),
        );
        let output = run_path(&config, Some("axpc"));
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}: {}",
            directory.display(),
            stderr(&output)
        );
    }
}

#[cfg(windows)]
#[test]
fn axpc_ignores_a_directory_holding_only_dot_git() {
    let tree = TempDir::new();
    tree.write("repository/.git/HEAD", b"ref: refs/heads/main\n");
    let directory = tree.path().join("repository/evidence");
    let config = tree.write(
        "repository/config/host.json",
        axpc_configuration(directory.to_str().unwrap()).as_bytes(),
    );
    let output = run_path(&config, Some("axpc"));
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stderr(&output).contains(NOTE));
}
