//! the Tart backend run as a real process against the fake `tart`, `softnet` and
//! `docker`: availability, the golden VM check, cleanup of leftover VMs, the
//! prerequisites, the boot, the probes, the job container, and the release.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use marshal_cli::image::check::{GoldenCheck, ImageCheck, freshness_warnings};
use marshal_cli::image::contract::CONTRACT_VERSION;
use marshal_cli::macos::count::ConfiguredPool;
use marshal_cli::macos::gate::NetworkGate;
use marshal_cli::profile::{Os, Profile};
use marshal_cli::runtime::backend::{
    Backend, ContainerSpec, CpuRequest, DiagnosticRecord, HostPreparation, HostRequest, JobHost,
    StopRequest,
};
use marshal_cli::runtime::board::Board;
use marshal_cli::runtime::clock::Clock;
use marshal_cli::runtime::diagnostic::finalize_at_job_end;
use marshal_cli::runtime::docker_args::{probe_run_arguments, vm_job_run_arguments};
use marshal_cli::runtime::log::Log;
use marshal_cli::runtime::name::{cpu_probe_name, job_container_name};
use marshal_cli::runtime::pool::{PoolSpec, pools_from_plan};
use marshal_cli::runtime::secret::Secret;
use marshal_cli::runtime::tart::args::run_arguments;
use marshal_cli::runtime::tart::names::GoldenVmName;
use marshal_cli::runtime::tart::{GOLDEN_VM_VERSION, TartTimings, VmLauncher};
use marshal_cli::validate::validate;
use support::rig::{
    LAUNCHER, POOL, Rig, SLOT, TestClock, golden_name, host, job_vm, quick_timings, request,
};
use support::{BINARY, TempDir, write_token_file};

const IMAGE: &str = "actions-runner:local";
const JIT: &str = "jit-0123456789";
const STAMP: &str = "20240305060708009";

fn timings(change: impl FnOnce(&mut TartTimings)) -> TartTimings {
    let mut timings = quick_timings();
    change(&mut timings);
    timings
}

fn container_name() -> String {
    job_container_name(POOL, SLOT, TestClock.now())
}

fn spec(name: &str, diagnostic: Option<DiagnosticRecord>) -> ContainerSpec {
    ContainerSpec {
        name: name.to_string(),
        cpus: "4".to_string(),
        cpuset: "0-3".to_string(),
        memory_gb: 16,
        mounts: Vec::new(),
        image: IMAGE.to_string(),
        run_command: "/home/runner/run.sh".to_string(),
        diagnostic,
    }
}

fn probe_request() -> CpuRequest {
    CpuRequest {
        probe_name: cpu_probe_name(POOL, SLOT),
        image: IMAGE.to_string(),
        count: 4,
        offset: 0,
    }
}

async fn prepared(rig: &Rig) {
    assert_eq!(
        rig.backend.prepare_job_host(&request()).await,
        HostPreparation::Ready,
        "{:?}",
        rig.log.lines()
    );
}

fn held(preparation: HostPreparation) -> String {
    match preparation {
        HostPreparation::Held(reason) => reason,
        other => panic!("expected a held slot, got {other:?}"),
    }
}

fn failed(preparation: HostPreparation) -> String {
    match preparation {
        HostPreparation::Failed(error) => error.to_string(),
        other => panic!("expected a failed preparation, got {other:?}"),
    }
}

