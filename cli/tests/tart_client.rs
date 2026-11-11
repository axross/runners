//! the `tart` client, run as a real process against the fake `tart`, and the VM names
//! and listing reader it comes with.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::ffi::OsString;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use marshal_cli::image::contract::{CONTRACT_VERSION, RUNNER_VERSION_LABEL};
use marshal_cli::image::definition::declared_label;
use marshal_cli::runtime::tart::args::{
    SOFTNET_ALLOW, SOFTNET_BLOCK, clone_arguments, delete_arguments, exec_arguments,
    force_stop_arguments, is_digest_reference, list_arguments, pull_arguments, rename_arguments,
    run_arguments, set_arguments, stop_arguments,
};
use marshal_cli::runtime::tart::client::{
    TART_PROGRAM, TEST_TART_VARIABLE, program_from_environment,
};
use marshal_cli::runtime::tart::golden_check::clone_source;
use marshal_cli::runtime::tart::list::{MAX_ENTRIES, parse_vm_list};
use marshal_cli::runtime::tart::names::{
    GOLDEN_VM_VERSION, GoldenVmName, build_vm_name, golden_vms, is_job_vm_of_pools, job_vm_name,
    leftover_build_vms, leftover_job_vms, newest_golden_vm, parse_build_vm_name, parse_job_vm_name,
};
use marshal_cli::runtime::tart::{
    MAX_OUTPUT_BYTES, TartError, TartLimits, VmEntry, VmSource, VmState,
};
use support::tart::{FAKE_TART, Fake};

const JIT_VARIABLE: &str = "ACTIONS_RUNNER_INPUT_JITCONFIG";
const BUDGET: Duration = Duration::from_secs(20);
const GOLDEN: &str = "marshal-golden-linux-g1-c1-r2.337.0-20240305060708";
const DIGEST: &str = "ghcr.io/cirruslabs/ubuntu@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn at(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis)
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

#[test]
fn the_argument_lists_are_exact() {
    assert_eq!(list_arguments(), words(&["list", "--format", "json"]));
    assert_eq!(clone_arguments("a", "b"), words(&["clone", "a", "b"]));
    assert_eq!(
        set_arguments("vm", 4, 16, 60),
        words(&[
            "set",
            "vm",
            "--cpu",
            "4",
            "--memory",
            "16384",
            "--disk-size",
            "60"
        ])
    );
    assert_eq!(rename_arguments("a", "b"), words(&["rename", "a", "b"]));
    assert_eq!(delete_arguments("vm"), words(&["delete", "vm"]));
    assert_eq!(pull_arguments(DIGEST), words(&["pull", DIGEST]));
    assert_eq!(
        stop_arguments("vm", Duration::from_secs(7)),
        words(&["stop", "vm", "--timeout", "7"])
    );
    assert_eq!(
        force_stop_arguments("vm"),
        words(&["stop", "vm", "--timeout", "0"])
    );
    let command = words(&["launcher", "docker", "run", "-e", "X"]);
    assert_eq!(
        exec_arguments("vm", false, &command),
        words(&["exec", "vm", "launcher", "docker", "run", "-e", "X"])
    );
    assert_eq!(
        exec_arguments("vm", true, &command),
        words(&["exec", "-i", "vm", "launcher", "docker", "run", "-e", "X"])
    );
}

#[test]
fn a_run_is_headless_without_clipboard_audio_or_usb_and_on_softnet_with_the_rules() {
    let arguments = run_arguments("marshal-job-pool-1-20240305060708009");
    assert_eq!(
        arguments,
        words(&[
            "run",
            "--no-graphics",
            "--no-clipboard",
            "--no-audio",
            "--no-usb-accessories",
            "--net-softnet",
            "--net-softnet-allow",
            "out proto udp @host port 53,out proto tcp @host port 53",
            "--net-softnet-block",
            "out @host,224.0.0.0/4",
            "marshal-job-pool-1-20240305060708009",
        ])
    );
    assert_eq!(SOFTNET_ALLOW, arguments[7]);
    assert_eq!(SOFTNET_BLOCK, arguments[9]);
    for forbidden in [
        "--dir",
        "--rosetta",
        "--graphics",
        "--net-bridged",
        "--net-host",
        "--net-fd",
        "--net-softnet-expose",
        "--net-softnet-control-fd",
        "--disk",
        "--serial",
        "--serial-path",
        "--vnc",
        "--vnc-experimental",
        "--nested",
        "--suspendable",
        "--recovery",
    ] {
        assert!(
            arguments
                .iter()
                .all(|argument| argument != forbidden
                    && !argument.starts_with(&format!("{forbidden}="))),
            "{forbidden}"
        );
    }
}

