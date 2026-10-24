//! The slot's loop, driven through the fake seams and a clock that does not wait.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::Duration;

use runners_cli::runtime::backend::{BackendError, ContainerSpec, Mount};
use runners_cli::runtime::clock::SystemClock;
use runners_cli::runtime::fake::{Call, FakeBackend, FakeClock, FakeRegistrar, MemoryLog};
use runners_cli::runtime::registration::RegistrationError;
use runners_cli::runtime::worker::{run_slot, stop_channel};
use support::{TempDir, slot_spec, write_token_file};

const TOKEN: &str = "ghp_slottoken";
const START: u64 = 1_709_618_828_009;

struct Rig {
    scratch: TempDir,
    backend: FakeBackend,
    registrar: FakeRegistrar,
    clock: FakeClock,
    log: MemoryLog,
}

impl Rig {
    fn new() -> Self {
        let scratch = TempDir::new();
        scratch.write_token("token", format!("  {TOKEN}\n").as_bytes());
        Self {
            scratch,
            backend: FakeBackend::default(),
            registrar: FakeRegistrar::default(),
            clock: FakeClock::at(START),
            log: MemoryLog::default(),
        }
    }

    /// Runs the slot until the scripted exits run out.
    async fn run(&self) {
        let (stop, receiver) = stop_channel();
        self.backend.state.borrow_mut().stop_when_exits_run_out = Some(stop);
        let slot = slot_spec(self.scratch.path().join("token"));
        run_slot(
            &slot,
            &self.backend,
            &self.clock,
            &self.registrar,
            &self.log,
            receiver,
        )
        .await;
    }

    fn seconds(&self) -> Vec<u64> {
        self.clock.sleeps().iter().map(Duration::as_secs).collect()
    }

    fn warnings(&self) -> Vec<String> {
        self.log
            .lines()
            .into_iter()
            .filter(|line| line.starts_with("WARN: "))
            .collect()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_failing_probe_backs_off_5_10_20_seconds_and_registers_nothing() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        for _ in 0..3 {
            state.cpusets.push_back(Err(BackendError::new(
                "CPU discovery: allowed CPU probe failed.",
            )));
        }
    }
    rig.run().await;
    assert_eq!(rig.seconds()[..3], [5, 10, 20]);
    let warnings = rig.warnings();
    assert_eq!(
        warnings[0],
        "WARN: example-repo-one slot 2: CPU discovery or probe cleanup failed; no registration - retrying in 5s."
    );
    assert!(warnings[2].ends_with("retrying in 20s."));
    // only the fourth attempt registered.
    assert_eq!(rig.registrar.seen.borrow().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn the_backoff_doubles_to_300_seconds_and_resets_only_on_exit_zero() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        for _ in 0..9 {
            state.exits.push_back(Ok(1));
        }
        state.exits.push_back(Ok(0));
        state.exits.push_back(Ok(2));
        state
            .exits
            .push_back(Err(BackendError::new("the docker client could not start")));
    }
    rig.run().await;
    assert_eq!(
        rig.seconds(),
        [5, 10, 20, 40, 80, 160, 300, 300, 300, 5, 10]
    );
    let warnings = rig.warnings();
    assert_eq!(
        warnings[0],
        "WARN: example-repo-one slot 2: container exited with code 1 - retrying in 5s."
    );
    assert!(warnings[9].contains("code 2") && warnings[9].ends_with("retrying in 5s."));
    assert!(warnings[10].contains("could not start") && warnings[10].ends_with("retrying in 10s."));
    assert!(rig.log.lines().contains(
        &"INFO: example-repo-one slot 2: container exited successfully (GitHub owns the job result) - starting a replacement.".to_string()
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn an_exit_of_zero_starts_a_replacement_without_a_delay() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        state.exits.push_back(Ok(0));
        state.exits.push_back(Ok(0));
    }
    rig.run().await;
    assert!(rig.clock.sleeps().is_empty());
    let runs = rig
        .backend
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::RunContainer(_)))
        .count();
    assert_eq!(runs, 3);
}