fn subcommands(calls: &[Vec<String>]) -> Vec<&str> {
    calls
        .iter()
        .map(|call| call.first().map_or("", String::as_str))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_tart_that_does_not_answer_is_waited_for_and_never_a_failure() {
    let rig = Rig::new();
    rig.fake
        .script("fail-list", "Error: the tart server is starting");
    let flag = rig.fake.dir.path().join("fail-list");
    let heal = async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        fs::remove_file(flag).unwrap();
    };
    tokio::join!(rig.backend.wait_until_available(), heal);
    assert!(
        !rig.log_lines("WARN: Tart is not responding yet").is_empty(),
        "{:?}",
        rig.log.lines()
    );
    assert_eq!(
        rig.backend.engine_poll_interval(),
        Duration::from_millis(50)
    );
    assert!(
        rig.tart_calls()
            .iter()
            .all(|call| call == &["list", "--format", "json"]),
        "availability is a listing"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_golden_vm_check_reads_the_listing_and_nothing_boots() {
    let rig = Rig::new();
    let check = rig.backend.check_image(IMAGE).await;
    assert!(
        matches!(&check, ImageCheck::Golden(GoldenCheck::Matching { name, .. }) if *name == golden_name()),
        "{check:?}"
    );
    assert_eq!(check.refusal(IMAGE), None);
    assert_eq!(rig.tart_calls(), [["list", "--format", "json"]]);

    rig.fake.seed(&[]);
    let missing = rig.backend.check_image(IMAGE).await;
    assert_eq!(
        missing.refusal(IMAGE).unwrap(),
        format!(
            "the golden VM is missing; this CLI needs one with golden VM version {GOLDEN_VM_VERSION} and image contract {CONTRACT_VERSION}"
        )
    );

    let mut other = GoldenVmName::parse(&golden_name()).unwrap();
    other.golden_version += 1;
    let other = other.to_string();
    rig.fake.seed(&[(&other, "stopped", "local")]);
    let mismatched = rig.backend.check_image(IMAGE).await;
    assert_eq!(
        mismatched.refusal(IMAGE).unwrap(),
        format!(
            "the golden VM {other} has golden VM version {} and image contract {CONTRACT_VERSION}, \
             but this CLI needs golden VM version {GOLDEN_VM_VERSION} and image contract {CONTRACT_VERSION}",
            GOLDEN_VM_VERSION + 1
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_old_golden_vm_gives_the_freshness_warning_and_a_listing_that_hangs_is_unavailable() {
    let rig = Rig::with(
        VmLauncher::Direct,
        timings(|timings| timings.image_check = Duration::from_secs(1)),
    );
    let check = rig.backend.check_image(IMAGE).await;
    let eight_days = TestClock.now() + Duration::from_secs(8 * 86_400);
    assert!(
        freshness_warnings(&check, eight_days).contains(
            &"the golden VM is 8 days old and should be rebuilt with `marshal build`".to_string()
        ),
        "{:?}",
        freshness_warnings(&check, eight_days)
    );

    rig.fake.script("hang-list", "");
    let started = Instant::now();
    let unanswered = rig.backend.check_image(IMAGE).await;
    assert_eq!(
        unanswered,
        ImageCheck::Unavailable("Tart did not answer the golden VM check within 1 s".to_string())
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

fn axmc_pools(scratch: &TempDir) -> Vec<PoolSpec> {
    let config = r#"{"budget":{"cpus":16,"memoryGb":32,"diskGb":300},"repositories":[
        {"owner":"o","repository":"a","name":"alpha","slots":2,"tokenPath":"/t",
         "os":"linux","cpus":4,"memoryGb":8,"diskGb":40},
        {"owner":"o","repository":"b","name":"bravo","slots":1,"tokenPath":"/u",
         "os":"linux","cpus":3,"memoryGb":6,"diskGb":30,
         "diagnostics":true,"diagnosticDirectory":"/evidence"}]}"#;
    let plan = validate(config.as_bytes(), Profile::Axmc, None)
        .outcome
        .unwrap();
    let mut pools = pools_from_plan(&plan).unwrap();
    for pool in &mut pools {
        let token_path = scratch.path().join(format!("{}.token", pool.name));
        write_token_file(&token_path, b"ghp_token\n");
        pool.token_path = Some(token_path);
    }
    pools
}

#[tokio::test(flavor = "current_thread")]
async fn start_deletes_the_leftover_job_vms_of_configured_pools_and_nothing_else() {
    let rig = Rig::new();
    let golden = golden_name();
    let build = format!("marshal-build-linux-{STAMP}");
    let leftovers = [
        format!("marshal-job-alpha-1-{STAMP}"),
        format!("marshal-job-alpha-2-{STAMP}"),
        format!("marshal-job-bravo-1-{STAMP}"),
    ];
    let kept = [
        golden.clone(),
        format!("marshal-job-charlie-1-{STAMP}"),
        build,
        "marshal-job-alpha-1-nonsense".to_string(),
        format!("marshal-job-alpha-x-{STAMP}"),
        "someone-elses-vm".to_string(),
    ];
    let mut vms: Vec<(&str, &str, &str)> = kept
        .iter()
        .map(|name| (name.as_str(), "stopped", "local"))
        .collect();
    vms.push((&leftovers[0], "stopped", "local"));
    vms.push((&leftovers[1], "running", "local"));
    vms.push((&leftovers[2], "stopped", "local"));
    let cached = format!("marshal-job-alpha-9-{STAMP}");
    vms.push((&cached, "stopped", "OCI"));
    rig.fake.seed(&vms);

    let scratch = TempDir::new();
    let board = Board::default();
    let prepared = support::prepare_pools(&rig.backend, axmc_pools(&scratch), &*rig.log, &board)
        .await
        .unwrap();
    assert_eq!(prepared.len(), 2);

    let mut left = rig.fake.vm_names();
    left.sort();
    let mut expected: Vec<String> = kept.to_vec();
    expected.push(format!("marshal-job-alpha-9-{STAMP}"));
    expected.sort();
    assert_eq!(left, expected);

    let calls = rig.tart_calls();
    assert!(
        subcommands(&calls)
            .iter()
            .all(|call| ["list", "stop", "delete"].contains(call)),
        "{calls:?}"
    );
    let lines = rig.log.lines();
    for name in &leftovers {
        assert!(
            lines.contains(&format!(
                "WARN: Deleting job VM '{name}' left over from an earlier run."
            )),
            "{lines:?}"
        );
    }
    assert!(
        lines.contains(&format!(
            "WARN: Diagnostic gap in pool bravo, job VM {}: private export unavailable, incomplete, unsafe or over budget; prior evidence retained.",
            leftovers[2]
        )),
        "{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("Diagnostic gap in pool alpha")),
        "{lines:?}"
    );
}

/// a leftover job VM that cannot be deleted skips its pool as a labelled container
/// that remains does on the container backend, and the other pools start.
#[tokio::test(flavor = "current_thread")]
async fn a_leftover_job_vm_that_cannot_be_deleted_skips_its_pool_and_the_others_start() {
    let rig = Rig::new();
    let leftover = format!("marshal-job-alpha-1-{STAMP}");
    rig.fake.seed(&[
        (golden_name().as_str(), "stopped", "local"),
        (&leftover, "stopped", "local"),
    ]);
    rig.fake.script("fail-delete", "");
    let scratch = TempDir::new();
    let board = Board::default();

    let prepared = support::prepare_pools(&rig.backend, axmc_pools(&scratch), &*rig.log, &board)
        .await
        .unwrap();

    assert_eq!(
        prepared
            .iter()
            .map(|pool| pool.name.as_str())
            .collect::<Vec<_>>(),
        ["bravo"]
    );
    let reason = format!(
        "Job VMs that an earlier run left could not be deleted ({leftover}); entry startup \
         refused. Delete each with tart delete, then start again."
    );
    assert!(
        board
            .fields()
            .contains(&("pool", format!("alpha: skipped: {reason}"))),
        "{:?}",
        board.fields()
    );
    let lines = rig.log.lines();
    assert!(
        lines.iter().any(|line| line.starts_with("WARN: ")
            && line.contains("is not started and gets no slots")
            && line.ends_with(&reason)),
        "{lines:?}"
    );
    assert!(rig.fake.vm_names().contains(&leftover));
}

/// the leftover VM of a diagnostics-enabled pool that cannot be deleted still reports
/// the gap that discarding its evidence leaves, and the pool is skipped.
#[tokio::test(flavor = "current_thread")]
async fn a_leftover_diagnostic_job_vm_that_cannot_be_deleted_reports_its_gap_and_skips_its_pool() {
    let rig = Rig::new();
    let leftover = format!("marshal-job-bravo-1-{STAMP}");
    rig.fake.seed(&[
        (golden_name().as_str(), "stopped", "local"),
        (&leftover, "stopped", "local"),
    ]);
    rig.fake.script("fail-delete", "");
    let scratch = TempDir::new();
    let board = Board::default();

    let prepared = support::prepare_pools(&rig.backend, axmc_pools(&scratch), &*rig.log, &board)
        .await
        .unwrap();

    assert_eq!(
        prepared
            .iter()
            .map(|pool| pool.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha"]
    );
    let reason = format!(
        "Job VMs that an earlier run left could not be deleted ({leftover}); entry startup \
         refused. Delete each with tart delete, then start again."
    );
    assert!(
        board
            .fields()
            .contains(&("pool", format!("bravo: skipped: {reason}"))),
        "{:?}",
        board.fields()
    );
    let lines = rig.log.lines();
    assert!(
        lines.contains(&format!(
            "WARN: Diagnostic gap in pool bravo, job VM {leftover}: private export unavailable, incomplete, unsafe or over budget; prior evidence retained."
        )),
        "{lines:?}"
    );
    assert!(rig.fake.vm_names().contains(&leftover));
}

#[tokio::test(flavor = "current_thread")]
async fn a_slot_cycle_clones_the_newest_golden_vm_sets_it_boots_it_and_waits_for_the_agent() {
    let rig = Rig::new();
    let mut older = GoldenVmName::parse(&golden_name()).unwrap();
    older.built_seconds -= 86_400;
    let older = older.to_string();
    let newer = golden_name();
    rig.fake
        .seed(&[(&older, "stopped", "local"), (&newer, "stopped", "local")]);

    prepared(&rig).await;

    let vm = job_vm();
    assert_eq!(vm, format!("marshal-job-{POOL}-{SLOT}-{STAMP}"));
    let calls = rig.tart_calls();
    assert_eq!(calls[0], ["--version"]);
    assert_eq!(calls[1], ["list", "--format", "json"]);
    assert_eq!(calls[2], ["clone", &newer, &vm]);
    assert_eq!(
        calls[3],
        [
            "set",
            &vm,
            "--cpu",
            "4",
            "--memory",
            "16384",
            "--disk-size",
            "60"
        ]
    );
    assert!(calls.contains(&run_arguments(&vm)));
    assert!(calls.iter().any(|call| call == &["exec", &vm, "true"]));
    assert_eq!(rig.backend.host_vm(&host()), Some(vm.clone()));
    assert_eq!(rig.backend.prepared_hosts(), [host()]);
    assert!(
        rig.log
            .lines()
            .contains(&"INFO: Using Tart 2.41.1 and Softnet 0.25.0-0f1e2d3.".to_string()),
        "{:?}",
        rig.log.lines()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn every_tart_call_has_no_auto_prune_the_hosts_path_and_no_terminal_and_no_jit() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    let calls = rig.fake.invocations();
    assert!(calls.len() >= 7);
    let path = rig.fake.client.config().path.clone().unwrap();
    for call in calls {
        assert_eq!(call.no_auto_prune.as_deref(), Some("1"), "{call:?}");
        assert_eq!(call.path.as_deref(), path.to_str(), "{call:?}");
        assert!(!call.jit_present, "{call:?}");
        assert!(!call.stdout_terminal && !call.stdin_terminal, "{call:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_vm_that_is_not_up_yet_is_asked_again_until_its_agent_answers() {
    let rig = Rig::new();
    rig.fake.script("exec_exit", "1");
    let open = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        fs::remove_file(rig.fake.dir.path().join("exec_exit")).unwrap();
    };
    let wanted = request();
    let (preparation, ()) = tokio::join!(rig.backend.prepare_job_host(&wanted), open);
    assert_eq!(preparation, HostPreparation::Ready);
    assert!(rig.tart_calls_of("exec").len() >= 2);
}

#[tokio::test(flavor = "current_thread")]
async fn releasing_stops_the_vm_deletes_it_and_a_second_release_does_nothing() {
    let rig = Rig::new();
    prepared(&rig).await;
    let vm = job_vm();
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(rig.backend.prepared_hosts().is_empty());
    let calls = rig.tart_calls();
    assert!(calls.contains(&vec![
        "stop".to_string(),
        vm.clone(),
        "--timeout".to_string(),
        "4".to_string()
    ]));
    assert!(calls.contains(&vec!["delete".to_string(), vm]));
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(rig.tart_calls(), calls, "a released host has nothing to do");
}

#[tokio::test(flavor = "current_thread")]
async fn two_releases_at_once_delete_the_vm_once() {
    let rig = Rig::new();
    prepared(&rig).await;
    let budget = Duration::from_secs(10);
    let job = host();
    let (first, second) = tokio::join!(
        rig.backend.release_job_host(&job, budget),
        rig.backend.release_job_host(&job, budget)
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(rig.tart_calls_of("delete").len(), 1);
    assert_eq!(rig.tart_calls_of("stop").len(), 1);
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_vm_that_ignores_the_graceful_stop_is_stopped_forcibly_and_deleted_in_budget() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.fake.script("stop_ignored", "");
    let started = Instant::now();
    rig.backend
        .release_job_host(&host(), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "in budget");
    let vm = job_vm();
    let stops: Vec<Vec<String>> = rig
        .tart_calls_of("stop")
        .into_iter()
        .map(|call| call.arguments)
        .collect();
    assert_eq!(
        stops,
        [
            ["stop", &vm, "--timeout", "1"],
            ["stop", &vm, "--timeout", "0"]
        ]
    );
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_release_that_fails_keeps_the_host_and_the_next_preparation_deletes_the_old_vm_first() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.fake.script("hang-delete", "");
    let error = rig
        .backend
        .release_job_host(&host(), Duration::from_secs(3))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains(&job_vm()), "{error}");
    assert_eq!(rig.backend.prepared_hosts(), [host()]);
    assert!(rig.fake.vm_names().contains(&job_vm()));

    fs::remove_file(rig.fake.dir.path().join("hang-delete")).unwrap();
    // the clock names the next VM like the old one, so it can only be cloned once
    // the old one is gone
    prepared(&rig).await;
    assert_eq!(rig.fake.vm_names().len(), 2);
    assert_eq!(rig.tart_calls_of("clone").len(), 2);
}

/// ends the `tart run` of the prepared VM with SIGKILL, as a killed shim leaves it:
/// the process is gone and the VM still runs.
async fn kill_the_holder(rig: &Rig) {
    let run = rig.tart_calls_of("run");
    let status = std::process::Command::new("kill")
        .args(["-9", &run[0].pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let number = rig
        .fake
        .invocations()
        .iter()
        .position(|call| call.arguments.first().map(String::as_str) == Some("run"))
        .unwrap();
    assert!(rig.fake.ended_within(number, Duration::from_secs(5)));
    // the backend sees the holder end through its waiter task
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// runs `act` once the container `name` exists in its VM and drops the run when `act`
/// ends, so an `act` that releases the VM ends the job with it.
async fn during_run<A: std::future::Future>(rig: &Rig, name: &str, act: A) -> A::Output {
    let spec = spec(name, None);
    let jit = Secret::new(JIT.to_string());
    let started = Instant::now();
    let run = rig.backend.run_container(&spec, &jit);
    let act = async {
        while !rig.containers().contains(&name.to_string()) {
            assert!(started.elapsed() < Duration::from_secs(10), "it never ran");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        act.await
    };
    tokio::select! {
        _ = run => panic!("the container ended on its own"),
        output = act => output,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_vm_whose_holder_was_killed_is_stopped_forcibly_once_and_then_deleted() {
    let rig = Rig::new();
    prepared(&rig).await;
    let vm = job_vm();
    kill_the_holder(&rig).await;
    assert!(
        rig.fake
            .vms()
            .iter()
            .any(|fields| fields[0] == vm && fields[1] == "running"),
        "the VM outlived its holder"
    );

    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();

    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(rig.backend.prepared_hosts().is_empty());
    let calls = rig.tart_calls();
    let teardown: Vec<&str> = subcommands(&calls)
        .into_iter()
        .filter(|subcommand| matches!(*subcommand, "stop" | "delete"))
        .collect();
    assert_eq!(
        teardown,
        ["delete", "stop", "delete"],
        "no graceful stop, one forced stop between the two deletions"
    );
    let stops = rig.tart_calls_of("stop");
    assert_eq!(stops[0].arguments, ["stop", &vm, "--timeout", "0"]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_failing_container_removal_during_a_held_release_is_not_an_error() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("run_mode", "hold");
    rig.docker_script("rm_fails", "");
    rig.fake.script("hang-delete", "");
    let name = container_name();
    let both = async {
        let job = host();
        let release = rig.backend.release_job_host(&job, Duration::from_secs(3));
        let removal = async {
            let started = Instant::now();
            while rig.tart_calls_of("delete").is_empty() {
                assert!(started.elapsed() < Duration::from_secs(5), "no deletion");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            rig.backend
                .remove_containers(std::slice::from_ref(&name), Duration::from_secs(2))
                .await
        };
        tokio::join!(release, removal)
    };
    let (release, removal) = during_run(&rig, &name, both).await;
    assert!(
        release.is_err(),
        "the deletion hangs, so the release ends at its budget"
    );
    assert_eq!(removal, Ok(()), "the VM takes the container with it");
}

#[tokio::test(flavor = "current_thread")]
async fn a_failing_container_removal_with_no_release_is_an_error() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("run_mode", "hold");
    rig.docker_script("rm_fails", "");
    let name = container_name();
    let removal_then_release = async {
        let removal = rig
            .backend
            .remove_containers(std::slice::from_ref(&name), Duration::from_secs(2))
            .await;
        rig.backend
            .release_job_host(&host(), Duration::from_secs(10))
            .await
            .unwrap();
        removal
    };
    let removal = during_run(&rig, &name, removal_then_release).await;
    assert!(removal.is_err(), "nothing was releasing the VM");
}

#[tokio::test(flavor = "current_thread")]
async fn a_preparation_that_is_dropped_is_still_counted_and_released() {
    let rig = Rig::new();
    rig.fake.script("hang-clone", "");
    let outcome = tokio::time::timeout(
        Duration::from_millis(600),
        rig.backend.prepare_job_host(&request()),
    )
    .await;
    assert!(outcome.is_err(), "the clone hangs");
    assert_eq!(rig.backend.prepared_hosts(), [host()]);
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert!(rig.backend.prepared_hosts().is_empty());
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_failure_at_each_step_from_the_clone_on_fails_the_preparation_and_the_release_deletes_the_vm()
 {
    for step in ["clone", "set"] {
        let rig = Rig::new();
        rig.fake.script(&format!("fail-{step}"), "Error: scripted");
        let error = failed(rig.backend.prepare_job_host(&request()).await);
        assert!(error.contains("scripted"), "{step}: {error}");
        rig.backend
            .release_job_host(&host(), Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(rig.fake.vm_names(), [golden_name()], "{step}");
        assert!(rig.backend.prepared_hosts().is_empty());
        assert!(
            rig.tart_calls_of("run").is_empty(),
            "{step}: nothing boots after a failed {step}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_tart_run_is_a_boot_failure_with_tarts_error_and_the_signed_in_section() {
    let rig = Rig::new();
    rig.fake.script("run_mode", "exit");
    rig.fake
        .script("run_error", "Error: the login keychain is locked");
    let error = failed(rig.backend.prepare_job_host(&request()).await);
    let vm = job_vm();
    assert_eq!(
        error,
        format!(
            "boot failure: tart run for the job VM {vm} ended with exit code 1 (Error: the login keychain is locked); \
             a locked login keychain is the documented cause, see the \"The Signed-In Session\" section of the macOS Runner Host document"
        )
    );
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_guest_agent_that_does_not_answer_by_the_boot_deadline_is_a_boot_failure() {
    let rig = Rig::with(
        VmLauncher::Direct,
        timings(|timings| timings.boot_deadline = Duration::from_millis(800)),
    );
    rig.fake
        .script("fail-exec", "Error: the guest agent is not up");
    let started = Instant::now();
    let error = failed(rig.backend.prepare_job_host(&request()).await);
    assert!(started.elapsed() < Duration::from_secs(5));
    let vm = job_vm();
    assert_eq!(
        error,
        format!(
            "boot failure: the guest agent of the job VM {vm} did not answer by the boot deadline (Error: the guest agent is not up); \
             a locked login keychain is the documented cause, see the \"The Signed-In Session\" section of the macOS Runner Host document"
        )
    );
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
}

#[tokio::test(flavor = "current_thread")]
async fn the_boot_deadline_is_a_hundred_and_twenty_seconds() {
    assert_eq!(
        TartTimings::default().boot_deadline,
        Duration::from_secs(120)
    );
    assert_eq!(
        marshal_cli::runtime::tart::TART_POLL_INTERVAL,
        Duration::from_secs(5)
    );
    assert_eq!(Rig::new().backend.release_budget(), Duration::from_secs(10));
}

#[tokio::test(flavor = "current_thread")]
async fn a_tart_version_that_does_not_run_holds_the_slot_before_anything_boots() {
    let rig = Rig::new();
    rig.fake.script("version_output", "2.40.0\n");
    let reason = held(rig.backend.prepare_job_host(&request()).await);
    assert_eq!(
        reason,
        "Tart 2.40.0 is not the tested version 2.41.1 or a newer patch of 2.41. Install Tart 2.41.1 as the \"Installing Tart and Softnet\" section of the macOS Runner Host document describes."
    );
    assert_eq!(rig.tart_calls(), [["--version"]]);
    assert_eq!(
        rig.backend.prepared_hosts(),
        [host()],
        "counted until released"
    );
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert!(rig.backend.prepared_hosts().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_softnet_or_a_softnet_that_is_not_set_up_holds_the_slot() {
    let rig = Rig::without_softnet();
    let reason = held(rig.backend.prepare_job_host(&request()).await);
    assert_eq!(
        reason,
        "Softnet was not found on PATH. Install Softnet 0.25.0 as the \"Installing Tart and Softnet\" section of the macOS Runner Host document describes."
    );
    assert_eq!(rig.tart_calls(), [["--version"]]);

    for (owner, mode, text) in [
        (1000, 0o4755, "is owned by user 1000, not root"),
        (0, 0o0755, "does not have the SUID bit set"),
        (0, 0o4775, "can be written by its group"),
        (0, 0o4757, "can be written by others"),
    ] {
        let rig = Rig::with_softnet_file(owner, mode);
        let reason = held(rig.backend.prepare_job_host(&request()).await);
        assert!(
            reason.starts_with("Softnet at ")
                && reason.contains(text)
                && reason.ends_with(
                    "Set it up as the \"Setting Up Softnet\" section of the macOS Runner Host document describes."
                ),
            "{reason}"
        );
        assert_eq!(rig.tart_calls(), [["--version"]], "{text}");
    }
}

/// the prerequisites are checked after the network gate opens, so a Softnet that went
/// away while the gate was closed holds the slot instead of booting on a stale check.
#[tokio::test(flavor = "current_thread")]
async fn a_softnet_that_went_missing_while_the_gate_was_closed_holds_the_slot_once_it_opens() {
    let gate = NetworkGate::new();
    gate.close();
    let rig = Rig::with_gate(gate.clone());
    let softnet = rig.softnet.root.path().join("bin").join("softnet");
    let job = request();
    let preparing = rig.backend.prepare_job_host(&job);
    let opening = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            rig.tart_calls().is_empty(),
            "nothing runs while it is closed"
        );
        fs::remove_file(&softnet).unwrap();
        gate.open();
    };

    let (preparation, ()) = tokio::join!(preparing, opening);

    let reason = held(preparation);
    assert!(
        reason.starts_with("Softnet was not found on PATH"),
        "{reason}"
    );
    assert!(rig.tart_calls_of("clone").is_empty());
    assert!(rig.tart_calls_of("run").is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_softnet_that_does_not_answer_or_has_another_version_holds_the_slot() {
    let rig = Rig::new();
    rig.softnet
        .script("bin", "version_output", "softnet 0.24.9-0f1e2d3\n");
    let reason = held(rig.backend.prepare_job_host(&request()).await);
    assert!(
        reason.starts_with(
            "Softnet 0.24.9 is not the tested version 0.25.0 or a newer patch of 0.25."
        ),
        "{reason}"
    );
    rig.softnet.script("bin", "version_output", "0.25.0\n");
    let reason = held(rig.backend.prepare_job_host(&request()).await);
    assert!(
        reason.contains("which is not \"softnet \" and exactly three"),
        "{reason}"
    );
    assert!(rig.tart_calls_of("clone").is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn the_two_version_reads_of_the_check_before_a_run_go_together_within_the_bound_given() {
    let rig = Rig::new();
    rig.fake.script("hang-version", "");
    rig.softnet.script("bin", "hang-version", "");
    let started = Instant::now();
    let reason = rig
        .backend
        .check_prerequisites(Duration::from_secs(1))
        .await
        .unwrap_err();
    let waited = started.elapsed();
    assert!(
        reason.starts_with("\"softnet --version\" did not answer within 1 s."),
        "{reason}"
    );
    assert!(
        waited >= Duration::from_secs(1) && waited < Duration::from_millis(1700),
        "a Tart and a Softnet that do not answer took {waited:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_newer_patch_runs_with_a_warning_logged_once_and_kept_for_status() {
    let rig = Rig::new();
    rig.fake.script("version_output", "2.41.3\n");
    rig.softnet
        .script("bin", "version_output", "softnet 0.25.2-0f1e2d3\n");
    for _ in 0..2 {
        prepared(&rig).await;
        rig.backend
            .release_job_host(&host(), Duration::from_secs(10))
            .await
            .unwrap();
    }
    let warnings = rig.backend.prerequisite_warnings();
    assert_eq!(
        warnings,
        [
            "Tart 2.41.3 is a newer patch of the tested version 2.41.1; it runs, but only 2.41.1 was tested.",
            "Softnet 0.25.2 is a newer patch of the tested version 0.25.0; it runs, but only 0.25.0 was tested."
        ]
    );
    let logged = rig.log_lines("WARN: ");
    assert_eq!(logged.len(), 2, "each warning is logged once: {logged:?}");
    assert_eq!(
        rig.log_lines("INFO: Using ").len(),
        1,
        "the versions are logged when they change"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_tart_that_is_missing_makes_the_slot_wait_and_not_fail() {
    let rig = Rig::with_tart_program("/nonexistent/tart");
    let outcome = tokio::time::timeout(
        Duration::from_millis(500),
        rig.backend.prepare_job_host(&request()),
    )
    .await;
    assert!(outcome.is_err(), "the slot keeps waiting");
    let waiting = rig.log_lines("WARN: Tart is not available (Tart could not be started");
    assert!(waiting.len() >= 2, "{:?}", rig.log.lines());
    assert!(waiting[0].ends_with("- waiting."), "{waiting:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn the_probes_ask_the_vms_docker_engine_then_the_cpu_placement_through_the_agent() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("probe_cpus", "0-3");
    let vm = job_vm();
    let cpus = rig.backend.select_cpuset(&probe_request()).await.unwrap();
    assert_eq!(cpus, "0-3");
    let calls = rig.tart_calls();
    let info = calls
        .iter()
        .position(|call| call == &["exec", &vm, "docker", "info"])
        .expect("the engine is asked first");
    let mut probe = vec!["exec".to_string(), vm, "docker".to_string()];
    probe.extend(probe_run_arguments(&cpu_probe_name(POOL, SLOT), IMAGE));
    let placement = calls
        .iter()
        .position(|call| *call == probe)
        .expect("the CPU placement is read through the agent");
    assert!(info < placement);
}

#[tokio::test(flavor = "current_thread")]
async fn a_probe_that_fails_or_answers_badly_gives_no_cpu_set() {
    for (mode, expected) in [
        ("fail", "CPU discovery: allowed CPU probe failed."),
        ("flood", "CPU discovery: invalid allowed CPU list."),
    ] {
        let rig = Rig::new();
        prepared(&rig).await;
        rig.docker_script("probe_mode", mode);
        let error = rig
            .backend
            .select_cpuset(&probe_request())
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(error, expected, "{mode}");
    }
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("probe_mode", "garbage");
    assert!(rig.backend.select_cpuset(&probe_request()).await.is_err());

    // four vCPUs cannot be taken from a VM that allows two
    rig.docker_script("probe_mode", "");
    rig.docker_script("probe_cpus", "0-1");
    assert!(rig.backend.select_cpuset(&probe_request()).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn an_engine_that_is_slow_is_waited_for_and_one_that_never_answers_is_an_error() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("info_failures", "3");
    assert_eq!(
        rig.backend.select_cpuset(&probe_request()).await.unwrap(),
        "0-3"
    );
    assert!(
        rig.docker_calls()
            .iter()
            .filter(|call| call.arguments == ["info"])
            .count()
            >= 4
    );

    let rig = Rig::with(
        VmLauncher::Direct,
        timings(|timings| timings.engine_wait = Duration::from_millis(300)),
    );
    prepared(&rig).await;
    rig.docker_script("info_failures", "100000");
    let error = rig
        .backend
        .select_cpuset(&probe_request())
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "CPU discovery: the Docker Engine in the job VM did not answer within 0 s."
    );
    assert!(
        rig.log_lines("WARN: ")
            .iter()
            .any(|line| line.contains(&cpu_probe_name(POOL, SLOT))),
        "the reason is logged"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_release_that_begins_during_the_engine_wait_ends_the_wait_at_once() {
    let rig = Rig::with(
        VmLauncher::Direct,
        timings(|timings| timings.engine_wait = Duration::from_secs(30)),
    );
    prepared(&rig).await;
    rig.docker_script("info_failures", "100000");
    let started = Instant::now();
    let release = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        rig.backend
            .release_job_host(&host(), Duration::from_secs(10))
            .await
            .unwrap();
    };
    let request = probe_request();
    let (selected, ()) = tokio::join!(rig.backend.select_cpuset(&request), release);
    assert_eq!(
        selected.unwrap_err().to_string(),
        "CPU discovery: the job VM was released.",
        "{:?}",
        rig.log.lines()
    );
    let waited = started.elapsed();
    assert!(
        waited < Duration::from_secs(10),
        "the wait ran on after the release: {waited:?}"
    );
    assert!(
        rig.log_lines("WARN: ")
            .iter()
            .any(|line| line.contains("CPU discovery: the job VM was released.")),
        "the reason is logged"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_cpu_set_needs_a_prepared_vm() {
    let rig = Rig::new();
    assert!(rig.backend.select_cpuset(&probe_request()).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn the_runner_container_mounts_the_vms_own_docker_socket_and_nothing_else_of_a_host() {
    let rig = Rig::new();
    prepared(&rig).await;
    let vm = job_vm();
    let spec = spec(&container_name(), None);
    let jit = Secret::new(JIT.to_string());
    rig.log.mask(&jit);
    rig.docker_script("run_prints_jit", "");

    assert_eq!(rig.backend.run_container(&spec, &jit).await.unwrap(), 0);

    let launch = rig
        .fake
        .invocations()
        .into_iter()
        .find(|call| call.arguments.contains(&LAUNCHER.to_string()))
        .expect("the launcher is started through tart exec");
    assert_eq!(launch.arguments[..3], ["exec", "-i", vm.as_str()]);
    let mounts: Vec<&String> = launch
        .arguments
        .iter()
        .filter(|argument| argument.starts_with("type="))
        .collect();
    assert_eq!(
        mounts,
        ["type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock"],
        "the VM's socket is the one mount, and it is inside the VM"
    );
    assert!(
        !launch
            .arguments
            .iter()
            .any(|argument| argument == "--privileged")
    );
    for call in rig.fake.invocations() {
        if call.arguments != launch.arguments {
            assert!(
                call.arguments
                    .iter()
                    .all(|argument| !argument.contains("docker.sock")),
                "only the job container's run carries the socket: {:?}",
                call.arguments
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_jit_configuration_reaches_the_guest_only_on_standard_input() {
    let rig = Rig::new();
    prepared(&rig).await;
    let vm = job_vm();
    let name = container_name();
    let spec = spec(&name, None);
    let jit = Secret::new(JIT.to_string());
    rig.log.mask(&jit);
    rig.docker_script("run_prints_jit", "");

    let exit = rig.backend.run_container(&spec, &jit).await.unwrap();
    assert_eq!(exit, 0);

    let launch = rig
        .fake
        .invocations()
        .into_iter()
        .find(|call| call.arguments.contains(&LAUNCHER.to_string()))
        .expect("the launcher is started through tart exec");
    let mut expected = vec![
        "exec".to_string(),
        "-i".to_string(),
        vm,
        LAUNCHER.to_string(),
    ];
    expected.extend(vm_job_run_arguments(&spec));
    assert_eq!(launch.arguments, expected);
    assert_eq!(launch.stdin.as_deref(), Some(JIT.as_bytes()));
    assert!(
        rig.fake
            .invocations()
            .iter()
            .all(|call| !call.arguments.iter().any(|argument| argument.contains(JIT))),
        "no tart argument holds it"
    );
    assert!(
        rig.fake.invocations().iter().all(|call| !call.jit_present),
        "no tart process has it in its environment"
    );
    let engine = rig
        .docker_calls()
        .into_iter()
        .find(|call| call.arguments.first().map(String::as_str) == Some("run"))
        .unwrap();
    assert_eq!(
        engine.jit.as_deref(),
        Some(JIT),
        "the launcher hands it to docker"
    );
    assert!(
        !engine
            .arguments
            .iter()
            .any(|argument| argument.contains(JIT))
    );

    let lines = rig.log.lines();
    assert!(
        lines.contains(&"INFO: fake runner started".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"INFO: jit on stdout: <masked>".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"INFO: jit on stderr: <masked>".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|line| !line.contains(JIT)),
        "no log line holds it: {lines:?}"
    );
    assert!(rig.backend.running_containers().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn the_runner_container_runs_with_all_the_vms_cpus_its_memory_and_no_pull() {
    let rig = Rig::new();
    prepared(&rig).await;
    let spec = spec(&container_name(), None);
    rig.docker_script("run_exit", "3");
    let exit = rig
        .backend
        .run_container(&spec, &Secret::new(JIT.to_string()))
        .await
        .unwrap();
    assert_eq!(exit, 3, "the container's exit code is the job's");
    let run = rig
        .docker_calls()
        .into_iter()
        .find(|call| call.arguments.first().map(String::as_str) == Some("run"))
        .unwrap()
        .arguments;
    let value_after = |flag: &str| {
        let at = run.iter().position(|argument| argument == flag).unwrap();
        run[at + 1].clone()
    };
    assert_eq!(value_after("--pull"), "never");
    assert_eq!(value_after("--cpus"), "4");
    assert_eq!(value_after("--cpuset-cpus"), "0-3");
    assert_eq!(value_after("--memory"), "16g");
    assert_eq!(value_after("--memory-swap"), "16g");
    assert_eq!(value_after("--name"), container_name());
}

#[tokio::test(flavor = "current_thread")]
async fn a_container_needs_a_prepared_vm_and_job_output_is_forwarded_in_bounded_pieces() {
    let rig = Rig::new();
    let spec = spec(&container_name(), None);
    let jit = Secret::new(JIT.to_string());
    assert!(rig.backend.run_container(&spec, &jit).await.is_err());

    prepared(&rig).await;
    rig.docker_script("run_mode", "longline");
    rig.backend.run_container(&spec, &jit).await.unwrap();
    let lines = rig.log.lines();
    let long: Vec<&String> = lines.iter().filter(|line| line.len() > 1000).collect();
    assert!(
        long.len() >= 3,
        "150 000 bytes go in pieces of 64 KiB at the most"
    );
    assert!(long.iter().all(|line| line.len() <= 64 * 1024 + 8));
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_guest_output_is_cut_at_64_kib() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.fake.script("flood-exec", "");
    let vm = job_vm();
    let guest = marshal_cli::runtime::tart::guest::Guest::new(&rig.fake.client, &vm);
    let answer = guest
        .docker(&["info".to_string()], Duration::from_secs(10))
        .await
        .unwrap();
    assert!(answer.stdout_cut && answer.stderr_cut);
    assert_eq!(
        answer.stdout.len(),
        marshal_cli::runtime::tart::MAX_OUTPUT_BYTES
    );
}

/// runs `run` until the container shows as running, then `act`, and returns what the
/// run ended with.
async fn while_running<A: std::future::Future>(
    rig: &Rig,
    name: &str,
    act: A,
) -> (
    Result<i32, marshal_cli::runtime::backend::BackendError>,
    A::Output,
) {
    let spec = spec(name, None);
    let jit = Secret::new(JIT.to_string());
    let run = rig.backend.run_container(&spec, &jit);
    let act = async {
        let started = Instant::now();
        while !rig.backend.running_containers().contains(&name.to_string()) {
            assert!(started.elapsed() < Duration::from_secs(10), "it never ran");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // the container must exist in the VM before it is stopped
        while !rig.containers().contains(&name.to_string()) {
            assert!(started.elapsed() < Duration::from_secs(10), "it never ran");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        act.await
    };
    tokio::join!(run, act)
}

#[tokio::test(flavor = "current_thread")]
async fn the_stop_sequence_stops_then_removes_the_container_in_its_vm_before_the_vm_goes() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("run_mode", "hold");
    let name = container_name();
    let vm = job_vm();
    let sequence = async {
        rig.backend
            .stop_containers(&StopRequest {
                names: vec![name.clone()],
                grace: Duration::from_secs(7),
                budget: Duration::from_secs(12),
            })
            .await
            .unwrap();
        rig.backend
            .remove_containers(std::slice::from_ref(&name), Duration::from_secs(6))
            .await
            .unwrap();
        rig.backend
            .release_job_host(&host(), Duration::from_secs(10))
            .await
            .unwrap();
    };
    let (exit, ()) = while_running(&rig, &name, sequence).await;
    assert_eq!(exit.unwrap(), 137);
    let calls = rig.tart_calls();
    let position = |wanted: Vec<String>| calls.iter().position(|call| *call == wanted).unwrap();
    let stop = position(
        ["exec", &vm, "docker", "stop", "--time", "7", &name]
            .map(str::to_string)
            .to_vec(),
    );
    let remove = position(
        ["exec", &vm, "docker", "rm", "-f", &name]
            .map(str::to_string)
            .to_vec(),
    );
    let delete = position(["delete", &vm].map(str::to_string).to_vec());
    assert!(stop < remove && remove < delete, "{calls:?}");
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_container_can_be_ended_at_once_and_a_stop_of_an_unknown_one_does_nothing() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("run_mode", "hold");
    let name = container_name();
    let kill = async {
        rig.backend
            .kill_containers(
                &[name.clone(), "unknown".to_string()],
                Duration::from_secs(5),
            )
            .await
            .unwrap();
    };
    let (exit, ()) = while_running(&rig, &name, kill).await;
    assert_eq!(exit.unwrap(), 137);
    let kills: Vec<_> = rig
        .docker_calls()
        .into_iter()
        .filter(|call| call.arguments.first().map(String::as_str) == Some("kill"))
        .map(|call| call.arguments)
        .collect();
    assert_eq!(kills, [["kill", &name]]);
    rig.backend
        .stop_containers(&StopRequest {
            names: vec!["unknown".to_string()],
            grace: Duration::from_secs(1),
            budget: Duration::from_secs(2),
        })
        .await
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn releasing_a_vm_whose_container_runs_ends_the_job_with_the_vm() {
    let rig = Rig::new();
    prepared(&rig).await;
    rig.docker_script("run_mode", "hold");
    let name = container_name();
    let release = async {
        rig.backend
            .release_job_host(&host(), Duration::from_secs(10))
            .await
            .unwrap();
    };
    let spec = spec(&name, None);
    let jit = Secret::new(JIT.to_string());
    let started = Instant::now();
    let run = rig.backend.run_container(&spec, &jit);
    let wait_then_release = async {
        while !rig.containers().contains(&name) {
            assert!(started.elapsed() < Duration::from_secs(10), "it never ran");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        release.await;
    };
    tokio::select! {
        _ = run => panic!("the container ended on its own"),
        () = wait_then_release => {}
    }
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(rig.backend.prepared_hosts().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_diagnostic_container_is_retained_until_a_finalisation_deletes_its_vm_and_a_missing_exporter_is_a_gap()
 {
    let rig = Rig::new();
    prepared(&rig).await;
    let record = DiagnosticRecord {
        pool: POOL.to_string(),
        directory: "/evidence".to_string(),
        raw_records: false,
    };
    let name = container_name();
    let spec = spec(&name, Some(record));
    let exit = rig
        .backend
        .run_container(&spec, &Secret::new(JIT.to_string()))
        .await
        .unwrap();
    assert_eq!(exit, 0);
    assert!(
        rig.backend.retained().contains(&name),
        "kept for its export"
    );
    assert!(
        rig.containers().contains(&name),
        "a diagnostic run is not removed"
    );
    assert!(
        rig.backend.running_containers().is_empty(),
        "a retained container is not a running one"
    );

    assert!(
        rig.backend.host_awaits_finalisation(&host()),
        "its VM holds the evidence"
    );

    finalize_at_job_end(&rig.backend, &*rig.log, &name, tokio::time::Instant::now()).await;
    assert!(!rig.backend.retained().contains(&name));
    assert!(!rig.backend.host_awaits_finalisation(&host()));
    assert_eq!(
        rig.fake.vm_names(),
        [golden_name()],
        "the removal of a diagnostic container is the deletion of its VM"
    );
    assert!(
        rig.log.lines().contains(&format!(
            "WARN: Diagnostic gap in pool {POOL}, container {name}: private export unavailable, incomplete, unsafe or over budget; prior evidence retained."
        )),
        "{:?}",
        rig.log.lines()
    );

    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    let calls = rig.tart_calls();
    assert!(
        calls.iter().all(|call| !call.contains(&"rm".to_string())),
        "no `docker rm` runs in a VM that is deleted: {calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == ["delete", &job_vm()])
            .count(),
        1,
        "the VM is deleted once, whoever asks"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn volumes_are_refused_for_a_vm() {
    let rig = Rig::new();
    rig.backend.prepare_volumes(IMAGE, &[]).await.unwrap();
    let volume = marshal_cli::validate::PlannedVolume {
        name: "v".to_string(),
        mount_path: "/home/runner/.npm".to_string(),
    };
    assert!(rig.backend.prepare_volumes(IMAGE, &[volume]).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn under_the_vm_shim_the_tart_run_is_the_shims_and_dropping_the_backend_deletes_the_vm() {
    let rig = Rig::with(VmLauncher::Shim(PathBuf::from(BINARY)), quick_timings());
    prepared(&rig).await;
    let vm = job_vm();
    assert!(rig.fake.vm_names().contains(&vm));
    let run = rig.tart_calls_of("run");
    assert_eq!(run.len(), 1);
    assert_eq!(run[0].arguments, run_arguments(&vm));
    assert_eq!(
        run[0].no_auto_prune.as_deref(),
        Some("1"),
        "the shim's own tart calls are not pruning either"
    );

    let fake_dir = rig.fake.dir.path().to_path_buf();
    drop(rig.backend);
    let started = Instant::now();
    loop {
        let remaining = fs::read_to_string(fake_dir.join("vms")).unwrap_or_default();
        if !remaining.contains(&vm) {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the shim did not delete the VM: {remaining}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_tart_run_that_fails_under_the_shim_is_a_boot_failure_with_tarts_own_text() {
    let rig = Rig::with(VmLauncher::Shim(PathBuf::from(BINARY)), quick_timings());
    rig.fake.script("run_mode", "exit");
    rig.fake
        .script("run_error", "Error: the login keychain is locked");
    let error = failed(rig.backend.prepare_job_host(&request()).await);
    assert!(
        error.contains("ended with exit code 1 (Error: the login keychain is locked)")
            && error
                .contains("\"The Signed-In Session\" section of the macOS Runner Host document"),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_release_under_the_shim_stops_and_deletes_and_the_shim_ends() {
    let rig = Rig::with(VmLauncher::Shim(PathBuf::from(BINARY)), quick_timings());
    prepared(&rig).await;
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    let run = rig
        .fake
        .invocations()
        .iter()
        .position(|call| call.arguments.first().map(String::as_str) == Some("run"))
        .unwrap();
    assert!(
        rig.fake.ended_within(run, Duration::from_secs(10)),
        "tart run ends with its VM"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_macos_slot_without_a_golden_macos_vm_fails_only_its_own_preparation() {
    let rig = Rig::with_pools(vec![
        ConfiguredPool {
            name: POOL.to_string(),
            os: Os::Macos,
        },
        ConfiguredPool {
            name: "linuxpool".to_string(),
            os: Os::Linux,
        },
    ]);
    let error = failed(rig.backend.prepare_job_host(&request()).await);
    assert!(
        error.contains("no golden macOS VM was found; `marshal build` makes it"),
        "{error}"
    );
    assert!(
        rig.fake
            .invocations()
            .iter()
            .all(|call| call.arguments[0] != "clone"),
        "nothing is cloned for the refused slot"
    );
    let mut vms: Vec<String> = rig
        .fake
        .vms()
        .iter()
        .map(|fields| fields.join("\t"))
        .collect();
    vms.push("marshal-golden-macos-g2-r2.337.0-20240101000000\tstopped\tlocal\t\t\t".to_string());
    rig.fake.script("vms", &vms.join("\n"));
    let error = failed(rig.backend.prepare_job_host(&request()).await);
    assert!(
        error.contains("is version 2, and this CLI runs version 1"),
        "{error}"
    );
    let linux = HostRequest {
        host: JobHost {
            pool: "linuxpool".to_string(),
            slot: 1,
        },
        ..request()
    };
    assert_eq!(
        rig.backend.prepare_job_host(&linux).await,
        HostPreparation::Ready,
        "{:?}",
        rig.log.lines()
    );
}
