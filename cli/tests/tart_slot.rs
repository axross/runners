//! the slot's loop on the Tart backend, with the fake `tart`, `softnet` and `docker`
//! as processes: the order of a cycle, and what each failure does to the VM and to
//! the registration.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::cell::{Cell, RefCell};
use std::time::Duration;

use marshal_cli::runtime::backend::Backend;
use marshal_cli::runtime::board::Board;
use marshal_cli::runtime::fake::FakeClock;
use marshal_cli::runtime::pool::SlotSpec;
use marshal_cli::runtime::registration::{JitRequest, Registrar, RegistrationError};
use marshal_cli::runtime::secret::Secret;
use marshal_cli::runtime::worker::{run_slot, stop_channel};
use support::rig::{Rig, START_MILLIS, golden_name, job_vm};
use support::{TempDir, slot_spec};
use tokio::sync::watch;

/// answers every request, and asks for the stop at the `stop_on`th, and keeps what
/// the fake `tart` had been called with when each request came.
struct Recorder<'a> {
    rig: &'a Rig,
    stop: watch::Sender<bool>,
    stop_on: usize,
    requests: Cell<usize>,
    seen: RefCell<Vec<Vec<Vec<String>>>>,
}

impl Registrar for Recorder<'_> {
    async fn request_jit(&self, _request: &JitRequest<'_>) -> Result<Secret, RegistrationError> {
        self.requests.set(self.requests.get() + 1);
        self.seen.borrow_mut().push(self.rig.tart_calls());
        if self.requests.get() >= self.stop_on {
            let _ = self.stop.send(true);
        }
        Ok(Secret::new("jit-0123456789".to_string()))
    }
}

fn vm_slot(scratch: &TempDir) -> SlotSpec {
    let mut slot = slot_spec(scratch.write_token("token", b"ghp_slottoken\n"));
    slot.cpus = "4".to_string();
    slot.cpu_count = 4;
    slot.cpu_offset = 0;
    slot.memory_gb = 16;
    slot.disk_gb = Some(60);
    slot.mounts = Vec::new();
    slot
}

struct Run {
    registrar_requests: usize,
    seen: Vec<Vec<Vec<String>>>,
    sleeps: Vec<Duration>,
}

