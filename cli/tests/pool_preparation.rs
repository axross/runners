//! Pool preparation: token files, diagnostics pools, stale cleanup and volumes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use runners_cli::profile::Profile;
use runners_cli::runtime::backend::{Backend, BackendError};
use runners_cli::runtime::board::Board;
use runners_cli::runtime::fake::{Call, FakeBackend, FakeContainer, MemoryLog};
use runners_cli::runtime::log::Log;
use runners_cli::runtime::pool::{NoPoolStartable, PoolSpec, pools_from_plan, prepare_pools};
use runners_cli::validate::validate;
use support::{TempDir, write_token_file};

const STAMP: &str = "20240305060708009";

fn entry(name: &str, token: &str, extra: &str) -> String {
    format!(
        r#"{{"owner":"o","repository":"{name}","name":"{name}","slots":1,"tokenPath":"C:\\{token}",
        "volumes":[{{"suffix":"npm","mountPath":"/home/runner/.npm"}}]{extra}}}"#
    )
}

/// Pools `alpha`, `bravo` and `charlie`; `bravo` has diagnostics on. Token files
/// are `<name>.token` in `scratch`, written for the names in `with_token`.
fn pools(scratch: &TempDir, with_token: &[(&str, &str)]) -> Vec<PoolSpec> {
    let config = format!(
        r#"{{"imageName":"actions-runner:local","repositories":[{},{},{}]}}"#,
        entry("alpha", "a.token", ""),
        entry(
            "bravo",
            "b.token",
            r#","diagnostics":true,"diagnosticDirectory":"C:\\evidence""#
        ),
        entry("charlie", "c.token", ""),
    );
    let plan = validate(config.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    let mut pools = pools_from_plan(&plan).unwrap();
    for pool in &mut pools {
        pool.token_path = scratch.path().join(format!("{}.token", pool.name));
        if let Some((_, content)) = with_token.iter().find(|(name, _)| *name == pool.name) {
            write_token_file(&pool.token_path, content.as_bytes());
        }
    }
    pools
}

fn container(name: &str, diagnostic: bool) -> FakeContainer {
    FakeContainer {
        name: name.to_string(),
        diagnostic,
    }
}

fn names(prepared: &[PoolSpec]) -> Vec<&str> {
    prepared.iter().map(|pool| pool.name.as_str()).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_or_blank_token_and_a_diagnostics_pool_are_skipped_and_the_rest_run() {
    let scratch = TempDir::new();
    let all = pools(
        &scratch,
        &[
            ("alpha", "ghp_a\n"),
            ("bravo", "ghp_b\n"),
            ("charlie", " \n"),
        ],
    );
    let backend = FakeBackend::default();
    let log = MemoryLog::default();
    let board = Board::default();
    let prepared = prepare_pools(&backend, all.clone(), &log, &board)
        .await
        .unwrap();
    assert_eq!(names(&prepared), ["alpha"]);
    let fields = board.fields();
    assert_eq!(fields.len(), 3, "{fields:?}");
    assert_eq!(fields[0], ("pool", "alpha: running".to_string()));
    assert!(
        fields[1]
            .1
            .starts_with("bravo: skipped: diagnostics are not supported"),
        "{fields:?}"
    );
    assert!(
        fields[2]
            .1
            .starts_with("charlie: skipped: repositories[2]: token file at "),
        "{fields:?}"
    );
    let warnings = log.lines();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0]
            .starts_with("WARN: repositories[1] (o/bravo) is not started and gets no slots: ")
            && warnings[0].contains("diagnostics are not supported by this runtime yet"),
        "{warnings:?}"
    );
    assert!(
        warnings[1].starts_with("WARN: repositories[2] (o/charlie) is not started and gets no slots: repositories[2]: token file at ")
            && warnings[1].ends_with(" is empty."),
        "{warnings:?}"
    );
    // every token read is masked from then on, whatever line it reaches.
    log.info("forwarded: ghp_a and ghp_b");
    assert_eq!(
        log.lines().last().unwrap(),
        "INFO: forwarded: <masked> and <masked>"
    );

    // a missing file is named as missing.
    let fresh = TempDir::new();
    let missing = pools(&fresh, &[("alpha", "ghp_a")]);
    let log = MemoryLog::default();
    let prepared = prepare_pools(&FakeBackend::default(), missing, &log, &Board::default())
        .await
        .unwrap();
    assert_eq!(names(&prepared), ["alpha"]);
    assert!(
        log.lines()[1].contains("token file not found at "),
        "{:?}",
        log.lines()
    );
    // no warning holds a token.
    assert!(log.lines().iter().all(|line| !line.contains("ghp_")));

    // the skipped pools were never touched, and Docker was waited for once.
    let calls = backend.calls();
    assert_eq!(
        calls,
        [
            Call::WaitUntilAvailable,
            Call::ListPoolContainers("alpha".to_string()),
            Call::PrepareVolumes {
                image: "actions-runner:local".to_string(),
                volumes: vec!["alpha-npm".to_string()],
            },
        ]
    );
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "current_thread")]
async fn on_macos_a_token_file_that_is_not_0600_skips_its_pool_and_the_rest_run() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = TempDir::new();
    let all = pools(
        &scratch,
        &[
            ("alpha", "ghp_a\n"),
            ("bravo", "ghp_b\n"),
            ("charlie", "ghp_c\n"),
        ],
    );
    let charlie = all.iter().find(|pool| pool.name == "charlie").unwrap();
    std::fs::set_permissions(&charlie.token_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let log = MemoryLog::default();
    let prepared = prepare_pools(
        &FakeBackend::default(),
        all.clone(),
        &log,
        &Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(names(&prepared), ["alpha"]);
    let warnings = log.lines();
    assert!(
        warnings[1].starts_with("WARN: repositories[2] (o/charlie) is not started")
            && warnings[1].contains("token file is not private")
            && warnings[1].contains("has mode 0644"),
        "{warnings:?}"
    );
    assert!(warnings.iter().all(|line| !line.contains("ghp_")));
}

#[tokio::test(flavor = "current_thread")]
async fn without_a_startable_pool_docker_is_not_waited_for_and_there_is_nothing_to_run() {
    let scratch = TempDir::new();
    let backend = FakeBackend::default();
    let log = MemoryLog::default();
    let result = prepare_pools(&backend, pools(&scratch, &[]), &log, &Board::default()).await;
    assert_eq!(result.unwrap_err(), NoPoolStartable);
    assert!(backend.calls().is_empty());
    assert_eq!(log.lines().len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn stale_cleanup_removes_only_the_pools_own_unlabelled_containers() {
    let scratch = TempDir::new();
    let backend = FakeBackend::default();
    let own = format!("alpha-1-{STAMP}");
    {
        let mut state = backend.state.borrow_mut();
        state.containers = vec![
            container(&own, false),
            container(&format!("alpha-12-{STAMP}"), false),
            container(&format!("alpha-x-{STAMP}"), false),
            container(&format!("alpha-extra-1-{STAMP}"), false),
            container(&format!("alpha-1-{}", &STAMP[1..]), false),
            container("alpha-cpu-probe-1", false),
            container(&format!("bravo-1-{STAMP}"), false),
            container(&format!("charlie-1-{STAMP}"), false),
        ];
    }
    let log = MemoryLog::default();
    let prepared = prepare_pools(
        &backend,
        pools(&scratch, &[("alpha", "ghp_pooltoken")]),
        &log,
        &Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(names(&prepared), ["alpha"]);
    let state = backend.state.borrow();
    let removed: Vec<&Vec<String>> = state
        .calls
        .iter()
        .filter_map(|call| match call {
            Call::RemoveContainers(names) => Some(names),
            _ => None,
        })
        .collect();
    assert_eq!(
        removed,
        [&vec![own.clone()], &vec![format!("alpha-12-{STAMP}")]]
    );
    let left: Vec<&str> = state.containers.iter().map(|c| c.name.as_str()).collect();
    assert!(
        left.contains(&"alpha-cpu-probe-1") && left.contains(&format!("bravo-1-{STAMP}").as_str())
    );
    assert_eq!(left.len(), 6);
    assert!(log.lines().contains(&format!(
        "WARN: Removing stale container '{own}' left over from an earlier run."
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn a_name_that_only_contains_the_pool_is_listed_but_not_removed() {
    let scratch = TempDir::new();
    let backend = FakeBackend::default();
    let lookalike = format!("my-alpha-1-{STAMP}");
    backend.state.borrow_mut().containers = vec![container(&lookalike, false)];
    // Docker's name filter matches anywhere in a name, and the fake does too.
    let listing = backend.list_pool_containers("alpha").await.unwrap();
    assert_eq!(listing.names, std::slice::from_ref(&lookalike));
    prepare_pools(
        &backend,
        pools(&scratch, &[("alpha", "ghp_pooltoken")]),
        &MemoryLog::default(),
        &Board::default(),
    )
    .await
    .unwrap();
    let state = backend.state.borrow();
    assert!(
        !state
            .calls
            .iter()
            .any(|call| matches!(call, Call::RemoveContainers(_)))
    );
    assert_eq!(state.containers.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_labelled_container_is_never_removed_and_refuses_the_pool() {
    let scratch = TempDir::new();
    let backend = FakeBackend::default();
    let evidence = format!("alpha-1-{STAMP}");
    let stale = format!("alpha-2-{STAMP}");
    backend.state.borrow_mut().containers =
        vec![container(&evidence, true), container(&stale, false)];
    let log = MemoryLog::default();
    let result = prepare_pools(
        &backend,
        pools(
            &scratch,
            &[("alpha", "ghp_pooltoken"), ("charlie", "ghp_pooltoken")],
        ),
        &log,
        &Board::default(),
    )
    .await;
    let prepared = result.unwrap();
    // alpha is refused; charlie still starts.
    assert_eq!(names(&prepared), ["charlie"]);
    let state = backend.state.borrow();
    assert!(
        state.containers.iter().any(|c| c.name == evidence),
        "the labelled container stays"
    );
    for call in &state.calls {
        if let Call::RemoveContainers(names) = call {
            assert!(!names.contains(&evidence));
        }
    }
    let lines = log.lines().join("\n");
    assert!(
        lines.contains("Diagnostic evidence retained after disabling; deletion refused"),
        "{lines}"
    );
    assert!(lines.contains(
        "repositories[0] (o/alpha) is not started and gets no slots: Marked diagnostic containers remain after disabling; entry startup refused."
    ));
    // alpha's volumes were not prepared.
    assert!(
        !state.calls.iter().any(
            |c| matches!(c, Call::PrepareVolumes { volumes, .. } if volumes == &["alpha-npm"])
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_listing_a_removal_or_a_volume_failure_is_handled_per_pool() {
    let scratch = TempDir::new();
    // a failed listing skips the pool.
    let backend = FakeBackend::default();
    backend.state.borrow_mut().list_error = Some(BackendError::new(
        "Diagnostic lifecycle identification unavailable; stale cleanup refused.",
    ));
    let log = MemoryLog::default();
    let result = prepare_pools(
        &backend,
        pools(&scratch, &[("alpha", "ghp_pooltoken")]),
        &log,
        &Board::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), NoPoolStartable);
    assert!(
        log.lines()[2].ends_with("stale cleanup refused."),
        "{:?}",
        log.lines()
    );

    // a failed removal is a warning only.
    let backend = FakeBackend::default();
    {
        let mut state = backend.state.borrow_mut();
        state.containers = vec![container(&format!("alpha-1-{STAMP}"), false)];
        state.remove_error = Some(BackendError::new("no such daemon"));
    }
    let log = MemoryLog::default();
    let prepared = prepare_pools(
        &backend,
        pools(&scratch, &[("alpha", "ghp_pooltoken")]),
        &log,
        &Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(names(&prepared), ["alpha"]);
    assert!(
        log.lines()
            .iter()
            .any(|l| l.starts_with("WARN: Failed to remove stale container "))
    );

    // a failed volume skips only its pool.
    let backend = FakeBackend::default();
    backend.state.borrow_mut().failing_volume = Some("alpha-npm".to_string());
    let log = MemoryLog::default();
    let prepared = prepare_pools(
        &backend,
        pools(
            &scratch,
            &[("alpha", "ghp_pooltoken"), ("charlie", "ghp_pooltoken")],
        ),
        &log,
        &Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(names(&prepared), ["charlie"]);
}

#[test]
fn a_plan_for_the_other_profile_is_not_a_container_plan() {
    let config = br#"{"budget":{"cpus":4,"memoryGb":16,"diskGb":100},"repositories":[
        {"owner":"o","repository":"r","name":"r","slots":1,"tokenPath":"/t","os":"linux","diskGb":10}]}"#;
    let plan = validate(config, Profile::Axmc, None).outcome.unwrap();
    assert!(pools_from_plan(&plan).is_err());
}
