//! the slot's loop on the Tart backend, with the fake `tart`, `softnet` and `docker`
//! as processes: the order of a cycle, what each failure does to the VM and to the
//! registration, and what a drain for an update does to a Tart runner, read from the
//! guest runner's recorded lines.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::time::{Duration, Instant};

use marshal_cli::instance::lifecycle::StopTimes;
use marshal_cli::instance::steps::{Steps, container_steps};
use marshal_cli::runtime::backend::Backend;
use marshal_cli::runtime::board::Board;
use marshal_cli::runtime::clock::Clock;
use marshal_cli::runtime::fake::FakeClock;
use marshal_cli::runtime::job_marks::Activity;
use marshal_cli::runtime::name::job_container_name;
use marshal_cli::runtime::pool::SlotSpec;
use marshal_cli::runtime::registration::{JitRequest, Registrar, RegistrationError};
use marshal_cli::runtime::secret::Secret;
use marshal_cli::runtime::token::TokenFiles;
use marshal_cli::runtime::worker::{Services, run_slot, run_slot_with, stop_channel};
use support::rig::{POOL, Rig, SLOT, START_MILLIS, TestClock, golden_name, job_vm};
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

const LISTENING: &str = "stdout\t2026-11-12 08:00:01Z: Listening for Jobs";
const RUNNING_JOB: &str = "stderr\t2026-11-12 08:03:00Z: Running job: build";

fn job_container() -> String {
    job_container_name(POOL, SLOT, TestClock.now())
}

