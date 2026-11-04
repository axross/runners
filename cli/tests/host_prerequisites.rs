//! the host prerequisite checks run against the fake `tart` and the fake `softnet`:
//! the version of each, Softnet's file, and finding Softnet on `PATH` with links
//! followed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::ffi::OsString;
use std::time::{Duration, Instant};

use marshal_cli::macos::prerequisites::{
    Cause, Check, HOST_DOCUMENT, INSTALL_SECTION, Refusal, SIGNED_IN_SECTION, SOFTNET_SECTION,
    VERSION_BUDGET, check_tart, check_tart_within,
};
use support::repository_root;
use support::tart::Fake;

const BUDGET: Duration = Duration::from_secs(20);
const SHORT_BUDGET: Duration = Duration::from_millis(400);

const TART_INSTALL: &str = "Install Tart 2.41.1 as the \"Installing Tart and Softnet\" section of the macOS Runner Host document describes.";

fn refusal(check: Check) -> Refusal {
    match check {
        Check::Refuse(refusal) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn each_version_read_is_bounded_to_five_seconds() {
    assert_eq!(VERSION_BUDGET, Duration::from_secs(5));
}

/// the messages send the operator to the host document by these titles, so the
/// document must keep them.
#[test]
fn the_host_document_has_the_title_and_the_sections_the_messages_name() {
    let path = repository_root().join("docs/operations/macos-runner-host.md");
    let document = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = document.lines().collect();
    let titles: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("# "))
        .collect();
    assert_eq!(titles, [HOST_DOCUMENT], "{}", path.display());
    for section in [INSTALL_SECTION, SOFTNET_SECTION, SIGNED_IN_SECTION] {
        assert!(
            lines.contains(&format!("## {section}").as_str()),
            "{} has no `## {section}` heading",
            path.display()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_tested_tart_version_runs() {
    let fake = Fake::new();
    let check = check_tart_within(&fake.client, BUDGET).await;
    assert_eq!(
        check,
        Check::Pass {
            version: "2.41.1".to_string()
        }
    );
    let calls = fake.invocations();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments, ["--version"]);
    assert_eq!(calls[0].no_auto_prune.as_deref(), Some("1"));
    assert_eq!(calls[0].path.as_deref(), fake.path.to_str());
    assert!(!calls[0].stdout_terminal && !calls[0].stdin_terminal);
    #[cfg(unix)]
    assert_eq!(calls[0].pgid, Some(calls[0].pid));
}

#[tokio::test(flavor = "current_thread")]
async fn a_newer_patch_of_the_tested_tart_minor_version_runs_with_a_warning_that_names_the_tested_version()
 {
    let fake = Fake::new();
    fake.script("version_output", "2.41.2\n");
    assert_eq!(
        check_tart_within(&fake.client, BUDGET).await,
        Check::Warn {
            version: "2.41.2".to_string(),
            message: "Tart 2.41.2 is a newer patch of the tested version 2.41.1; it runs, but only 2.41.1 was tested.".to_string()
        }
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tart_versions_that_do_not_run_are_refused_with_the_tested_version_and_the_install_section()
{
    let fake = Fake::new();
    let versions = [
        (
            "2.41.0\n",
            "Tart 2.41.0 is older than the tested version 2.41.1.",
        ),
        (
            "2.40.9\n",
            "Tart 2.40.9 is not the tested version 2.41.1 or a newer patch of 2.41.",
        ),
        (
            "2.42.0\n",
            "Tart 2.42.0 is not the tested version 2.41.1 or a newer patch of 2.41.",
        ),
        (
            "3.41.1\n",
            "Tart 3.41.1 is not the tested version 2.41.1 or a newer patch of 2.41.",
        ),
        (
            "1.41.1\n",
            "Tart 1.41.1 is not the tested version 2.41.1 or a newer patch of 2.41.",
        ),
    ];
    for (output, reason) in versions {
        fake.script("version_output", output);
        let refusal = refusal(check_tart_within(&fake.client, BUDGET).await);
        assert_eq!(refusal.cause, Cause::Version, "{output:?}");
        assert_eq!(
            refusal.message,
            format!("{reason} {TART_INSTALL}"),
            "{output:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn tart_output_that_is_not_exactly_three_numbers_is_refused() {
    let fake = Fake::new();
    for output in [
        "2.41.1.0\n",
        "2.41.1-beta\n",
        "2.41.1+1\n",
        "v2.41.1\n",
        "tart 2.41.1\n",
        "2.41\n",
        "2.41.1 \n",
        "2.41.1\n\n",
        "SNAPSHOT\n",
        "",
        "\n",
        "no version here\n",
    ] {
        fake.script("version_output", output);
        let refusal = refusal(check_tart_within(&fake.client, BUDGET).await);
        assert_eq!(refusal.cause, Cause::Version, "{output:?}");
        assert!(
            refusal
                .message
                .contains("exactly three dot-separated numbers")
                && refusal.message.ends_with(TART_INSTALL),
            "{output:?}: {}",
            refusal.message
        );
    }
    fake.script("version_output", "");
    let message = refusal(check_tart_within(&fake.client, BUDGET).await).message;
    assert!(message.starts_with("\"tart --version\" printed nothing, which is not"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_tart_that_fails_its_version_call_is_refused_with_its_error() {
    let fake = Fake::new();
    fake.script("fail-version", "Error:   no   such\nthing");
    let refusal = refusal(check_tart_within(&fake.client, BUDGET).await);
    assert_eq!(refusal.cause, Cause::Version);
    assert_eq!(
        refusal.message,
        format!("\"tart --version\" failed: Error: no such thing. {TART_INSTALL}")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tart_output_beyond_the_bound_is_refused_and_not_read_as_a_version() {
    let fake = Fake::new();
    fake.script("flood-version", "");
    let refusal = refusal(check_tart_within(&fake.client, BUDGET).await);
    assert_eq!(refusal.cause, Cause::Version);
    assert!(
        refusal.message.contains("printed more than 65536 bytes"),
        "{}",
        refusal.message
    );
    assert!(refusal.message.chars().count() < 1_000);
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_tart_is_refused_naming_the_install_section() {
    let fake = Fake::with_config(|config| {
        config.program = OsString::from("/nonexistent-directory/tart");
    });
    let refusal = refusal(check_tart_within(&fake.client, BUDGET).await);
    assert_eq!(refusal.cause, Cause::Missing);
    assert!(
        refusal.message.starts_with("Tart could not be started ("),
        "{}",
        refusal.message
    );
    assert!(refusal.message.ends_with(TART_INSTALL));
}

#[tokio::test(flavor = "current_thread")]
async fn a_tart_that_does_not_answer_in_time_is_not_answering_and_is_ended() {
    let fake = Fake::new();
    fake.script("hang-version", "");
    let started = Instant::now();
    let refusal = refusal(check_tart_within(&fake.client, SHORT_BUDGET).await);
    assert_eq!(refusal.cause, Cause::NotAnswering);
    assert!(
        refusal
            .message
            .starts_with("\"tart --version\" did not answer within")
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(fake.ended_within(0, Duration::from_secs(10)));
}

#[tokio::test(flavor = "current_thread")]
async fn the_default_check_gives_tart_five_seconds() {
    let fake = Fake::new();
    fake.script("hang-version", "");
    let started = Instant::now();
    let refusal = refusal(check_tart(&fake.client).await);
    assert_eq!(refusal.cause, Cause::NotAnswering);
    assert_eq!(
        refusal.message,
        "\"tart --version\" did not answer within 5 s."
    );
    let waited = started.elapsed();
    assert!(waited >= Duration::from_secs(5), "{waited:?}");
    assert!(waited < Duration::from_secs(20), "{waited:?}");
}

#[cfg(unix)]
mod softnet {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    use marshal_cli::macos::prerequisites::{
        Cause, Check, VERSION_BUDGET, check_softnet, check_softnet_with,
    };
    use marshal_cli::macos::softnet::{
        FileKind, OwnerMode, SOFTNET_PROGRAM, TEST_OWNER_MODE_VARIABLE, find_on_path,
        read_file_facts,
    };

    use super::support::softnet::Host;
    use super::{BUDGET, SHORT_BUDGET, refusal};

    const ROOT_SUID: OwnerMode = OwnerMode {
        uid: 0,
        mode: 0o4755,
    };
    const SOFTNET_INSTALL: &str = "Install Softnet 0.25.0 as the \"Installing Tart and Softnet\" section of the macOS Runner Host document describes.";
    const SET_UP: &str = "Set it up as the \"Setting Up Softnet\" section of the macOS Runner Host document describes.";

    #[tokio::test(flavor = "current_thread")]
    async fn the_tested_softnet_version_in_the_form_softnet_prints_runs() {
        let host = Host::new();
        host.link("bin");
        let path = host.path_with_system(&["bin"]);
        let check = check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await;
        assert_eq!(
            check,
            Check::Pass {
                version: "0.25.0-0f1e2d3".to_string()
            }
        );
        let calls = host.invocations("bin");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments, ["--version"]);
        assert_eq!(calls[0].path.as_deref(), path.to_str());
        assert!(!calls[0].jit_present);
        assert!(!calls[0].stdin_terminal && !calls[0].stdout_terminal);
        assert_eq!(calls[0].pgid, Some(calls[0].pid));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_newer_patch_of_the_tested_softnet_minor_version_runs_with_a_warning() {
        let host = Host::new();
        host.link("bin");
        host.script("bin", "version_output", "softnet 0.25.3-abcdef0\n");
        let path = host.path(&["bin"]);
        assert_eq!(
            check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await,
            Check::Warn {
                version: "0.25.3-abcdef0".to_string(),
                message: "Softnet 0.25.3 is a newer patch of the tested version 0.25.0; it runs, but only 0.25.0 was tested.".to_string()
            }
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn softnet_versions_that_do_not_run_are_refused_with_the_tested_version_and_the_install_section()
     {
        let host = Host::new();
        host.link("bin");
        let path = host.path(&["bin"]);
        let other = "is not the tested version 0.25.0 or a newer patch of 0.25.";
        let versions = [
            (
                "softnet 0.24.9-abcdef0\n",
                format!("Softnet 0.24.9 {other}"),
            ),
            (
                "softnet 0.26.0-abcdef0\n",
                format!("Softnet 0.26.0 {other}"),
            ),
            (
                "softnet 1.25.0-abcdef0\n",
                format!("Softnet 1.25.0 {other}"),
            ),
        ];
        for (output, reason) in versions {
            host.script("bin", "version_output", output);
            let refusal = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await);
            assert_eq!(refusal.cause, Cause::Version, "{output:?}");
            assert_eq!(
                refusal.message,
                format!("{reason} {SOFTNET_INSTALL}"),
                "{output:?}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn softnet_output_that_is_not_its_release_form_is_refused() {
        let host = Host::new();
        host.link("bin");
        let path = host.path(&["bin"]);
        for output in [
            "",
            "\n",
            "0.25.0-abcdef0\n",
            "softnet 0.25.0\n",
            "softnet 0.25.0.1-abcdef0\n",
            "softnet 0.25.0-abcdef0-dirty\n",
            "softnet 0.25.0-rc1\n",
            "softnet unknown-unknown\n",
            "softnet 0.25.0-abcdef0\nsoftnet 0.25.0-abcdef0\n",
            "softnet 0.25.0-ABCDEF0\n",
            "no version here\n",
        ] {
            host.script("bin", "version_output", output);
            let refusal = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await);
            assert_eq!(refusal.cause, Cause::Version, "{output:?}");
            assert!(
                refusal.message.contains("a hyphen and a short commit")
                    && refusal.message.ends_with(SOFTNET_INSTALL),
                "{output:?}: {}",
                refusal.message
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_softnet_that_fails_or_floods_its_version_call_is_refused() {
        let host = Host::new();
        host.link("bin");
        let path = host.path(&["bin"]);
        host.script("bin", "fail-version", "Error:  broken");
        let failed = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await);
        assert_eq!(failed.cause, Cause::Version);
        assert_eq!(
            failed.message,
            format!("\"softnet --version\" failed: Error: broken. {SOFTNET_INSTALL}")
        );
        std::fs::remove_file(host.directory("bin").join("fail-version")).unwrap();
        host.script("bin", "flood-version", "");
        let flooded = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await);
        assert_eq!(flooded.cause, Cause::Version);
        assert!(flooded.message.contains("printed more than 65536 bytes"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_softnet_that_does_not_answer_in_time_is_not_answering_and_is_ended() {
        let host = Host::new();
        host.link("bin");
        host.script("bin", "hang-version", "");
        let path = host.path(&["bin"]);
        let started = std::time::Instant::now();
        let refusal = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), SHORT_BUDGET).await);
        assert_eq!(refusal.cause, Cause::NotAnswering);
        assert_eq!(
            refusal.message,
            "\"softnet --version\" did not answer within 0 s. Check Softnet as the \"Setting Up Softnet\" section of the macOS Runner Host document describes."
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        assert!(host.ended_within("bin", 0, std::time::Duration::from_secs(10)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_missing_softnet_is_refused_naming_the_install_section() {
        let host = Host::new();
        host.directory("empty");
        host.directory("other");
        let path = host.path(&["empty", "other"]);
        let missing = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await);
        assert_eq!(missing.cause, Cause::Missing);
        assert_eq!(
            missing.message,
            format!("Softnet was not found on PATH. {SOFTNET_INSTALL}")
        );
        let unset = refusal(check_softnet_with(None, Some(ROOT_SUID), BUDGET).await);
        assert_eq!(unset.cause, Cause::Missing);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_softnet_that_cannot_start_is_refused_as_missing() {
        let host = Host::new();
        let file = host.copy("bin");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let path = host.path(&["bin"]);
        let refusal = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await);
        assert_eq!(refusal.cause, Cause::Missing);
        assert!(
            refusal
                .message
                .starts_with("Softnet could not be started ("),
            "{}",
            refusal.message
        );
        assert!(refusal.message.ends_with(SOFTNET_INSTALL));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn softnet_is_the_first_match_on_the_path_found_through_links() {
        let host = Host::new();
        host.directory("first");
        let dangling = host.directory("dangling").join("softnet");
        symlink(host.root.path().join("nowhere"), &dangling).unwrap();
        let middle = host.directory("middle").join("softnet");
        symlink(super::support::softnet::FAKE_SOFTNET, &middle).unwrap();
        let linked = host.directory("linked").join("softnet");
        symlink(&middle, &linked).unwrap();
        host.link("later");
        host.script("later", "version_output", "softnet 0.1.0-abcdef0\n");
        let path = host.path(&["first", "dangling", "linked", "later"]);
        assert_eq!(find_on_path(Some(&path)), Some(linked.clone()));
        host.script("linked", "version_output", "softnet 0.25.1-abcdef0\n");
        let check = check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await;
        assert!(
            matches!(&check, Check::Warn { version, .. } if version == "0.25.1-abcdef0"),
            "{check:?}"
        );
        assert_eq!(host.invocations("linked").len(), 1);
        assert!(host.invocations("later").is_empty());
        assert!(host.invocations("middle").is_empty());
    }

    #[test]
    fn empty_path_entries_are_skipped_and_the_first_existing_file_of_any_type_matches() {
        let host = Host::new();
        let directory = host.directory("odd").join(SOFTNET_PROGRAM);
        std::fs::create_dir_all(&directory).unwrap();
        let file = host.copy("real");
        let path = host.path(&["odd", "real"]);
        assert_eq!(find_on_path(Some(&path)), Some(directory));
        let with_empty =
            std::env::join_paths([std::path::PathBuf::new(), host.root.path().join("real")])
                .unwrap();
        assert_eq!(find_on_path(Some(&with_empty)), Some(file));
        assert_eq!(find_on_path(Some(std::ffi::OsStr::new(""))), None);
        assert_eq!(find_on_path(None), None);
    }

    #[test]
    fn the_file_is_read_through_links_for_its_type_owner_and_mode() {
        let host = Host::new();
        let target = host.copy("real");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = host.directory("links").join("softnet");
        symlink(&target, &link).unwrap();
        let second = host.directory("more").join("softnet");
        symlink(&link, &second).unwrap();
        let metadata = std::fs::metadata(&target).unwrap();
        for file in [&target, &link, &second] {
            let facts = read_file_facts(file, None).unwrap();
            assert_eq!(facts.kind, FileKind::Regular);
            assert_eq!(facts.owner_mode.uid, metadata.uid());
            assert_eq!(facts.owner_mode.mode, 0o640, "{}", file.display());
        }
        assert_ne!(
            std::fs::symlink_metadata(&link).unwrap().mode() & 0o777,
            0o640,
            "the link's own mode differs from its target's"
        );
        let directory_link = host.directory("dir-links").join("softnet");
        symlink(host.directory("real"), &directory_link).unwrap();
        assert_eq!(
            read_file_facts(&directory_link, None).unwrap().kind,
            FileKind::Directory
        );
        assert!(read_file_facts(&host.root.path().join("absent"), None).is_err());
    }

    #[test]
    fn the_override_replaces_the_owner_and_mode_but_not_the_type() {
        let host = Host::new();
        let file = host.copy("bin");
        let facts = read_file_facts(&file, Some(ROOT_SUID)).unwrap();
        assert_eq!(facts.kind, FileKind::Regular);
        assert_eq!(facts.owner_mode, ROOT_SUID);
        let directory = host.directory("dir");
        let facts = read_file_facts(&directory, Some(ROOT_SUID)).unwrap();
        assert_eq!(facts.kind, FileKind::Directory);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_file_that_is_not_set_up_is_refused_before_softnet_runs() {
        let host = Host::new();
        host.link("bin");
        let path = host.path(&["bin"]);
        let shown = host.root.path().join("bin").join("softnet");
        let shown = shown.display();
        let cases = [
            (
                OwnerMode {
                    uid: 501,
                    mode: 0o4755,
                },
                "is owned by user 501, not root, so it cannot run with root privilege",
            ),
            (
                OwnerMode {
                    uid: 0,
                    mode: 0o0755,
                },
                "does not have the SUID bit set, so it cannot run with root privilege",
            ),
            (
                OwnerMode {
                    uid: 0,
                    mode: 0o4775,
                },
                "can be written by its group",
            ),
            (
                OwnerMode {
                    uid: 0,
                    mode: 0o4757,
                },
                "can be written by others",
            ),
        ];
        for (owner_mode, problem) in cases {
            let refusal = refusal(check_softnet_with(Some(&path), Some(owner_mode), BUDGET).await);
            assert_eq!(refusal.cause, Cause::Setup, "{owner_mode:?}");
            assert_eq!(
                refusal.message,
                format!("Softnet at {shown} {problem}. {SET_UP}"),
                "{owner_mode:?}"
            );
        }
        assert!(host.invocations("bin").is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_directory_called_softnet_is_the_match_and_is_refused() {
        let host = Host::new();
        std::fs::create_dir_all(host.directory("bin").join("softnet")).unwrap();
        host.link("later");
        let path = host.path(&["bin", "later"]);
        let refusal = refusal(check_softnet_with(Some(&path), Some(ROOT_SUID), BUDGET).await);
        assert_eq!(refusal.cause, Cause::Setup);
        assert!(
            refusal
                .message
                .ends_with(&format!("is a directory, not a regular file. {SET_UP}")),
            "{}",
            refusal.message
        );
        assert!(host.invocations("later").is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_real_file_is_judged_when_nothing_overrides_it() {
        let host = Host::new();
        let file = host.copy("bin");
        let path = host.path(&["bin"]);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o4755)).unwrap();
        let mine = std::fs::metadata(&file).unwrap().uid();
        let check = check_softnet_with(Some(&path), None, BUDGET).await;
        if mine == 0 {
            assert!(matches!(check, Check::Pass { .. }), "{check:?}");
        } else {
            let refusal = refusal(check);
            assert_eq!(refusal.cause, Cause::Setup);
            assert!(
                refusal
                    .message
                    .contains(&format!("is owned by user {mine}, not root")),
                "{}",
                refusal.message
            );
            assert!(host.invocations("bin").is_empty());
        }
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o0755)).unwrap();
        let refusal = refusal(check_softnet_with(Some(&path), None, BUDGET).await);
        assert_eq!(refusal.cause, Cause::Setup);
        assert!(refusal.message.ends_with(SET_UP));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_default_check_judges_the_real_file_when_the_environment_names_no_override() {
        let host = Host::new();
        let file = host.copy("bin");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o0755)).unwrap();
        let path = host.path(&["bin"]);
        let refusal = refusal(check_softnet(Some(&path)).await);
        assert_eq!(refusal.cause, Cause::Setup);
        assert!(host.invocations("bin").is_empty());
        assert_eq!(TEST_OWNER_MODE_VARIABLE, "MARSHAL_TEST_SOFTNET_OWNER_MODE");
        assert_eq!(VERSION_BUDGET, std::time::Duration::from_secs(5));
    }
}
