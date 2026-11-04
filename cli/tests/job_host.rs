//! the per-job host steps of the slot loop: the host is prepared after the image check
//! and released on every path out of its preparation, before any backoff, on the fake
//! backend and a clock that does not wait.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::Duration;

use marshal_cli::runtime::backend::{
    BackendError, DiagnosticRecord, HostPreparation, HostRequest, JobHost, RELEASE_WITHIN,
};
use marshal_cli::runtime::board::Board;
use marshal_cli::runtime::fake::{
    Call, FakeBackend, FakeClock, FakeRegistrar, HostCall, MemoryLog,
};
use marshal_cli::runtime::pool::SlotSpec;
use marshal_cli::runtime::registration::RegistrationError;
use marshal_cli::runtime::worker::{run_slot, stop_channel};
use support::{TempDir, slot_spec};

const START: u64 = 1_709_618_828_009;

struct Rig {
    scratch: TempDir,
    backend: FakeBackend,
    registrar: FakeRegistrar,
    clock: FakeClock,
    log: MemoryLog,
    board: Board,
}

impl Rig {
    fn new() -> Self {
        let scratch = TempDir::new();
        scratch.write_token("token", b"ghp_hosttoken\n");
        let mut backend = FakeBackend::default();
        let mut clock = FakeClock::at(START);
        let registrar = FakeRegistrar::default();
        backend.timeline = registrar.timeline.clone();
        clock.timeline = registrar.timeline.clone();
        Self {
            scratch,
            backend,
            registrar,
            clock,
            log: MemoryLog::default(),
            board: Board::default(),
        }
    }

    fn slot(&self) -> SlotSpec {
        slot_spec(self.scratch.path().join("token"))
    }

    /// runs the slot until the scripted exits run out, which asks for the stop.
    async fn run(&self) {
        self.run_slot(&self.slot()).await;
    }

    async fn run_slot(&self, slot: &SlotSpec) {
        let (stop, receiver) = stop_channel();
        self.backend.state.borrow_mut().stop_when_exits_run_out = Some(stop);
        self.run_until_stopped(slot, receiver).await;
    }

    async fn run_until_stopped(&self, slot: &SlotSpec, stop: tokio::sync::watch::Receiver<bool>) {
        run_slot(
            slot,
            &self.backend,
            &self.clock,
            &self.registrar,
            &self.log,
            &self.board,
            stop,
        )
        .await;
    }

    fn steps(&self) -> Vec<&'static str> {
        self.registrar.timeline.steps()
    }

    fn warnings(&self) -> Vec<String> {
        self.log
            .lines()
            .into_iter()
            .filter(|line| line.starts_with("WARN: "))
            .collect()
    }

    fn host_calls(&self) -> Vec<HostCall> {
        self.backend.state.borrow().host_calls.clone()
    }
}

fn host() -> JobHost {
    JobHost {
        pool: "example-repo-one".to_string(),
        slot: 2,
    }
}