#[tokio::test(flavor = "current_thread")]
async fn a_registration_failure_backs_off_and_the_message_is_the_scrubbed_error() {
    let rig = Rig::new();
    rig.registrar
        .results
        .borrow_mut()
        .push_back(Err(RegistrationError::new(
            "Response status code does not indicate success: 403 (Forbidden). <token>",
        )));
    rig.registrar
        .results
        .borrow_mut()
        .push_back(Err(RegistrationError::new(
            "GitHub returned no JIT configuration",
        )));
    rig.run().await;
    assert_eq!(rig.seconds()[..2], [5, 10]);
    assert_eq!(
        rig.warnings()[0],
        "WARN: example-repo-one slot 2: failed to request a JIT registration (Response status code does not indicate success: 403 (Forbidden). <token>) - retrying in 5s."
    );
    // nothing ran for the failed attempts.
    let state = rig.backend.state.borrow();
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|c| matches!(c, Call::RunContainer(_)))
            .count(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_or_blank_token_file_is_a_backoff_not_a_registration() {
    let rig = Rig::new();
    let path = rig.scratch.path().join("token");
    let slot = slot_spec(path.clone());
    std::fs::remove_file(&path).unwrap();
    for _ in 0..2 {
        let (stop, receiver) = stop_channel();
        rig.clock.stop_after(rig.clock.sleeps().len() + 1, stop);
        run_slot(
            &slot,
            &rig.backend,
            &rig.clock,
            &rig.registrar,
            &rig.log,
            receiver,
        )
        .await;
        write_token_file(&path, b"  \r\n");
    }
    let warnings = rig.warnings();
    assert!(
        warnings[0].contains("failed to request a JIT registration (token file not found at "),
        "{warnings:?}"
    );
    assert!(
        warnings[1].contains("token file at ") && warnings[1].contains(" is empty)"),
        "{warnings:?}"
    );
    assert_eq!(rig.seconds(), [5, 5]);
    assert!(rig.registrar.seen.borrow().is_empty());
    assert!(
        rig.backend
            .calls()
            .iter()
            .all(|c| !matches!(c, Call::RunContainer(_)))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_token_is_read_again_at_every_registration() {
    let rig = Rig::new();
    let path = rig.scratch.path().join("token");
    {
        let mut state = rig.backend.state.borrow_mut();
        state.exits.push_back(Ok(0));
        state.exits.push_back(Ok(0));
        state.on_run = Some(Box::new(move || {
            let _ = std::fs::write(&path, "ghp_rotated\n");
        }));
    }
    rig.run().await;
    let tokens: Vec<String> = rig
        .registrar
        .seen
        .borrow()
        .iter()
        .map(|s| s.token.clone())
        .collect();
    assert_eq!(tokens, [TOKEN, "ghp_rotated", "ghp_rotated"]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_container_runs_under_the_run_contract_with_the_jit_config_and_a_utc_name() {
    let rig = Rig::new();
    rig.backend
        .state
        .borrow_mut()
        .cpusets
        .push_back(Ok("6,11".to_string()));
    rig.registrar
        .results
        .borrow_mut()
        .push_back(Ok("the-jit-config".to_string()));
    rig.backend.state.borrow_mut().exits.push_back(Ok(0));
    rig.run().await;

    let calls = rig.backend.calls();
    let Call::SelectCpuset(request) = &calls[0] else {
        panic!("{calls:?}");
    };
    assert_eq!(request.probe_name, "example-repo-one-cpu-probe-2");
    assert_eq!((request.count, request.offset), (2, 2));
    assert_eq!(request.image, "actions-runner:local");
    let spec = calls
        .iter()
        .find_map(|call| match call {
            Call::RunContainer(spec) => Some(spec.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        spec,
        ContainerSpec {
            name: "example-repo-one-2-20240305060708009".to_string(),
            cpus: "1.5".to_string(),
            cpuset: "6,11".to_string(),
            memory_gb: 16,
            mounts: vec![Mount {
                volume: "example-repo-one-npm".to_string(),
                target: "/home/runner/.npm".to_string(),
            }],
            image: "actions-runner:local".to_string(),
            run_command: "/home/runner/run.sh".to_string(),
        }
    );
    let seen = &rig.registrar.seen.borrow()[0];
    assert_eq!(
        seen.name, spec.name,
        "the runner and the container share one name"
    );
    assert_eq!(
        (seen.owner.as_str(), seen.repository.as_str()),
        ("example-owner", "example-repo-one")
    );
    assert_eq!(seen.labels, ["self-hosted", "linux", "x64", "axpc"]);
    assert_eq!(rig.backend.state.borrow().jit_values[0], "the-jit-config");
    let text = rig.log.lines().join("\n");
    assert!(!text.contains("the-jit-config") && !text.contains(TOKEN));
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_that_is_already_asked_for_does_nothing() {
    let rig = Rig::new();
    let (stop, receiver) = stop_channel();
    stop.send(true).unwrap();
    let slot = slot_spec(rig.scratch.path().join("token"));
    run_slot(
        &slot,
        &rig.backend,
        &rig.clock,
        &rig.registrar,
        &rig.log,
        receiver,
    )
    .await;
    assert!(rig.backend.calls().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_asked_for_as_registration_completes_starts_no_container() {
    // select! picks among ready branches at random, so one run could pass by luck.
    for _ in 0..32 {
        let rig = Rig::new();
        let (stop, receiver) = stop_channel();
        *rig.registrar.stop_during_request.borrow_mut() = Some(stop);
        let slot = slot_spec(rig.scratch.path().join("token"));
        run_slot(
            &slot,
            &rig.backend,
            &rig.clock,
            &rig.registrar,
            &rig.log,
            receiver,
        )
        .await;
        assert_eq!(rig.registrar.seen.borrow().len(), 1);
        assert!(
            !rig.backend
                .calls()
                .iter()
                .any(|call| matches!(call, Call::RunContainer(_)))
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_ends_a_backoff_wait_at_once() {
    let rig = Rig::new();
    rig.backend
        .state
        .borrow_mut()
        .cpusets
        .push_back(Err(BackendError::new("probe failed")));
    let (stop, receiver) = stop_channel();
    let slot = slot_spec(rig.scratch.path().join("token"));
    let started = std::time::Instant::now();
    let worker = run_slot(
        &slot,
        &rig.backend,
        &SystemClock,
        &rig.registrar,
        &rig.log,
        receiver,
    );
    let asker = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stop.send(true).unwrap();
    };
    tokio::join!(worker, asker);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the 5 s wait was not cut short"
    );
}
