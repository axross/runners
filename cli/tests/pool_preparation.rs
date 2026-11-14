//! pool preparation: token files, diagnostic containers left over, stale cleanup
//! and volumes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use marshal_cli::affinity::select_cpus;
use marshal_cli::profile::Profile;
use std::time::Duration;

use marshal_cli::runtime::backend::{Backend, BackendError, DiagnosticRecord, StopRequest};
use marshal_cli::runtime::board::Board;
use marshal_cli::runtime::fake::{Call, FakeBackend, FakeContainer, MemoryLog};
use marshal_cli::runtime::log::Log;
use marshal_cli::runtime::pool::{NoPoolStartable, PoolSpec, pools_from_plan};
use marshal_cli::validate::{Plan, validate};
use support::{TempDir, prepare_pools, write_token_file};

const STAMP: &str = "20240305060708009";

fn entry(name: &str, token: &str, extra: &str) -> String {
    format!(
        r#"{{"owner":"o","repository":"{name}","name":"{name}","slots":1,"tokenPath":"C:\\{token}",
        "volumes":[{{"suffix":"npm","mountPath":"/home/runner/.npm"}}]{extra}}}"#
    )
}

/// pools `alpha`, `bravo` and `charlie`, of which `bravo` has diagnostics on; token
/// files are `<name>.token` in `scratch`, written for the names in `with_token`.
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
        let token_path = scratch.path().join(format!("{}.token", pool.name));
        if let Some((_, content)) = with_token.iter().find(|(name, _)| *name == pool.name) {
            write_token_file(&token_path, content.as_bytes());
        }
        pool.token_path = Some(token_path);
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
async fn a_missing_or_blank_token_is_skipped_and_the_rest_run_a_diagnostics_pool_included() {
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
    assert_eq!(names(&prepared), ["alpha", "bravo"]);
    assert!(prepared[1].diagnostic.is_some() && prepared[0].diagnostic.is_none());
    let fields = board.fields();
    assert_eq!(fields.len(), 3, "{fields:?}");
    assert_eq!(fields[0], ("pool", "alpha: running".to_string()));
    assert_eq!(fields[1], ("pool", "bravo: running".to_string()));
    assert!(
        fields[2]
            .1
            .starts_with("charlie: skipped: repositories[2]: token file at "),
        "{fields:?}"
    );
    let warnings = log.lines();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("WARN: repositories[2] (o/charlie) is not started and gets no slots: repositories[2]: token file at ")
            && warnings[0].ends_with(" is empty."),
        "{warnings:?}"
    );
    log.info("forwarded: ghp_a and ghp_b");
    assert_eq!(
        log.lines().last().unwrap(),
        "INFO: forwarded: <masked> and <masked>"
    );

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
    assert!(log.lines().iter().all(|line| !line.contains("ghp_")));

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
            Call::ListPoolContainers("bravo".to_string()),
            Call::PrepareVolumes {
                image: "actions-runner:local".to_string(),
                volumes: vec!["bravo-npm".to_string()],
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
    std::fs::set_permissions(
        charlie.token_path.as_ref().unwrap(),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let log = MemoryLog::default();
    let prepared = prepare_pools(
        &FakeBackend::default(),
        all.clone(),
        &log,
        &Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(names(&prepared), ["alpha", "bravo"]);
    let warnings = log.lines();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("WARN: repositories[2] (o/charlie) is not started")
            && warnings[0].contains("token file is not private")
            && warnings[0].contains("has mode 0644"),
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
    // the evidence got one bounded stop
    assert!(state.calls.contains(&Call::StopContainers(StopRequest {
        names: vec![evidence.clone()],
        grace: Duration::from_secs(10),
        budget: Duration::from_secs(12),
    })));
    assert!(
        lines.contains("Recover before disabling diagnostics"),
        "the refusal names the manual recovery: {lines}"
    );
    assert_eq!(
        backend.retained.unmanaged(),
        [("alpha".to_string(), evidence.clone())]
    );
    assert!(
        !state.calls.iter().any(
            |c| matches!(c, Call::PrepareVolumes { volumes, .. } if volumes == &["alpha-npm"])
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_listing_a_removal_or_a_volume_failure_is_handled_per_pool() {
    let scratch = TempDir::new();
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

fn axmc_plan(image: &str, second_os: &str) -> Plan {
    let config = format!(
        r#"{{{image}"budget":{{"cpus":16,"memoryGb":32,"diskGb":300}},"repositories":[
        {{"owner":"o","repository":"a","name":"alpha","slots":2,"tokenPath":"/t",
          "os":"linux","cpus":4,"memoryGb":8,"diskGb":40}},
        {{"owner":"o","repository":"b","name":"bravo","slots":1,"tokenPath":"/u",
          "os":"{second_os}","cpus":3,"memoryGb":6,"diskGb":50}}]}}"#
    );
    validate(config.as_bytes(), Profile::Axmc, None)
        .outcome
        .unwrap()
}

#[test]
fn every_axmc_pool_gets_a_vm_pool_a_macos_one_included() {
    let plan = axmc_plan("", "macos");
    let names: Vec<_> = pools_from_plan(&plan)
        .unwrap()
        .into_iter()
        .map(|pool| pool.name)
        .collect();
    assert_eq!(names, ["alpha", "bravo"]);
}

#[test]
fn an_axmc_linux_pool_gives_each_slot_all_of_its_vms_cpus_and_its_memory_and_disk() {
    let plan = axmc_plan("", "linux");
    let pools = pools_from_plan(&plan).unwrap();
    let slots: Vec<_> = pools.iter().flat_map(PoolSpec::slot_specs).collect();
    let seen: Vec<_> = slots
        .iter()
        .map(|slot| {
            (
                slot.pool.as_str(),
                slot.number,
                slot.cpus.as_str(),
                slot.cpu_count,
                slot.cpu_offset,
                slot.memory_gb,
                slot.disk_gb,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("alpha", 1, "4", 4, 0, 8, Some(40)),
            ("alpha", 2, "4", 4, 0, 8, Some(40)),
            ("bravo", 1, "3", 3, 0, 6, Some(50)),
        ]
    );
    for slot in &slots {
        let all_vcpus = format!("0-{}", slot.cpu_count - 1);
        let cpuset = select_cpus(&all_vcpus, slot.cpu_count, slot.cpu_offset).unwrap();
        assert_eq!(cpuset, all_vcpus);
        let request = slot.host_request();
        assert_eq!(request.vcpus, slot.cpu_count);
        assert_eq!(request.memory_gb, slot.memory_gb);
        assert_eq!(request.disk_gb, slot.disk_gb);
    }
}

#[test]
fn an_axmc_pool_runs_the_default_image_unless_the_configuration_names_one() {
    let default = pools_from_plan(&axmc_plan("", "linux")).unwrap();
    assert!(
        default
            .iter()
            .all(|pool| pool.image == "actions-runner:local")
    );
    let named = pools_from_plan(&axmc_plan(r#""imageName":"runner:custom","#, "linux")).unwrap();
    assert!(named.iter().all(|pool| pool.image == "runner:custom"));
}

#[test]
fn an_axmc_plan_keeps_its_summary_without_an_image_name() {
    let plan = axmc_plan("", "linux");
    assert_eq!(plan.image_name, None);
    assert!(
        marshal_cli::summary::plan_summary(&plan)
            .iter()
            .all(|line| !line.contains("actions-runner:local")),
        "the default is applied by the start-side path, not by validation"
    );
}

fn bravo_record() -> DiagnosticRecord {
    DiagnosticRecord {
        pool: "bravo".to_string(),
        directory: r"C:\evidence".to_string(),
        raw_records: false,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_labelled_container_of_a_diagnostics_pool_is_exported_and_removed_at_startup() {
    let scratch = TempDir::new();
    let backend = FakeBackend::default();
    let evidence = format!("bravo-1-{STAMP}");
    backend.state.borrow_mut().containers = vec![container(&evidence, true)];
    let log = MemoryLog::default();
    let prepared = prepare_pools(
        &backend,
        pools(&scratch, &[("bravo", "ghp_pooltoken")]),
        &log,
        &Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(names(&prepared), ["bravo"]);
    let state = backend.state.borrow();
    let diagnostic: Vec<&Call> = state
        .calls
        .iter()
        .filter(|call| {
            matches!(
                call,
                Call::StopContainers(_) | Call::ExportDiagnostic(..) | Call::RemoveContainers(_)
            )
        })
        .collect();
    assert_eq!(diagnostic.len(), 3, "{diagnostic:?}");
    let Call::StopContainers(stop) = diagnostic[0] else {
        panic!("{diagnostic:?}");
    };
    assert_eq!(stop.names, std::slice::from_ref(&evidence));
    assert_eq!(stop.grace, Duration::from_secs(10));
    assert!(stop.budget <= Duration::from_secs(12), "{stop:?}");
    assert!(matches!(diagnostic[1], Call::ExportDiagnostic(name, _) if name == &evidence));
    assert_eq!(
        diagnostic[2],
        &Call::RemoveContainers(vec![evidence.clone()])
    );
    assert_eq!(state.bundles, [(evidence.clone(), bravo_record())]);
    assert!(state.containers.is_empty());
    assert!(!backend.retained.contains(&evidence));
    assert!(log.lines().contains(&format!(
        "INFO: Diagnostic container {evidence} that an earlier run left: exported, removed."
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn a_container_the_backend_retains_keeps_its_record_through_a_preparation() {
    let scratch = TempDir::new();
    let backend = FakeBackend::default();
    let evidence = format!("bravo-1-{STAMP}");
    backend.state.borrow_mut().containers = vec![container(&evidence, true)];
    let earlier = DiagnosticRecord {
        directory: r"D:\earlier".to_string(),
        raw_records: true,
        ..bravo_record()
    };
    backend.retained.adopt(&evidence, &earlier);
    let prepared = prepare_pools(
        &backend,
        pools(&scratch, &[("bravo", "ghp_pooltoken")]),
        &MemoryLog::default(),
        &Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(names(&prepared), ["bravo"]);
    let calls = backend.calls();
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            Call::StopContainers(_) | Call::ExportDiagnostic(..) | Call::RemoveContainers(_)
        )),
        "{calls:?}"
    );
    assert_eq!(backend.retained.record(&evidence), Some(earlier));
}