/// the steps before the slot's first backoff, which is where a failed attempt ends.
fn before_the_first_backoff(steps: &[&'static str]) -> Vec<&'static str> {
    let end = steps.iter().position(|step| *step == "sleep").unwrap();
    steps[..end].to_vec()
}

fn count(steps: &[&str], wanted: &str) -> usize {
    steps.iter().filter(|step| **step == wanted).count()
}

#[tokio::test(flavor = "current_thread")]
async fn a_slot_cycle_checks_the_image_then_prepares_probes_registers_runs_and_releases() {
    let rig = Rig::new();
    rig.backend.state.borrow_mut().exits.push_back(Ok(0));
    rig.run().await;
    let steps = rig.steps();
    assert_eq!(
        steps[..6],
        [
            "check_image",
            "prepare",
            "probe",
            "register",
            "run",
            "release"
        ]
    );
    assert_eq!(
        rig.host_calls(),
        [
            HostCall::Prepare(HostRequest {
                host: host(),
                vcpus: 2,
                memory_gb: 16,
                disk_gb: None,
                diagnostic: false,
            }),
            HostCall::Release(host(), RELEASE_WITHIN),
            HostCall::Prepare(HostRequest {
                host: host(),
                vcpus: 2,
                memory_gb: 16,
                disk_gb: None,
                diagnostic: false,
            }),
            HostCall::Release(host(), Duration::ZERO),
        ],
        "the release after the stop gets the backend's release budget"
    );
    assert!(rig.backend.state.borrow().hosts.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_diagnostic_run_is_exported_before_its_host_is_released() {
    let rig = Rig::new();
    rig.backend.state.borrow_mut().exits.push_back(Ok(0));
    let mut slot = rig.slot();
    slot.diagnostic = Some(DiagnosticRecord {
        pool: "example-repo-one".to_string(),
        directory: "/evidence".to_string(),
        raw_records: false,
    });
    rig.run_slot(&slot).await;
    assert_eq!(
        rig.steps()[..8],
        [
            "check_image",
            "prepare",
            "probe",
            "register",
            "run",
            "export",
            "remove",
            "release"
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_preparation_releases_the_host_once_before_the_backoff_and_registers_nothing() {
    let rig = Rig::new();
    rig.backend
        .state
        .borrow_mut()
        .preparations
        .push_back(HostPreparation::Failed(BackendError::new("clone failed")));
    rig.run().await;
    let steps = rig.steps();
    assert_eq!(
        before_the_first_backoff(&steps),
        ["check_image", "prepare", "release"]
    );
    assert_eq!(
        rig.warnings()[0],
        "WARN: example-repo-one slot 2: the job host could not be prepared (clone failed); no registration - retrying in 5s."
    );
    assert_eq!(
        count(&steps, "prepare"),
        count(&steps, "release"),
        "every preparation is released"
    );
    assert_eq!(
        rig.registrar.seen.borrow().len(),
        1,
        "only the retry registers"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_probe_releases_the_host_once_before_the_backoff_and_registers_nothing() {
    let rig = Rig::new();
    rig.backend
        .state
        .borrow_mut()
        .cpusets
        .push_back(Err(BackendError::new("probe failed")));
    rig.run().await;
    let steps = rig.steps();
    assert_eq!(
        before_the_first_backoff(&steps),
        ["check_image", "prepare", "probe", "release"]
    );
    assert_eq!(count(&steps, "prepare"), count(&steps, "release"));
    assert_eq!(
        rig.registrar.seen.borrow().len(),
        1,
        "only the retry registers"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_registration_releases_the_host_once_before_the_backoff() {
    let rig = Rig::new();
    rig.registrar
        .results
        .borrow_mut()
        .push_back(Err(RegistrationError::new("403 (Forbidden)")));
    rig.run().await;
    let steps = rig.steps();
    assert_eq!(
        before_the_first_backoff(&steps),
        ["check_image", "prepare", "probe", "register", "release"]
    );
    assert_eq!(count(&steps, "prepare"), count(&steps, "release"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_run_releases_the_host_once_before_the_backoff() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        state
            .exits
            .push_back(Err(BackendError::new("could not start")));
        state.exits.push_back(Ok(1));
    }
    rig.run().await;
    let steps = rig.steps();
    assert_eq!(
        before_the_first_backoff(&steps),
        [
            "check_image",
            "prepare",
            "probe",
            "register",
            "run",
            "release"
        ]
    );
    assert_eq!(count(&steps, "prepare"), count(&steps, "release"));
    assert_eq!(
        rig.warnings()[1],
        "WARN: example-repo-one slot 2: container exited with code 1 - retrying in 10s."
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_held_preparation_boots_nothing_releases_once_and_shows_the_reason_on_the_board() {
    let rig = Rig::new();
    rig.backend
        .state
        .borrow_mut()
        .preparations
        .push_back(HostPreparation::Held("Softnet is not set up".to_string()));
    let (stop, receiver) = stop_channel();
    rig.clock.stop_after(1, stop);
    rig.run_until_stopped(&rig.slot(), receiver).await;
    assert_eq!(rig.steps(), ["check_image", "prepare", "release", "sleep"]);
    assert_eq!(
        rig.warnings(),
        [
            "WARN: example-repo-one slot 2: held, nothing is started (Softnet is not set up) - retrying in 5s."
        ]
    );
    assert_eq!(
        rig.board.fields(),
        [(
            "slot",
            "example-repo-one slot 2: held: Softnet is not set up".to_string()
        )]
    );
    assert!(rig.registrar.seen.borrow().is_empty());
    assert!(
        !rig.backend
            .calls()
            .iter()
            .any(|call| matches!(call, Call::SelectCpuset(_) | Call::RunContainer(_)))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_held_slot_retries_on_the_backoff_schedule_and_runs_once_the_check_passes() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        for _ in 0..3 {
            state
                .preparations
                .push_back(HostPreparation::Held("Softnet is not set up".to_string()));
        }
        state
            .preparations
            .push_back(HostPreparation::Failed(BackendError::new("clone failed")));
        state.exits.push_back(Ok(0));
    }
    rig.run().await;
    let seconds: Vec<u64> = rig.clock.sleeps().iter().map(Duration::as_secs).collect();
    assert_eq!(
        seconds,
        [5, 10, 20, 40],
        "held and failed attempts share one schedule"
    );
    let steps = rig.steps();
    assert_eq!(count(&steps, "prepare"), 6);
    assert_eq!(count(&steps, "release"), 6);
    assert_eq!(
        count(&steps, "run"),
        2,
        "the first run ends 0, and the stop ends the second"
    );
    let fields = rig.board.fields();
    assert!(
        fields[0]
            .1
            .starts_with("example-repo-one slot 2: running container example-repo-one-2-"),
        "the slot leaves the held state once the host is ready: {fields:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_during_the_preparation_releases_the_host_and_ends_the_slot() {
    let rig = Rig::new();
    let (stop, receiver) = stop_channel();
    rig.backend.state.borrow_mut().stop_in_prepare = Some(stop);
    rig.run_until_stopped(&rig.slot(), receiver).await;
    assert_eq!(rig.steps(), ["check_image", "prepare", "release"]);
    assert_eq!(
        rig.host_calls()[1],
        HostCall::Release(host(), Duration::ZERO),
        "a stop gives the release the backend's budget for it"
    );
    assert!(rig.registrar.seen.borrow().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_during_the_registration_releases_the_host_and_starts_no_container() {
    for _ in 0..16 {
        let rig = Rig::new();
        let (stop, receiver) = stop_channel();
        *rig.registrar.stop_during_request.borrow_mut() = Some(stop);
        rig.run_until_stopped(&rig.slot(), receiver).await;
        assert_eq!(
            rig.steps(),
            ["check_image", "prepare", "probe", "register", "release"]
        );
        assert!(rig.backend.state.borrow().hosts.is_empty());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_release_is_a_warning_and_changes_neither_the_backoff_nor_the_next_attempt() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        state.release_error = Some(BackendError::new("delete failed"));
        state.exits.push_back(Ok(0));
    }
    rig.run().await;
    assert!(rig.clock.sleeps().is_empty());
    assert_eq!(
        rig.warnings()[0],
        "WARN: example-repo-one slot 2: releasing the job host failed (delete failed)."
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_job_container_runs_the_command_the_backend_provides() {
    let mut rig = Rig::new();
    rig.backend.run_command = Some("/opt/runner/run.sh");
    rig.backend.state.borrow_mut().exits.push_back(Ok(0));
    rig.run().await;
    let command = rig
        .backend
        .calls()
        .into_iter()
        .find_map(|call| match call {
            Call::RunContainer(spec) => Some(spec.run_command),
            _ => None,
        })
        .unwrap();
    assert_eq!(command, "/opt/runner/run.sh");
}