async fn until(what: &str, condition: impl Fn() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "never saw {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// runs the slot with `drain` and `stop` beside `script`, and returns how many
/// registrations it asked for.
async fn drained_run(
    rig: &Rig,
    drain: &watch::Sender<bool>,
    stop: &watch::Sender<bool>,
    times: StopTimes,
    script: impl Future<Output = ()>,
) -> usize {
    let scratch = TempDir::new();
    let slot = vm_slot(&scratch);
    let recorder = Recorder {
        rig,
        stop: stop.clone(),
        stop_on: usize::MAX,
        requests: Cell::new(0),
        seen: RefCell::new(Vec::new()),
    };
    let board = Board::default();
    let services = Services {
        backend: &rig.backend,
        clock: &TestClock,
        registrar: &recorder,
        tokens: &TokenFiles,
        log: &*rig.log,
        board: &board,
        drain: Some(drain),
        stop_times: times,
    };
    let both = async { tokio::join!(run_slot_with(&slot, &services, stop.subscribe()), script) };
    tokio::time::timeout(Duration::from_secs(60), both)
        .await
        .expect("the slot and the script did not both end in a minute");
    recorder.requests.get()
}

/// waits until the slot's runner runs and its marks read busy, then begins the drain.
async fn busy_in_a_drain(rig: &Rig, drain: &watch::Sender<bool>) {
    let name = job_container();
    until("the runner running", || rig.containers().contains(&name)).await;
    until("the busy mark", || {
        rig.backend.job_activity(&name) == Activity::Busy
    })
    .await;
    drain.send_replace(true);
}

fn position(calls: &[Vec<String>], wanted: &[&str]) -> usize {
    calls
        .iter()
        .position(|call| *call == wanted)
        .unwrap_or_else(|| panic!("no {wanted:?} in {calls:?}"))
}

/// where `tart stop` of `vm` came, whatever its timeout.
fn vm_stopped(calls: &[Vec<String>], vm: &str) -> usize {
    calls
        .iter()
        .position(|call| call.len() > 1 && call[0] == "stop" && call[1] == vm)
        .unwrap_or_else(|| panic!("no stop of {vm} in {calls:?}"))
}

fn ended_in_the_vm(rig: &Rig) -> bool {
    rig.docker_calls().iter().any(|call| {
        matches!(
            call.arguments.first().map(String::as_str),
            Some("stop" | "kill" | "rm")
        )
    })
}

#[tokio::test(flavor = "current_thread")]
async fn in_a_drain_an_idle_runners_container_is_stopped_its_vm_stopped_and_deleted_and_nothing_registers()
 {
    let rig = Rig::new();
    rig.docker_script("run_mode", "hold");
    rig.docker_script("run_says", LISTENING);
    let (drain, _) = watch::channel(false);
    let (stop, _) = stop_channel();
    let name = job_container();
    let times = StopTimes::default();
    let script = async {
        until("the runner running", || rig.containers().contains(&name)).await;
        drain.send_replace(true);
        until("the job VM deleted", || {
            rig.fake.vm_names() == [golden_name()]
        })
        .await;
        // long enough for a slot that registers again to clone its next VM
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(
            rig.tart_calls_of("clone").len(),
            1,
            "a VM prepared in the drain"
        );
        stop.send_replace(true);
    };
    let registrations = drained_run(&rig, &drain, &stop, times, script).await;
    assert_eq!(registrations, 1, "a registration during the drain");
    let calls = rig.tart_calls();
    let vm = job_vm();
    let grace = times.grace.as_secs().to_string();
    let stopped = position(
        &calls,
        &["exec", &vm, "docker", "stop", "--time", &grace, &name],
    );
    let vm_stopped = vm_stopped(&calls, &vm);
    let deleted = position(&calls, &["delete", &vm]);
    assert!(stopped < vm_stopped && vm_stopped < deleted, "{calls:?}");
    assert!(rig.backend.prepared_hosts().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn in_a_drain_a_busy_runner_keeps_its_vm_until_its_guest_exits_and_nothing_registers() {
    let rig = Rig::new();
    rig.docker_script("run_mode", "hold");
    rig.docker_script("run_says", &format!("{LISTENING}\n{RUNNING_JOB}"));
    let (drain, _) = watch::channel(false);
    let (stop, _) = stop_channel();
    let vm = job_vm();
    let script = async {
        busy_in_a_drain(&rig, &drain).await;
        // several of the drain's looks at the marks
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(
            rig.containers().contains(&job_container()),
            "the busy job ended"
        );
        assert!(rig.fake.vm_names().contains(&vm), "the busy job's VM went");
        assert!(!ended_in_the_vm(&rig), "{:?}", rig.docker_calls());
        // the job ends in the guest: the held run exits once its container is gone
        rig.docker_script("containers", "");
        until("the job VM deleted", || {
            rig.fake.vm_names() == [golden_name()]
        })
        .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(
            rig.tart_calls_of("clone").len(),
            1,
            "a VM prepared in the drain"
        );
        stop.send_replace(true);
    };
    let registrations = drained_run(&rig, &drain, &stop, StopTimes::default(), script).await;
    assert_eq!(registrations, 1, "a registration during the drain");
    assert!(!ended_in_the_vm(&rig), "{:?}", rig.docker_calls());
    let calls = rig.tart_calls();
    assert!(vm_stopped(&calls, &vm) < position(&calls, &["delete", &vm]));
}

#[tokio::test(flavor = "current_thread")]
async fn the_stop_at_a_drains_bound_stops_a_busy_runner_in_its_vm_and_deletes_the_vm() {
    let rig = Rig::new();
    rig.docker_script("run_mode", "hold");
    rig.docker_script("run_says", &format!("{LISTENING}\n{RUNNING_JOB}"));
    let (drain, _) = watch::channel(false);
    let (stop, _) = stop_channel();
    let times = StopTimes::reserving(rig.backend.release_budget());
    let (force, _) = watch::channel(false);
    let removed = Cell::new(true);
    let script = async {
        busy_in_a_drain(&rig, &drain).await;
        // the agent's stop at the bound: the running containers, then the stop, then
        // the container steps
        let containers = rig.backend.running_containers();
        stop.send_replace(true);
        let steps = Steps {
            backend: &rig.backend,
            log: &*rig.log,
            times,
            force: &force,
            containers_removed: &removed,
            left: None,
        };
        container_steps(&steps, containers).await;
    };
    let registrations = drained_run(&rig, &drain, &stop, times, script).await;
    assert_eq!(registrations, 1);
    assert!(removed.get(), "{:?}", rig.log.lines());
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(rig.backend.prepared_hosts().is_empty());
    let calls = rig.tart_calls();
    let vm = job_vm();
    let name = job_container();
    let grace = times.grace.as_secs().to_string();
    let stopped = position(
        &calls,
        &["exec", &vm, "docker", "stop", "--time", &grace, &name],
    );
    assert!(stopped < position(&calls, &["delete", &vm]), "{calls:?}");
}
