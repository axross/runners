//! the VM shim run as a real process against the fake `tart`: it holds a VM's
//! `tart run`, and when its liveness pipe closes it stops and deletes the VM, within
//! its own bounded budget, for a job VM and for a build VM alike.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use marshal_cli::runtime::tart::{HeldVm, VmLauncher};
use marshal_cli::vm_shim::{DIAGNOSTIC_POOL_OPTION, VM_SHIM_COMMAND, cleanup_budget};
use support::BINARY;
use support::tart::{FAKE_TART, Fake};

const JOB_VM: &str = "marshal-job-example-repo-one-2-20240305060708009";
const BUILD_VM: &str = "marshal-build-linux-20240305060708009";

fn start_shim(fake: &Fake, vm: &str) -> Child {
    Command::new(BINARY)
        .arg(VM_SHIM_COMMAND)
        .arg("--name")
        .arg(vm)
        .arg("--")
        .arg(FAKE_TART)
        .env("FAKE_TART_DIR", fake.dir.path())
        .env("PATH", &fake.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn wait_until(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < within, "{what} did not happen");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn running(fake: &Fake, vm: &str) -> bool {
    fake.vms()
        .iter()
        .any(|fields| fields[0] == vm && fields[1] == "running")
}

fn wait_for_exit(shim: &mut Child, within: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = shim.try_wait().unwrap() {
            return status;
        }
        assert!(started.elapsed() < within, "the shim did not end");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn closing_the_liveness_pipe_stops_and_deletes_a_job_vm_and_a_build_vm() {
    for vm in [JOB_VM, BUILD_VM] {
        let fake = Fake::new();
        fake.seed(&[(vm, "stopped", "local")]);
        let mut shim = start_shim(&fake, vm);
        wait_until("the VM running", Duration::from_secs(10), || {
            running(&fake, vm)
        });

        drop(shim.stdin.take());
        let status = wait_for_exit(&mut shim, cleanup_budget() + Duration::from_secs(5));
        assert_eq!(status.code(), Some(0), "{vm}");
        assert!(fake.vm_names().is_empty(), "{vm}: {:?}", fake.vm_names());

        let calls: Vec<Vec<String>> = fake
            .invocations()
            .into_iter()
            .map(|call| call.arguments)
            .collect();
        assert_eq!(calls[0][0], "run", "{calls:?}");
        assert!(calls.contains(&vec![
            "stop".to_string(),
            vm.to_string(),
            "--timeout".to_string(),
            "5".to_string()
        ]));
        assert!(calls.contains(&vec!["delete".to_string(), vm.to_string()]));
        assert!(
            fake.invocations()
                .iter()
                .all(|call| call.no_auto_prune.as_deref() == Some("1")),
            "none of the shim's calls prunes"
        );
    }
}

#[test]
fn a_vm_that_ignores_the_graceful_stop_is_stopped_forcibly_within_the_shims_budget() {
    let fake = Fake::new();
    fake.seed(&[(JOB_VM, "stopped", "local")]);
    fake.script("stop_ignored", "");
    let mut shim = start_shim(&fake, JOB_VM);
    wait_until("the VM running", Duration::from_secs(10), || {
        running(&fake, JOB_VM)
    });

    let closed = Instant::now();
    drop(shim.stdin.take());
    let status = wait_for_exit(&mut shim, cleanup_budget() + Duration::from_secs(5));
    assert_eq!(status.code(), Some(0));
    assert!(
        closed.elapsed() < cleanup_budget(),
        "{:?}",
        closed.elapsed()
    );
    assert!(fake.vm_names().is_empty());
    let stops: Vec<String> = fake
        .invocations()
        .into_iter()
        .filter(|call| call.arguments[0] == "stop")
        .map(|call| call.arguments[3].clone())
        .collect();
    assert_eq!(stops, ["5", "0"], "graceful, then forced");
}

#[test]
fn a_deletion_that_hangs_ends_the_shim_at_its_bound_with_a_failure() {
    let fake = Fake::new();
    fake.seed(&[(JOB_VM, "stopped", "local")]);
    fake.script("hang-delete", "");
    let mut shim = start_shim(&fake, JOB_VM);
    wait_until("the VM running", Duration::from_secs(10), || {
        running(&fake, JOB_VM)
    });

    let closed = Instant::now();
    drop(shim.stdin.take());
    let status = wait_for_exit(&mut shim, cleanup_budget() + Duration::from_secs(5));
    assert_eq!(status.code(), Some(1));
    assert!(
        closed.elapsed() < cleanup_budget(),
        "{:?}",
        closed.elapsed()
    );
    assert_eq!(
        fake.vm_names(),
        [JOB_VM],
        "the VM is kept for the next start"
    );
}

#[test]
fn a_tart_run_that_ends_by_itself_is_relayed_and_nothing_is_deleted() {
    let fake = Fake::new();
    fake.seed(&[(JOB_VM, "stopped", "local")]);
    fake.script("run_mode", "exit");
    fake.script("run_error", "Error: the login keychain is locked");
    fake.script("run_exit", "3");
    let mut shim = start_shim(&fake, JOB_VM);
    // the pipe stays open, as a CLI's does while it waits for the VM
    let status = wait_for_exit(&mut shim, Duration::from_secs(10));
    assert_eq!(status.code(), Some(3), "Tart's own exit code");
    let mut relayed = String::new();
    shim.stdout
        .take()
        .unwrap()
        .read_to_string(&mut relayed)
        .unwrap();
    assert!(
        relayed.contains("Error: the login keychain is locked"),
        "{relayed}"
    );
    assert_eq!(fake.vm_names(), [JOB_VM]);
    assert!(
        fake.invocations()
            .iter()
            .all(|call| call.arguments[0] == "run")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn no_shim_holds_the_instance_lock() {
    let fake = Fake::new();
    fake.seed(&[(JOB_VM, "stopped", "local")]);
    let lock_path = fake.dir.path().join("instance.lock");
    let lock = fs::File::create(&lock_path).unwrap();
    lock.lock().unwrap();

    let held = HeldVm::start(&fake.client, &VmLauncher::Shim(BINARY.into()), JOB_VM).unwrap();
    let started = Instant::now();
    while !running(&fake, JOB_VM) {
        assert!(started.elapsed() < Duration::from_secs(10), "it never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    drop(lock);
    let next = fs::File::open(&lock_path).unwrap();
    // a child that another test thread forks while the lock file is open shares its
    // open file description, and so the lock, until that child execs; the shim lives
    // on, so a lock still held after the budget is the shim's
    let released = Instant::now();
    while let Err(blocked) = next.try_lock() {
        assert!(
            released.elapsed() < Duration::from_secs(5),
            "the CLI's lock is free while its shim lives: {blocked}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(held);
    let closed = Instant::now();
    while !fake.vm_names().is_empty() {
        assert!(
            closed.elapsed() < cleanup_budget(),
            "the VM was not deleted"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn start_diagnostic_shim(fake: &Fake, vm: &str, pool: &str) -> Child {
    Command::new(BINARY)
        .arg(VM_SHIM_COMMAND)
        .arg("--name")
        .arg(vm)
        .arg(DIAGNOSTIC_POOL_OPTION)
        .arg(pool)
        .arg("--")
        .arg(FAKE_TART)
        .env("FAKE_TART_DIR", fake.dir.path())
        .env("PATH", &fake.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// the standard error of a shim that has ended.
fn error_text(shim: &mut Child) -> String {
    let mut text = String::new();
    shim.stderr
        .take()
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

const GAP: &str = "WARNING: Diagnostic gap in pool example-repo-one, job VM marshal-job-example-repo-one-2-20240305060708009: private export unavailable, incomplete, unsafe or over budget; prior evidence retained.\n";

/// D4: a crash of the CLI closes the pipe; the shim of a diagnostics-enabled pool's VM
/// deletes it and reports the evidence that loses, with the container's gap text.
#[test]
fn a_shim_that_deletes_the_vm_of_a_diagnostics_pool_reports_the_lost_evidence() {
    let fake = Fake::new();
    fake.seed(&[(JOB_VM, "stopped", "local")]);
    let mut shim = start_diagnostic_shim(&fake, JOB_VM, "example-repo-one");
    wait_until("the VM running", Duration::from_secs(10), || {
        running(&fake, JOB_VM)
    });

    drop(shim.stdin.take());
    let status = wait_for_exit(&mut shim, cleanup_budget() + Duration::from_secs(5));

    assert_eq!(status.code(), Some(0));
    assert!(fake.vm_names().is_empty());
    assert_eq!(error_text(&mut shim), GAP);
}

/// D4: a VM that is already gone holds no evidence the shim could lose, so the shim says
/// nothing; neither does the shim of a pool without diagnostics.
#[test]
fn a_shim_reports_nothing_for_a_vm_already_gone_or_a_pool_without_diagnostics() {
    let fake = Fake::new();
    fake.seed(&[(JOB_VM, "stopped", "local")]);
    let mut shim = start_diagnostic_shim(&fake, JOB_VM, "example-repo-one");
    wait_until("the VM running", Duration::from_secs(10), || {
        running(&fake, JOB_VM)
    });
    // the CLI's own release stops the VM, which ends `tart run`, and deletes it
    for arguments in [&["stop", JOB_VM, "--timeout", "0"][..], &["delete", JOB_VM]] {
        let output = Command::new(FAKE_TART)
            .args(arguments)
            .env("FAKE_TART_DIR", fake.dir.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{arguments:?}");
    }
    drop(shim.stdin.take());
    wait_for_exit(&mut shim, cleanup_budget() + Duration::from_secs(5));
    assert_eq!(error_text(&mut shim), "");

    let fake = Fake::new();
    fake.seed(&[(JOB_VM, "stopped", "local")]);
    let mut shim = start_shim(&fake, JOB_VM);
    wait_until("the VM running", Duration::from_secs(10), || {
        running(&fake, JOB_VM)
    });
    drop(shim.stdin.take());
    wait_for_exit(&mut shim, cleanup_budget() + Duration::from_secs(5));
    assert!(fake.vm_names().is_empty());
    assert_eq!(error_text(&mut shim), "");
}