#[test]
fn digest_references_are_the_only_pullable_ones() {
    assert!(is_digest_reference(DIGEST));
    for reference in [
        "",
        "ghcr.io/cirruslabs/ubuntu:latest",
        "ghcr.io/cirruslabs/ubuntu@sha256:abc",
        "@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "-x@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "ghcr.io/Cirrus@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "ghcr.io/x@sha256:0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
        "ghcr.io/x@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0",
        "ghcr.io/x y@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    ] {
        assert!(!is_digest_reference(reference), "{reference:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn each_call_reaches_tart_with_its_arguments_and_the_fake_state_follows() {
    let fake = Fake::new();
    fake.seed(&[(GOLDEN, "stopped", "local")]);
    let client = &fake.client;
    let job = "marshal-job-pool-1-20240305060708009";

    client.clone_vm(GOLDEN, job).await.unwrap();
    client.set_resources(job, 4, 8, 60).await.unwrap();
    let mut running = client.spawn_run(job).unwrap();
    let started = Instant::now();
    while !client
        .list()
        .await
        .unwrap()
        .iter()
        .any(|entry| entry.name == job && entry.state == VmState::Running)
    {
        assert!(started.elapsed() < BUDGET, "the VM never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let answer = client
        .exec(job, &words(&["echo", "hi"]), None, BUDGET)
        .await
        .unwrap();
    assert!(answer.succeeded());
    client
        .exec(job, &words(&["cat"]), Some(b"input"), BUDGET)
        .await
        .unwrap();
    client
        .stop(job, Duration::from_secs(5), BUDGET)
        .await
        .unwrap();
    assert_eq!(running.wait().await.unwrap().code(), Some(0));
    client.rename(job, "renamed").await.unwrap();
    client.pull(DIGEST).await.unwrap();
    client.delete("renamed", BUDGET).await.unwrap();

    let calls: Vec<Vec<String>> = fake
        .invocations()
        .into_iter()
        .map(|call| call.arguments)
        .filter(|arguments| arguments != &list_arguments())
        .collect();
    assert_eq!(
        calls,
        vec![
            clone_arguments(GOLDEN, job),
            set_arguments(job, 4, 8, 60),
            run_arguments(job),
            exec_arguments(job, false, &words(&["echo", "hi"])),
            exec_arguments(job, true, &words(&["cat"])),
            stop_arguments(job, Duration::from_secs(5)),
            rename_arguments(job, "renamed"),
            pull_arguments(DIGEST),
            delete_arguments("renamed"),
        ]
    );
    let entries = client.list().await.unwrap();
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, vec![GOLDEN, DIGEST]);
    assert_eq!(entries[1].source, VmSource::Oci);
    assert_eq!(entries[0].state, VmState::Stopped);
}

#[tokio::test(flavor = "current_thread")]
async fn a_clone_gets_the_resources_set_and_a_forced_stop_ends_the_vm() {
    let fake = Fake::new();
    fake.seed(&[(GOLDEN, "stopped", "local")]);
    let job = "marshal-job-pool-1-20240305060708009";
    fake.client.clone_vm(GOLDEN, job).await.unwrap();
    fake.client.set_resources(job, 3, 2, 21).await.unwrap();
    assert_eq!(
        fake.vms()[1],
        vec![job, "stopped", "local", "3", "2048", "21"]
    );
    let mut running = fake.client.spawn_run(job).unwrap();
    while fake.vms()[1][1] != "running" {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    fake.client.force_stop(job, BUDGET).await.unwrap();
    running.wait().await.unwrap();
    assert_eq!(
        fake.invocations().last().unwrap().arguments,
        force_stop_arguments(job)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn every_call_gets_the_environment_and_a_standard_output_that_is_not_a_terminal() {
    let fake = Fake::new();
    fake.seed(&[(GOLDEN, "stopped", "local")]);
    let job = "marshal-job-pool-1-20240305060708009";
    fake.client.list().await.unwrap();
    fake.client.clone_vm(GOLDEN, job).await.unwrap();
    fake.client.set_resources(job, 1, 1, 20).await.unwrap();
    let mut running = fake.client.spawn_run(job).unwrap();
    while fake.vms()[1][1] != "running" {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    fake.client
        .exec(job, &words(&["true"]), Some(b"x"), BUDGET)
        .await
        .unwrap();
    fake.client
        .exec(job, &words(&["true"]), None, BUDGET)
        .await
        .unwrap();
    fake.client.force_stop(job, BUDGET).await.unwrap();
    running.wait().await.unwrap();
    fake.client.rename(job, "x").await.unwrap();
    fake.client.pull(DIGEST).await.unwrap();
    fake.client.delete("x", BUDGET).await.unwrap();

    let calls = fake.invocations();
    assert_eq!(calls.len(), 10);
    let path = fake.path.to_str().unwrap();
    for call in &calls {
        let what = format!("{:?}", call.arguments);
        assert_eq!(call.no_auto_prune.as_deref(), Some("1"), "{what}");
        assert_eq!(call.path.as_deref(), Some(path), "{what}");
        assert!(
            call.path
                .as_deref()
                .unwrap()
                .ends_with(support::tart::PATH_MARKER)
        );
        assert!(!call.stdout_terminal, "{what}");
        assert!(!call.stderr_terminal, "{what}");
        assert!(!call.stdin_terminal, "{what}");
        assert!(!call.jit_present, "{what}");
        #[cfg(unix)]
        assert_eq!(
            call.pgid,
            Some(call.pid),
            "{what} is not its own process group leader"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_jit_configuration_reaches_the_guest_only_on_standard_input() {
    let fake = Fake::new();
    fake.seed(&[("vm", "running", "local")]);
    let jit = b"super-secret-jit-config";
    fake.client
        .exec(
            "vm",
            &words(&["launcher", "docker", "run", "--pull", "never"]),
            Some(jit),
            BUDGET,
        )
        .await
        .unwrap();
    let call = &fake.invocations()[0];
    assert_eq!(call.stdin.as_deref(), Some(&jit[..]));
    assert_eq!(
        call.arguments,
        words(&[
            "exec", "-i", "vm", "launcher", "docker", "run", "--pull", "never"
        ])
    );
    assert!(
        !call
            .arguments
            .iter()
            .any(|argument| argument.contains("jit"))
    );
    assert!(!call.arguments.contains(&"-t".to_string()));
    assert!(!call.jit_present);
}

#[tokio::test(flavor = "current_thread")]
async fn a_configured_jit_variable_is_not_passed_on() {
    let fake = Fake::with_config(|config| {
        config.environment.push((
            OsString::from(JIT_VARIABLE),
            OsString::from("super-secret-jit-config"),
        ));
    });
    fake.client.list().await.unwrap();
    assert!(!fake.invocations()[0].jit_present);
}

/// a child of the test binary whose environment holds the JIT variable.
#[test]
fn the_jit_variable_in_the_callers_environment_is_not_passed_on() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "jit_variable_child", "--nocapture"])
        .env(JIT_VARIABLE, "super-secret-jit-config")
        .env(TEST_TART_VARIABLE, FAKE_TART)
        .env("TART_CLIENT_JIT_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// runs only as the child of the test above: the variable is in its environment.
#[tokio::test(flavor = "current_thread")]
async fn jit_variable_child() {
    if std::env::var_os("TART_CLIENT_JIT_CHILD").is_none() {
        return;
    }
    assert!(std::env::var_os(JIT_VARIABLE).is_some());
    let named = program_from_environment();
    if cfg!(debug_assertions) {
        assert_eq!(named, FAKE_TART);
    } else {
        assert_eq!(named, TART_PROGRAM);
    }
    let fake = Fake::new();
    fake.seed(&[("vm", "stopped", "local")]);
    fake.client.list().await.unwrap();
    let mut running = fake.client.spawn_run("vm").unwrap();
    while fake.vms()[0][1] != "running" {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    fake.client
        .exec("vm", &words(&["true"]), Some(b"x"), BUDGET)
        .await
        .unwrap();
    fake.client.force_stop("vm", BUDGET).await.unwrap();
    running.wait().await.unwrap();
    let calls = fake.invocations();
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|call| !call.jit_present));
}

#[tokio::test(flavor = "current_thread")]
async fn output_is_cut_off_at_sixty_four_kibibytes() {
    assert_eq!(MAX_OUTPUT_BYTES, 64 * 1024);
    let fake = Fake::new();
    fake.seed(&[("vm", "running", "local")]);
    fake.script("flood-exec", "");
    let answer = fake
        .client
        .exec("vm", &words(&["noisy"]), None, BUDGET)
        .await
        .unwrap();
    assert!(answer.succeeded());
    assert_eq!(answer.stdout.len(), 64 * 1024);
    assert_eq!(answer.stderr.len(), 64 * 1024);
    assert!(answer.stdout_cut && answer.stderr_cut);
    assert!(answer.stdout.bytes().all(|byte| byte == b'a'));
}

#[tokio::test(flavor = "current_thread")]
async fn output_below_the_bound_is_kept_whole_and_not_marked_cut() {
    let fake = Fake::new();
    fake.seed(&[("vm", "running", "local")]);
    fake.script("exec_stdout", "hello");
    let answer = fake
        .client
        .exec("vm", &words(&["true"]), None, BUDGET)
        .await
        .unwrap();
    assert_eq!(answer.stdout, "hello\n");
    assert!(!answer.stdout_cut && !answer.stderr_cut);
}

#[tokio::test(flavor = "current_thread")]
async fn a_listing_over_the_bound_is_refused() {
    let fake = Fake::new();
    fake.script("flood-list", "");
    let error = fake.client.list().await.unwrap_err();
    assert!(matches!(error, TartError::Unreadable(_)), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn the_output_of_a_running_vm_is_cut_off_too() {
    let fake = Fake::new();
    fake.seed(&[("vm", "stopped", "local")]);
    fake.script("flood-run", "");
    fake.script("run_mode", "exit");
    let mut running = fake.client.spawn_run("vm").unwrap();
    let status = running.wait().await.unwrap();
    let started = Instant::now();
    loop {
        let output = running.output(status.code());
        if output.stdout.len() == 64 * 1024 && output.stderr.len() >= 64 * 1024 {
            assert!(output.stdout_cut);
            break;
        }
        assert!(started.elapsed() < BUDGET, "the output never arrived");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_call_that_hangs_ends_at_its_budget_and_its_client_is_killed() {
    let fake = Fake::with_config(|config| {
        config.limits = TartLimits {
            command_budget: Duration::from_millis(300),
            pull_budget: Duration::from_millis(300),
        };
    });
    fake.seed(&[("vm", "running", "local")]);
    for (number, subcommand) in [
        "list", "clone", "set", "rename", "delete", "pull", "stop", "exec",
    ]
    .into_iter()
    .enumerate()
    {
        fake.script(&format!("hang-{subcommand}"), "");
        let started = Instant::now();
        let client = &fake.client;
        let budget = Duration::from_millis(300);
        let error = match subcommand {
            "list" => client.list().await.err(),
            "clone" => client.clone_vm("vm", "other").await.err(),
            "set" => client.set_resources("vm", 1, 1, 20).await.err(),
            "rename" => client.rename("vm", "other").await.err(),
            "delete" => client.delete("vm", budget).await.err(),
            "pull" => client.pull(DIGEST).await.err(),
            "stop" => client
                .stop("vm", Duration::from_secs(1), budget)
                .await
                .err(),
            _ => client.exec("vm", &words(&["x"]), None, budget).await.err(),
        };
        assert!(
            matches!(error, Some(TartError::TimedOut(_))),
            "{subcommand}: {error:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(10), "{subcommand}");
        assert!(
            fake.ended_within(number, Duration::from_secs(10)),
            "the hung {subcommand} client was not ended"
        );
    }
}

#[test]
fn the_default_budgets_bound_every_call() {
    let limits = TartLimits::default();
    assert_eq!(limits.command_budget, Duration::from_secs(120));
    assert_eq!(limits.pull_budget, Duration::from_secs(30 * 60));
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_has_no_budget_and_dropping_it_kills_the_client() {
    let fake = Fake::new();
    fake.seed(&[("vm", "stopped", "local")]);
    let running = fake.client.spawn_run("vm").unwrap();
    while fake.vms()[0][1] != "running" {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(running);
    assert!(fake.ended_within(0, Duration::from_secs(10)));
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_call_carries_the_clients_scrubbed_bounded_error() {
    let fake = Fake::new();
    fake.script("fail-clone", "Error:   the disk\nis full");
    let error = fake.client.clone_vm("a", "b").await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "tart clone failed: Error: the disk is full"
    );
    fake.script("fail-clone", &"x ".repeat(2000));
    let error = fake.client.clone_vm("a", "b").await.unwrap_err();
    assert!(error.to_string().ends_with("..."), "{error}");
    assert!(error.to_string().len() < 600);
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_run_shows_its_error_and_exit_code() {
    let fake = Fake::new();
    fake.seed(&[("vm", "stopped", "local")]);
    fake.script("run_mode", "exit");
    fake.script("run_error", "the keychain is locked");
    fake.script("run_exit", "3");
    let mut running = fake.client.spawn_run("vm").unwrap();
    let status = running.wait().await.unwrap();
    assert_eq!(status.code(), Some(3));
    let started = Instant::now();
    while !running
        .output(status.code())
        .stderr
        .contains("the keychain is locked")
    {
        assert!(started.elapsed() < BUDGET);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_client_that_cannot_start_is_unavailable() {
    let fake = Fake::with_config(|config| {
        config.program = OsString::from("/nonexistent/tart");
    });
    assert!(matches!(
        fake.client.list().await,
        Err(TartError::Unavailable(_))
    ));
    assert!(matches!(
        fake.client.spawn_run("vm"),
        Err(TartError::Unavailable(_))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_name_tart_would_read_as_an_option_and_a_pull_not_by_digest_are_never_sent() {
    let fake = Fake::new();
    assert!(matches!(
        fake.client.clone_vm("-x", "b").await,
        Err(TartError::Refused(_))
    ));
    assert!(matches!(
        fake.client.delete("", BUDGET).await,
        Err(TartError::Refused(_))
    ));
    assert!(matches!(
        fake.client.rename("a", "--dir=/").await,
        Err(TartError::Refused(_))
    ));
    assert!(matches!(
        fake.client.spawn_run("--net-host"),
        Err(TartError::Refused(_))
    ));
    assert!(matches!(
        fake.client.pull("ghcr.io/cirruslabs/ubuntu:latest").await,
        Err(TartError::Refused(_))
    ));
    assert!(fake.invocations().is_empty());
}

#[test]
fn job_vm_names_are_the_pool_the_slot_and_the_time() {
    let name = job_vm_name("example-repo-one", 2, at(1_709_618_828_009));
    assert_eq!(name, "marshal-job-example-repo-one-2-20240305060708009");
    let parsed = parse_job_vm_name(&name).unwrap();
    assert_eq!(parsed.pool, "example-repo-one");
    assert_eq!(parsed.slot, 2);
    assert_eq!(parsed.started_millis, 1_709_618_828_009);
}

#[test]
fn build_vm_names_are_linux_and_the_time() {
    let name = build_vm_name(at(1_709_618_828_009));
    assert_eq!(name, "marshal-build-linux-20240305060708009");
    assert_eq!(parse_build_vm_name(&name), Some(1_709_618_828_009));
    for other in [
        "marshal-build-linux-20240305060708",
        "marshal-build-linux-20240230060708009",
        "marshal-build-linux-20240305060708009x",
        "marshal-build-linux-",
        "marshal-build-macos-20240305060708009",
        GOLDEN,
    ] {
        assert_eq!(parse_build_vm_name(other), None, "{other}");
    }
}

#[test]
fn the_golden_vm_name_records_the_versions_and_the_build_second() {
    let runner = declared_label(RUNNER_VERSION_LABEL).unwrap();
    let current = GoldenVmName::current(at(1_709_618_828_999)).unwrap();
    assert_eq!(GOLDEN_VM_VERSION, 2);
    assert_eq!(current.golden_version, GOLDEN_VM_VERSION);
    assert_eq!(current.contract_version, CONTRACT_VERSION);
    assert_eq!(current.runner_version, runner);
    assert_eq!(
        current.to_string(),
        format!(
            "marshal-golden-linux-g{GOLDEN_VM_VERSION}-c{CONTRACT_VERSION}-r{runner}-20240305060708"
        )
    );
    assert_eq!(
        GoldenVmName::parse(&current.to_string()),
        Some(current.clone())
    );

    let parsed = GoldenVmName::parse(GOLDEN).unwrap();
    assert_eq!(parsed.golden_version, 1);
    assert_eq!(parsed.contract_version, 1);
    assert_eq!(parsed.runner_version, "2.337.0");
    assert_eq!(parsed.built_seconds, 1_709_618_828);
    assert_eq!(parsed.to_string(), GOLDEN);
}

#[test]
fn a_golden_vm_name_with_another_version_reads_back_and_only_another_runner_version_still_runs_here()
 {
    let current = GoldenVmName::current(at(0)).unwrap();
    let mut other_golden = current.clone();
    other_golden.golden_version += 1;
    let mut other_contract = current.clone();
    other_contract.contract_version += 1;
    let mut other_runner = current.clone();
    other_runner.runner_version = "0.0.1".to_string();
    for (other, runs_here) in [
        (other_golden, false),
        (other_contract, false),
        (other_runner, true),
    ] {
        let name = other.to_string();
        assert_eq!(GoldenVmName::parse(&name).as_ref(), Some(&other), "{name}");
        assert_ne!(other, current, "{name}");
        assert_eq!(clone_source(&[local(&name)]).is_some(), runs_here, "{name}");
    }
}

#[test]
fn only_exact_names_read_back() {
    for name in [
        "marshal-golden-linux-g1-c1-r2.337.0-2024030506070",
        "marshal-golden-linux-g1-c1-r2.337.0-202403050607089",
        "marshal-golden-linux-g1-c1-r2.337.0-20240305060708009",
        "marshal-golden-linux-g01-c1-r2.337.0-20240305060708",
        "marshal-golden-linux-g0-c1-r2.337.0-20240305060708",
        "marshal-golden-linux-g1-c1-r2.337.0.-20240305060708",
        "marshal-golden-linux-g1-c1-r2..337-20240305060708",
        "marshal-golden-linux-g1-c1-r-20240305060708",
        "marshal-golden-linux-g1-c1-r2.337.0-rc1-20240305060708",
        "marshal-golden-linux-g1-c1-r2.337.0-20240230060708",
        "marshal-golden-linux-g1-r2.337.0-20240305060708",
        "marshal-golden-linux-g1-c1-2.337.0-20240305060708",
        "marshal-golden-linux-gx-c1-r2.337.0-20240305060708",
        "marshal-golden-linux-g+1-c1-r2.337.0-20240305060708",
        "marshal-golden-macos-g1-c1-r2.337.0-20240305060708",
        "marshal-golden-g1-c1-r2.337.0-20240305060708",
        "Marshal-golden-linux-g1-c1-r2.337.0-20240305060708",
        "marshal-golden-linux-g1-c1-r2.337.0-20240305060708\n",
        " marshal-golden-linux-g1-c1-r2.337.0-20240305060708",
    ] {
        assert_eq!(GoldenVmName::parse(name), None, "{name:?}");
    }
    for name in [
        "marshal-job-example-20240305060708009",
        "marshal-job-example-x-20240305060708009",
        "marshal-job--1-20240305060708009",
        "marshal-job-example-1-2024030506070800",
        "marshal-job-example-1-20240305060708",
        "marshal-job-example-1-20240230060708009",
        "marshal-job-example-1-20240305060708009\n",
        "marshal-job-example-99999999999-20240305060708009",
        "marshal-jobs-example-1-20240305060708009",
        "Marshal-job-example-1-20240305060708009",
        "example-1-20240305060708009",
    ] {
        assert_eq!(parse_job_vm_name(name), None, "{name:?}");
    }
}

fn local(name: &str) -> VmEntry {
    VmEntry {
        name: name.to_string(),
        source: VmSource::Local,
        state: VmState::Stopped,
    }
}

#[test]
fn the_job_vm_matcher_matches_only_job_vms_of_configured_pools() {
    let pools = ["example-repo-one", "example-repo-two"];
    let stamp = "20240305060708009";
    let mine = [
        format!("marshal-job-example-repo-one-1-{stamp}"),
        format!("marshal-job-example-repo-one-12-{stamp}"),
        format!("marshal-job-example-repo-two-2-{stamp}"),
    ];
    for name in &mine {
        assert!(is_job_vm_of_pools(name, &pools), "{name}");
    }
    let not_mine = [
        ("a golden VM", GOLDEN.to_string()),
        ("a build VM", format!("marshal-build-linux-{stamp}")),
        (
            "a pool no longer configured",
            format!("marshal-job-example-repo-three-1-{stamp}"),
        ),
        (
            "a pool that extends a configured one",
            format!("marshal-job-example-repo-one-extra-1-{stamp}"),
        ),
        (
            "a pool that a configured one extends",
            format!("marshal-job-example-1-{stamp}"),
        ),
        ("a foreign VM", "ubuntu-base".to_string()),
        ("a container name", format!("example-repo-one-1-{stamp}")),
        (
            "a short time",
            "marshal-job-example-repo-one-1-2024030506070800".to_string(),
        ),
        (
            "a long time",
            format!("marshal-job-example-repo-one-1-{stamp}0"),
        ),
        (
            "a non-numeric slot",
            format!("marshal-job-example-repo-one-x-{stamp}"),
        ),
        (
            "another case",
            format!("marshal-job-Example-repo-one-1-{stamp}"),
        ),
        (
            "a trailing newline",
            format!("marshal-job-example-repo-one-1-{stamp}\n"),
        ),
        (
            "a time that is no date",
            "marshal-job-example-repo-one-1-20241301060708009".to_string(),
        ),
    ];
    for (what, name) in &not_mine {
        assert!(!is_job_vm_of_pools(name, &pools), "{what}: {name}");
    }
    assert!(!is_job_vm_of_pools(&mine[0], &[] as &[&str]));
}

#[test]
fn a_pool_that_extends_another_never_matches_through_its_prefix() {
    let name = "marshal-job-a-1-2-20240305060708009";
    assert!(!is_job_vm_of_pools(name, &["a"]));
    assert!(is_job_vm_of_pools(name, &["a-1"]));
    assert!(is_job_vm_of_pools(name, &["a", "a-1"]));
}

#[test]
fn cleanup_picks_job_vms_of_configured_pools_and_build_vms_from_the_local_ones_only() {
    let stamp = "20240305060708009";
    let mut entries = vec![
        local(&format!("marshal-job-example-repo-one-1-{stamp}")),
        local(&format!("marshal-job-other-1-{stamp}")),
        local(GOLDEN),
        local(&format!("marshal-build-linux-{stamp}")),
        local("ubuntu-base"),
    ];
    entries.push(VmEntry {
        name: format!("marshal-job-example-repo-one-2-{stamp}"),
        source: VmSource::Oci,
        state: VmState::Stopped,
    });
    entries.push(VmEntry {
        name: format!("marshal-build-linux-{stamp}"),
        source: VmSource::Oci,
        state: VmState::Stopped,
    });
    let jobs = leftover_job_vms(&entries, &["example-repo-one"]);
    assert_eq!(
        jobs.iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec![format!("marshal-job-example-repo-one-1-{stamp}")]
    );
    let builds = leftover_build_vms(&entries);
    assert_eq!(builds.len(), 1);
    assert_eq!(builds[0].source, VmSource::Local);
    assert_eq!(golden_vms(&entries).len(), 1);
}

#[test]
fn the_newest_golden_vm_is_the_one_built_last() {
    let entries = [
        local("marshal-golden-linux-g1-c1-r2.337.0-20240101000000"),
        local("marshal-golden-linux-g1-c1-r2.337.0-20240305060708"),
        local("marshal-golden-linux-g1-c1-r2.337.0-20231231235959"),
        local("marshal-golden-linux-g1-c1-r2.337.0-20240305060707"),
        local("ubuntu-base"),
        VmEntry {
            name: "marshal-golden-linux-g1-c1-r2.337.0-20250101000000".to_string(),
            source: VmSource::Oci,
            state: VmState::Stopped,
        },
    ];
    let (entry, name) = newest_golden_vm(&entries).unwrap();
    assert_eq!(
        entry.name,
        "marshal-golden-linux-g1-c1-r2.337.0-20240305060708"
    );
    assert_eq!(name.built_seconds, 1_709_618_828);
}

#[test]
fn the_newest_golden_vm_wins_even_when_its_versions_are_not_this_cli() {
    let entries = [
        local("marshal-golden-linux-g1-c1-r2.337.0-20240101000000"),
        local("marshal-golden-linux-g9-c1-r2.337.0-20240201000000"),
    ];
    let (_, name) = newest_golden_vm(&entries).unwrap();
    assert_eq!(name.golden_version, 9);
    assert_ne!(name.golden_version, GOLDEN_VM_VERSION);
}

#[test]
fn a_tie_in_build_time_goes_to_the_name_that_sorts_last() {
    let entries = [
        local("marshal-golden-linux-g1-c1-r2.337.0-20240305060708"),
        local("marshal-golden-linux-g1-c1-r2.338.0-20240305060708"),
    ];
    let (entry, _) = newest_golden_vm(&entries).unwrap();
    assert_eq!(
        entry.name,
        "marshal-golden-linux-g1-c1-r2.338.0-20240305060708"
    );
}

#[test]
fn no_golden_vm_gives_none() {
    assert!(newest_golden_vm(&[]).is_none());
    assert!(newest_golden_vm(&[local("ubuntu-base"), local("marshal-golden-linux-x")]).is_none());
}

/// the listing as Tart 2.41.1 prints it, with its escaped slashes.
const REAL_LISTING: &str = r#"[
  {
    "Source" : "local",
    "Name" : "marshal-golden-linux-g1-c1-r2.337.0-20240305060708",
    "Disk" : 20,
    "Size" : 3,
    "Accessed" : "2026-01-01T00:00:00Z",
    "Running" : false,
    "State" : "stopped"
  },
  {
    "Source" : "OCI",
    "Name" : "ghcr.io\/cirruslabs\/ubuntu@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "Disk" : 20,
    "Size" : 3,
    "Accessed" : "2026-01-01T00:00:00Z",
    "Running" : false,
    "State" : "stopped"
  },
  {
    "Source" : "local",
    "Name" : "marshal-job-pool-1-20240305060708009",
    "Disk" : null,
    "Size" : 1,
    "Accessed" : "2026-01-01T00:00:00Z",
    "Running" : true,
    "State" : "running",
    "Extra" : [1, 2, {"deep": null}]
  }
]
"#;

#[test]
fn a_listing_reads_the_name_the_state_and_the_source() {
    let entries = parse_vm_list(REAL_LISTING.as_bytes()).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, GOLDEN);
    assert_eq!(entries[0].source, VmSource::Local);
    assert_eq!(entries[0].state, VmState::Stopped);
    assert_eq!(entries[1].name, DIGEST);
    assert_eq!(entries[1].source, VmSource::Oci);
    assert_eq!(entries[2].state, VmState::Running);
    assert_eq!(parse_vm_list(b"[\n\n]\n").unwrap(), Vec::new());
}

#[test]
fn a_listing_of_any_other_shape_is_refused() {
    let entry = |name: &str, state: &str, source: &str| {
        format!(r#"{{"Name": {name}, "State": {state}, "Source": {source}}}"#)
    };
    let good = entry("\"a\"", "\"stopped\"", "\"local\"");
    assert!(parse_vm_list(format!("[{good}]").as_bytes()).is_ok());
    let too_many = format!("[{}]", vec![good.as_str(); MAX_ENTRIES + 1].join(","));
    let overlong = "a".repeat(513);
    let cases: Vec<(&str, String)> = vec![
        ("empty", String::new()),
        ("not json", "VM list".to_string()),
        ("an object", format!("{{\"vms\": [{good}]}}")),
        ("a string", "\"x\"".to_string()),
        ("null", "null".to_string()),
        ("trailing text", format!("[{good}] x")),
        ("an array entry", "[[]]".to_string()),
        ("a null entry", "[null]".to_string()),
        ("a string entry", "[\"a\"]".to_string()),
        (
            "no name",
            r#"[{"State": "stopped", "Source": "local"}]"#.to_string(),
        ),
        (
            "no state",
            r#"[{"Name": "a", "Source": "local"}]"#.to_string(),
        ),
        (
            "no source",
            r#"[{"Name": "a", "State": "stopped"}]"#.to_string(),
        ),
        (
            "a lowercase key",
            r#"[{"name": "a", "State": "stopped", "Source": "local"}]"#.to_string(),
        ),
        (
            "a numeric name",
            format!("[{}]", entry("1", "\"stopped\"", "\"local\"")),
        ),
        (
            "a null state",
            format!("[{}]", entry("\"a\"", "null", "\"local\"")),
        ),
        (
            "an object source",
            format!("[{}]", entry("\"a\"", "\"stopped\"", "{}")),
        ),
        (
            "an empty name",
            format!("[{}]", entry("\"\"", "\"stopped\"", "\"local\"")),
        ),
        (
            "a name over 512 bytes",
            format!(
                "[{}]",
                entry(&format!("\"{overlong}\""), "\"stopped\"", "\"local\"")
            ),
        ),
        (
            "a control character in the name",
            format!("[{}]", entry("\"a\\nb\"", "\"stopped\"", "\"local\"")),
        ),
        (
            "an unknown state",
            format!("[{}]", entry("\"a\"", "\"paused\"", "\"local\"")),
        ),
        (
            "another case of a state",
            format!("[{}]", entry("\"a\"", "\"Stopped\"", "\"local\"")),
        ),
        (
            "an unknown source",
            format!("[{}]", entry("\"a\"", "\"stopped\"", "\"remote\"")),
        ),
        ("too many entries", too_many),
        (
            "a repeated key with another case",
            r#"[{"Name": "a", "name": "b", "State": "stopped", "Source": "local"}]"#.to_string(),
        ),
    ];
    for (what, text) in cases {
        assert!(parse_vm_list(text.as_bytes()).is_err(), "{what}");
    }
}

#[test]
fn a_listing_that_is_not_utf8_is_refused() {
    assert!(parse_vm_list(&[b'[', 0xff, b']']).is_err());
}