/// runs the slot until it has registered `stop_on` times or backed off once.
async fn run(rig: &Rig, stop_on: usize) -> Run {
    let scratch = TempDir::new();
    let slot = vm_slot(&scratch);
    let (stop, receiver) = stop_channel();
    let clock = FakeClock::at(START_MILLIS);
    clock.stop_after(1, stop.clone());
    let recorder = Recorder {
        rig,
        stop,
        stop_on,
        requests: Cell::new(0),
        seen: RefCell::new(Vec::new()),
    };
    let board = Board::default();
    run_slot(
        &slot,
        &rig.backend,
        &clock,
        &recorder,
        &*rig.log,
        &board,
        receiver,
    )
    .await;
    Run {
        registrar_requests: recorder.requests.get(),
        seen: recorder.seen.into_inner(),
        sleeps: clock.sleeps(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_cycle_boots_a_vm_probes_it_registers_runs_and_deletes_the_vm_before_the_next_cycle() {
    let rig = Rig::new();
    let ran = run(&rig, 2).await;

    assert_eq!(ran.registrar_requests, 2);
    let at_registration = &ran.seen[0];
    let position = |subcommand: &str| {
        at_registration
            .iter()
            .position(|call| call.first().map(String::as_str) == Some(subcommand))
            .unwrap_or_else(|| {
                panic!("{subcommand} came before the registration: {at_registration:?}")
            })
    };
    assert!(position("--version") < position("clone"));
    assert!(position("clone") < position("set"));
    assert!(position("set") < position("run"));
    assert!(
        at_registration
            .iter()
            .any(|call| call.contains(&"--network".to_string())),
        "the CPU placement was probed before the registration"
    );
    assert!(
        !at_registration
            .iter()
            .any(|call| call.contains(&"marshal-launch".to_string())
                || call.contains(&"-i".to_string())),
        "nothing ran in the VM before the registration"
    );

    let calls = rig.tart_calls();
    let run_in_vm = calls
        .iter()
        .position(|call| call.contains(&"/usr/local/bin/marshal-launch".to_string()))
        .expect("the runner container started through the launcher");
    let first_delete = calls
        .iter()
        .position(|call| call.first().map(String::as_str) == Some("delete"))
        .unwrap();
    let second_clone = calls
        .iter()
        .rposition(|call| call.first().map(String::as_str) == Some("clone"))
        .unwrap();
    assert!(run_in_vm < first_delete, "the VM is deleted after the job");
    assert!(
        first_delete < second_clone,
        "and before the next cycle's VM"
    );
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(rig.backend.prepared_hosts().is_empty());
    assert!(
        rig.log
            .lines()
            .iter()
            .any(|line| line.contains("starting container example-repo-one-2-20240305060708009")),
        "{:?}",
        rig.log.lines()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_probe_makes_no_registration_and_the_vm_is_deleted_before_the_backoff() {
    let rig = Rig::new();
    rig.docker_script("probe_mode", "fail");
    let ran = run(&rig, 1).await;
    assert_eq!(ran.registrar_requests, 0);
    assert_eq!(ran.sleeps, [Duration::from_secs(5)], "today's backoff");
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(
        rig.log
            .lines()
            .iter()
            .any(|line| line.contains("CPU discovery or probe cleanup failed; no registration")),
        "{:?}",
        rig.log.lines()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failure_at_the_clone_the_resize_or_the_boot_backs_off_with_the_vm_deleted_and_no_registration()
 {
    for (script, content) in [
        ("fail-clone", "Error: scripted"),
        ("fail-set", "Error: scripted"),
        ("run_mode", "exit"),
    ] {
        let rig = Rig::new();
        rig.fake.script(script, content);
        let ran = run(&rig, 1).await;
        assert_eq!(ran.registrar_requests, 0, "{script}");
        assert_eq!(ran.sleeps, [Duration::from_secs(5)], "{script}");
        assert_eq!(rig.fake.vm_names(), [golden_name()], "{script}");
        assert!(rig.backend.prepared_hosts().is_empty(), "{script}");
        assert!(
            rig.log
                .lines()
                .iter()
                .any(|line| line.contains("the job host could not be prepared")
                    && line.contains("no registration")),
            "{script}: {:?}",
            rig.log.lines()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_boot_failure_is_logged_with_tarts_error_and_the_signed_in_section() {
    let rig = Rig::new();
    rig.fake.script("run_mode", "exit");
    rig.fake
        .script("run_error", "Error: the login keychain is locked");
    run(&rig, 1).await;
    let warnings =
        rig.log_lines("WARN: example-repo-one slot 2: the job host could not be prepared");
    assert_eq!(warnings.len(), 1, "{:?}", rig.log.lines());
    assert!(
        warnings[0].contains("Error: the login keychain is locked")
            && warnings[0].contains(
                "see the \"The Signed-In Session\" section of the macOS Runner Host document"
            ),
        "{}",
        warnings[0]
    );
    assert!(warnings[0].contains(&job_vm()));
}

#[tokio::test(flavor = "current_thread")]
async fn a_prerequisite_refusal_holds_the_slot_and_nothing_is_cloned() {
    let rig = Rig::new();
    rig.fake.script("version_output", "2.40.0\n");
    let ran = run(&rig, 1).await;
    assert_eq!(ran.registrar_requests, 0);
    assert!(rig.tart_calls_of("clone").is_empty());
    assert!(
        rig.log.lines().iter().any(|line| line.starts_with(
            "WARN: example-repo-one slot 2: held, nothing is started (Tart 2.40.0 is not"
        )),
        "{:?}",
        rig.log.lines()
    );
}
