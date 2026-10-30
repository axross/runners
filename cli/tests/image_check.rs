//! the image check on the fake seams with an injected clock, through the container
//! backend against the fake `docker`, and through `start` as a real process.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::time::{Duration, Instant};

use marshal_cli::image::check::{ImageCheck, ImageLabels};
use marshal_cli::image::contract::CONTRACT_VERSION;
use marshal_cli::instance::fake::{CoreStep, FakeCore};
use marshal_cli::instance::lifecycle::{Core, Ending};
use marshal_cli::instance::protocol::{Outcome, Request};
use marshal_cli::instance::runtime_core::RuntimeCore;
use marshal_cli::profile::Profile;
use marshal_cli::runtime::backend::Backend;
use marshal_cli::runtime::board::Board;
use marshal_cli::runtime::docker::DockerLimits;
use marshal_cli::runtime::fake::{Call, FakeBackend, FakeClock, FakeRegistrar, MemoryLog};
use marshal_cli::runtime::worker::{run_slot, stop_channel};
use marshal_cli::validate::{Plan, validate};
use support::docker::Fake;
use support::host::{Host, registered, texts};
use support::instance::{CONFIG_PATH, Rig};
use support::stub::Stub;
use support::{TempDir, slot_spec};

const START: u64 = 1_709_618_828_009;
const START_SECONDS: u64 = START / 1000;
const DAY: u64 = 86_400;

fn labels(contract: &str, built_days_ago: Option<u64>, released: &str) -> ImageLabels {
    ImageLabels {
        contract: Some(contract.to_string()),
        built: built_days_ago.map(|days| (START_SECONDS - days * DAY).to_string()),
        runner_version: Some("2.337.0".to_string()),
        runner_released: Some(released.to_string()),
    }
}

fn mismatched() -> ImageCheck {
    ImageCheck::Mismatched(labels("0", Some(0), "2024-03-05"))
}

/// one pool, `r`, whose token file is `token` in `scratch`.
fn plan(scratch: &TempDir) -> Plan {
    let config = r#"{"imageName":"actions-runner:local","repositories":[{"owner":"o","repository":"r",
        "name":"r","slots":1,"tokenPath":"C:\\r.token","volumes":[]}]}"#;
    let mut plan = validate(config.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    plan.entries[0].token_path = scratch.path().join("token").to_str().unwrap().to_string();
    plan
}

struct CoreRig {
    scratch: TempDir,
    backend: FakeBackend,
    clock: FakeClock,
    registrar: FakeRegistrar,
    log: MemoryLog,
}

impl CoreRig {
    fn new() -> Self {
        let scratch = TempDir::new();
        scratch.write_token("token", b"ghp_imagetoken\n");
        Self {
            scratch,
            backend: FakeBackend::default(),
            clock: FakeClock::at(START),
            registrar: FakeRegistrar::default(),
            log: MemoryLog::default(),
        }
    }

    fn answer(&self, checks: impl IntoIterator<Item = ImageCheck>) {
        self.backend.state.borrow_mut().image_answers.extend(checks);
    }

    fn core(&self) -> RuntimeCore<'_, FakeBackend, FakeClock, FakeRegistrar> {
        RuntimeCore::new(&self.backend, &self.clock, &self.registrar, &self.log)
    }

    /// runs the core until its slot's scripted exits run out, or it fails.
    async fn run(
        &self,
        core: &RuntimeCore<'_, FakeBackend, FakeClock, FakeRegistrar>,
    ) -> Result<(), marshal_cli::instance::lifecycle::CoreFailure> {
        let (stop, receiver) = stop_channel();
        self.backend.state.borrow_mut().stop_when_exits_run_out = Some(stop);
        core.run(&plan(&self.scratch), receiver).await
    }

    fn image_checks(&self) -> usize {
        self.backend.state.borrow().image_checks.len()
    }

    fn freshness_warnings(&self) -> usize {
        self.log
            .lines()
            .iter()
            .filter(|line| line.starts_with("WARN: Image freshness: "))
            .count()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_mismatched_or_missing_image_fails_the_first_run_before_any_pool_is_prepared() {
    for check in [mismatched(), ImageCheck::Missing] {
        let rig = CoreRig::new();
        rig.answer([check]);
        let core = rig.core();
        let failure = rig.run(&core).await.unwrap_err();
        assert_eq!(failure.ending, Ending::ImageRefused);
        assert!(
            failure
                .message
                .starts_with("the image actions-runner:local "),
            "{}",
            failure.message
        );
        assert_eq!(rig.backend.calls(), [Call::WaitUntilAvailable]);
        assert!(rig.registrar.seen.borrow().is_empty());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn docker_unavailable_for_the_check_is_waited_out_and_never_read_as_a_mismatch() {
    let rig = CoreRig::new();
    rig.answer([ImageCheck::Unavailable(
        "Docker did not answer the image check within 4 s".to_string(),
    )]);
    let core = rig.core();
    rig.run(&core).await.unwrap();
    let calls = rig.backend.calls();
    assert_eq!(
        calls[..2],
        [Call::WaitUntilAvailable, Call::WaitUntilAvailable]
    );
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, Call::RunContainer(_)))
    );
    assert_eq!(rig.clock.sleeps()[0], Duration::from_secs(5));
    let text = rig.log.lines().join("\n");
    assert!(text.contains("is checked again in 5 s"), "{text}");
    assert!(!text.contains("image contract"), "{text}");
}

#[tokio::test(flavor = "current_thread")]
async fn the_freshness_warnings_are_logged_at_each_run_of_the_core_and_not_at_job_launch() {
    let rig = CoreRig::new();
    let core = rig.core();
    let old = || {
        ImageCheck::Matching(labels(
            &CONTRACT_VERSION.to_string(),
            Some(10),
            "2024-02-01",
        ))
    };
    for run in 1..=2 {
        rig.answer([old(), old(), old(), old()]);
        {
            let mut state = rig.backend.state.borrow_mut();
            state.exits.push_back(Ok(0));
            state.exits.push_back(Ok(0));
        }
        rig.run(&core).await.unwrap();
        // the run's check, then one per job launch: the two scripted exits and the
        // launch that finds them run out
        assert_eq!(rig.image_checks(), run * 4);
        assert_eq!(rig.freshness_warnings(), run * 2, "{:#?}", rig.log.lines());
    }
    let warnings: Vec<String> = rig
        .log
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("WARN: Image freshness: "))
        .collect();
    assert_eq!(
        warnings[..2],
        [
            "WARN: Image freshness: the image is 10 days old and should be rebuilt with `marshal build`.",
            "WARN: Image freshness: runner 2.337.0 was released 33 days ago, so each job's runner likely updates itself at start; update the CLI to get a newer runner, then rebuild the image.",
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_matching_image_runs_and_status_shows_it() {
    let rig = CoreRig::new();
    let core = rig.core();
    rig.run(&core).await.unwrap();
    assert!(rig.backend.calls().iter().any(
        |call| matches!(call, Call::RunContainer(spec) if spec.image == "actions-runner:local")
    ));
    assert_eq!(
        rig.backend.state.borrow().image_checks[0],
        "actions-runner:local"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_resume_check_refuses_a_mismatch_and_passes_when_docker_cannot_say() {
    let rig = CoreRig::new();
    let core = rig.core();
    let plan = plan(&rig.scratch);
    rig.answer([
        mismatched(),
        ImageCheck::Missing,
        ImageCheck::Unavailable("no answer".to_string()),
    ]);
    let refusal = core.check_image(&plan).await.unwrap_err();
    assert!(
        refusal.contains("has image contract 0, but this CLI needs image contract"),
        "{refusal}"
    );
    let refusal = core.check_image(&plan).await.unwrap_err();
    assert!(refusal.contains("is missing"), "{refusal}");
    assert_eq!(core.check_image(&plan).await, Ok(()));
    assert!(
        rig.log
            .lines()
            .join("\n")
            .contains("checked once Docker answers")
    );
    assert!(
        rig.backend.calls().is_empty(),
        "a resume check does not wait for Docker"
    );
}

#[test]
fn status_shows_the_latest_answered_check_and_not_checked_before_one() {
    let board = Board::default();
    let now = std::time::UNIX_EPOCH + Duration::from_secs(START_SECONDS);
    assert!(board.image_fields(now).is_empty(), "no image is named yet");
    board.expect_image("actions-runner:local");
    let fields = board.image_fields(now);
    assert_eq!(
        fields,
        [
            ("image", "actions-runner:local".to_string()),
            ("image-contract", "not checked".to_string()),
            ("image-built", "unknown".to_string()),
            ("runner", "unknown".to_string()),
        ]
    );
    board.record_image(
        "actions-runner:local",
        &ImageCheck::Unavailable("slow".to_string()),
    );
    assert_eq!(board.image_fields(now), fields);
    board.record_image(
        "actions-runner:local",
        &ImageCheck::Matching(labels(&CONTRACT_VERSION.to_string(), Some(3), "2024-03-01")),
    );
    board.clear();
    assert_eq!(
        board.image_fields(now),
        [
            ("image", "actions-runner:local".to_string()),
            ("image-contract", CONTRACT_VERSION.to_string()),
            ("image-built", "3 days ago".to_string()),
            ("runner", "2.337.0, released 4 days ago".to_string()),
        ]
    );
    board.record_image("actions-runner:local", &mismatched());
    assert_eq!(
        board.image_fields(now)[1],
        (
            "image-contract",
            format!("mismatch (image 0, CLI {CONTRACT_VERSION})")
        )
    );
    board.record_image("actions-runner:local", &ImageCheck::Missing);
    assert_eq!(
        board.image_fields(now)[1],
        ("image-contract", "missing".to_string())
    );
}

mod job_launch {
    use super::*;

    async fn run_with(check: ImageCheck) -> (FakeBackend, FakeRegistrar, FakeClock, MemoryLog) {
        let scratch = TempDir::new();
        scratch.write_token("token", b"ghp_slottoken\n");
        let backend = FakeBackend::default();
        backend.state.borrow_mut().image_answers.push_back(check);
        let (registrar, clock, log) = (
            FakeRegistrar::default(),
            FakeClock::at(START),
            MemoryLog::default(),
        );
        let (stop, receiver) = stop_channel();
        clock.stop_after(1, stop);
        run_slot(
            &slot_spec(scratch.path().join("token")),
            &backend,
            &clock,
            &registrar,
            &log,
            &Board::default(),
            receiver,
        )
        .await;
        (backend, registrar, clock, log)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_mismatched_or_missing_image_registers_nothing_runs_no_probe_and_backs_off() {
        for check in [mismatched(), ImageCheck::Missing] {
            let (backend, registrar, clock, log) = run_with(check).await;
            assert!(backend.calls().is_empty(), "{:?}", backend.calls());
            assert!(registrar.seen.borrow().is_empty());
            assert_eq!(clock.sleeps(), [Duration::from_secs(5)]);
            let warning = log
                .lines()
                .into_iter()
                .find(|line| line.starts_with("WARN: "))
                .unwrap();
            assert!(
                warning
                    .starts_with("WARN: example-repo-one slot 2: the image actions-runner:local "),
                "{warning}"
            );
            assert!(
                warning.ends_with("; no registration - retrying in 5s."),
                "{warning}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn docker_unavailable_backs_off_as_a_docker_failure_not_a_mismatch() {
        let (backend, registrar, clock, log) = run_with(ImageCheck::Unavailable(
            "Docker did not answer the image check within 4 s".to_string(),
        ))
        .await;
        assert!(backend.calls().is_empty());
        assert!(registrar.seen.borrow().is_empty());
        assert_eq!(clock.sleeps(), [Duration::from_secs(5)]);
        assert_eq!(
            log.lines(),
            [
                "WARN: example-repo-one slot 2: the image check failed (Docker did not answer the image check within 4 s); no registration - retrying in 5s."
            ]
        );
    }
}

mod instance {
    use super::*;

    fn refusal() -> String {
        format!(
            "the image actions-runner:local has image contract 0, but this CLI needs image \
             contract {CONTRACT_VERSION}"
        )
    }

    fn stop_build_start() -> String {
        format!(
            "{}; run `marshal stop`, then `marshal build --config {CONFIG_PATH}`, \
             then `marshal start --config {CONFIG_PATH}`",
            refusal()
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_image_at_the_first_run_ends_the_instance_naming_build_then_start() {
        let rig = Rig::new();
        rig.core.push(CoreStep::RefuseImage(refusal()));
        let (ended, ()) = rig.drive(|_control| async {}).await;
        assert_eq!(ended.ending, Ending::ImageRefused);
        assert_eq!(rig.core.runs.get(), 1);
        assert!(rig.clock.sleeps().is_empty(), "it is not restarted");
        assert!(rig.log.lines().contains(&format!(
            "WARN: The instance ends: {}. Run `marshal build --config {CONFIG_PATH}`, \
             then `marshal start --config {CONFIG_PATH}`.",
            refusal()
        )));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_image_at_a_restart_leaves_the_instance_stopped_with_stop_build_start() {
        let rig = Rig::new();
        rig.core.image_checks.borrow_mut().push_back(Err(refusal()));
        let (_, (restart, status)) = rig
            .drive(|control| async move {
                let restart = control.ask(Request::Restart).await;
                let status = control.ask(Request::Status).await;
                control.stop().await;
                (restart, status)
            })
            .await;
        assert_eq!(restart.outcome, Outcome::Refused);
        assert_eq!(
            restart.message,
            format!("the instance stays stopped: {}", stop_build_start())
        );
        assert_eq!(status.field("state"), Some("stopped"));
        assert_eq!(status.field("reason"), Some(stop_build_start().as_str()));
        assert_eq!(rig.core.runs.get(), 1, "the core is not run again");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_image_at_a_resume_keeps_the_instance_stopped_with_stop_build_start() {
        let rig = Rig::new();
        for _ in 0..4 {
            rig.core.push(FakeCore::failure("worker task panicked"));
        }
        rig.core.image_checks.borrow_mut().push_back(Err(refusal()));
        let (_, (resume, status)) = rig
            .drive(|control| async move {
                control.wait_for_state("stopped").await;
                let resume = control
                    .ask(support::instance::resume(Profile::Axpc, CONFIG_PATH))
                    .await;
                let status = control.ask(Request::Status).await;
                control.stop().await;
                (resume, status)
            })
            .await;
        assert_eq!(resume.outcome, Outcome::Refused);
        assert_eq!(status.field("state"), Some("stopped"));
        assert_eq!(status.field("reason"), Some(stop_build_start().as_str()));
        assert_eq!(rig.core.image_checked.get(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refused_image_at_a_core_restart_stops_the_instance_without_counting_a_failure() {
        let rig = Rig::new();
        for _ in 0..3 {
            rig.core.push(FakeCore::failure("worker task panicked"));
        }
        // a fourth failure would reach the restart limit
        rig.core.push(CoreStep::RefuseImage(refusal()));
        let (ended, status) = rig
            .drive(|control| async move {
                control.wait_for_state("stopped").await;
                let status = control.ask(Request::Status).await;
                control.stop().await;
                status
            })
            .await;
        assert_eq!(
            ended.ending,
            Ending::Ordinary,
            "a later run stops the instance, not ends it"
        );
        assert_eq!(status.field("reason"), Some(stop_build_start().as_str()));
        assert_eq!(rig.clock.sleeps(), [10, 30, 60].map(Duration::from_secs));
        assert!(
            rig.log
                .lines()
                .iter()
                .all(|line| !line.contains("The runtime core failed: the image")),
            "the refusal is not reported as a failure"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn docker_unavailable_at_a_resume_answers_running_and_a_later_mismatch_stops_it() {
        let rig = Rig::new();
        // the resume's check could not reach Docker, so it passes; the next run of
        // the core checks once Docker answers, and finds the mismatch
        rig.core.push(CoreStep::RunUntilStop);
        rig.core.push(CoreStep::RefuseImage(refusal()));
        let (_, (restart, status)) = rig
            .drive(|control| async move {
                let restart = control.ask(Request::Restart).await;
                control.wait_for_state("stopped").await;
                let status = control.ask(Request::Status).await;
                control.stop().await;
                (restart, status)
            })
            .await;
        assert_eq!(restart.outcome, Outcome::Ok);
        assert_eq!(restart.field("state"), Some("running"));
        assert_eq!(status.field("reason"), Some(stop_build_start().as_str()));
        assert_eq!(rig.core.image_checked.get(), 1);
    }
}

mod backend {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn a_name_without_a_tag_is_checked_as_latest_and_missing_when_only_other_tags_exist() {
        let fake = Fake::new();
        fake.script(
            "images",
            &format!(
                "runner:other\tsha256:a\tcom.github.axross.runners.contract={CONTRACT_VERSION}\n\
                 localhost:5000/team/runner:v2\tsha256:b\tcom.github.axross.runners.contract={CONTRACT_VERSION}"
            ),
        );
        assert_eq!(
            fake.backend.check_image("runner").await,
            ImageCheck::Missing
        );
        assert_eq!(
            fake.backend.check_image("localhost:5000/team/runner").await,
            ImageCheck::Missing
        );
        let references: Vec<String> = fake
            .invocations()
            .into_iter()
            .map(|call| call.arguments.last().unwrap().clone())
            .collect();
        assert_eq!(
            references,
            [
                "runner:latest",
                "runner:latest",
                "localhost:5000/team/runner:latest",
                "localhost:5000/team/runner:latest"
            ]
        );
        assert!(matches!(
            fake.backend.check_image("runner:other").await,
            ImageCheck::Matching(_)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_labels_are_read_and_sorted() {
        let fake = Fake::new();
        fake.script(
            "images",
            &format!(
                "good:1\tsha256:a\tcom.github.axross.runners.contract={CONTRACT_VERSION}\t\
                 com.github.axross.runners.built=1709000000\t\
                 com.github.axross.runners.runner-version=2.337.0\tcom.github.axross.runners.runner-released=2026-08-26\n\
                 old:1\tsha256:b\tcom.github.axross.runners.contract=0\n\
                 bare:1\tsha256:c"
            ),
        );
        assert_eq!(
            fake.backend.check_image("good:1").await,
            ImageCheck::Matching(ImageLabels {
                contract: Some(CONTRACT_VERSION.to_string()),
                built: Some("1709000000".to_string()),
                runner_version: Some("2.337.0".to_string()),
                runner_released: Some("2026-08-26".to_string()),
            })
        );
        assert!(matches!(
            fake.backend.check_image("old:1").await,
            ImageCheck::Mismatched(_)
        ));
        assert_eq!(
            fake.backend.check_image("bare:1").await,
            ImageCheck::Mismatched(ImageLabels::default())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_stalled_inspect_is_unavailable_within_the_bound_and_never_a_mismatch() {
        let fake = Fake::with_limits(DockerLimits {
            image_check_budget: Duration::from_millis(1500),
            client_termination: Duration::from_millis(500),
            ..DockerLimits::default()
        });
        fake.script("inspect_hangs", "");
        let started = Instant::now();
        let check = fake.backend.check_image("runner:local").await;
        assert!(matches!(check, ImageCheck::Unavailable(_)), "{check:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn the_image_check_is_bounded_at_four_seconds() {
        assert_eq!(
            DockerLimits::default().image_check_budget,
            Duration::from_secs(4)
        );
    }
}

mod process {
    use super::*;

    #[test]
    fn a_mismatched_or_missing_image_makes_start_exit_1_naming_build_then_start() {
        for (images, expected) in [
            (
                "actions-runner:local\tsha256:a\tcom.github.axross.runners.contract=0",
                "has image contract 0, but this CLI needs image contract",
            ),
            ("", "is missing"),
        ] {
            let host = Host::new();
            host.script("images", images);
            let stub = Stub::start(vec![registered()]);
            let finished = host.start(&stub).finish(Duration::from_secs(20));
            assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
            let config = fs::canonicalize(&host.config).unwrap();
            let advice = format!(
                "Run `marshal build --config {0}`, then `marshal start --config {0}`.",
                config.display()
            );
            assert!(finished.stderr.contains(expected), "{}", finished.stderr);
            assert!(finished.stderr.contains(&advice), "{}", finished.stderr);
            assert!(stub.requests().is_empty(), "nothing was registered");
            assert!(
                host.invocations()
                    .iter()
                    .all(|call| call.first().map(String::as_str) != Some("run")),
                "nothing was run"
            );
        }
    }

    #[test]
    fn when_the_inspect_stalls_during_a_resume_start_still_gets_its_answer_within_10_s() {
        let host = Host::new();
        host.script("run_mode", "hold");
        let stub = Stub::start(vec![registered(), registered()]);
        let started = host.start(&stub);
        host.wait_for_status("running container r-1-", Duration::from_secs(20));
        let valid = fs::read(&host.config).unwrap();
        fs::write(&host.config, b"{}").unwrap();
        assert_eq!(host.client(&["restart"]).status.code(), Some(1));
        host.wait_for_status("state: stopped", Duration::from_secs(5));

        fs::write(&host.config, &valid).unwrap();
        host.script("inspect_hangs", "");
        let asked = Instant::now();
        let resumed = host
            .command(&["start", "--profile", "axpc", "--config"])
            .arg(&host.config)
            .output()
            .unwrap();
        let took = asked.elapsed();
        assert_eq!(resumed.status.code(), Some(0), "{}", texts(&resumed).1);
        assert!(took < Duration::from_secs(10), "{took:?}");
        assert!(
            took >= Duration::from_secs(3),
            "the stalled inspect was waited for: {took:?}"
        );

        assert_eq!(host.client(&["stop"]).status.code(), Some(0));
        assert_eq!(
            started.finish(Duration::from_secs(40)).status.code(),
            Some(0)
        );
    }

    #[test]
    fn status_shows_the_image_its_contract_its_ages_and_its_warnings() {
        let host = Host::new();
        host.script("run_mode", "hold");
        host.script(
            "images",
            &format!(
                "actions-runner:local\tsha256:a\tcom.github.axross.runners.contract={CONTRACT_VERSION}\t\
                 com.github.axross.runners.runner-version=2.337.0\tcom.github.axross.runners.runner-released=2020-01-01"
            ),
        );
        let stub = Stub::start(vec![registered()]);
        let started = host.start(&stub);
        let status = host.wait_for_status("running container r-1-", Duration::from_secs(20));
        let lines: Vec<&str> = status.lines().collect();
        let at = lines
            .iter()
            .position(|line| line.starts_with("image: "))
            .unwrap();
        assert_eq!(lines[at], "image: actions-runner:local");
        assert_eq!(lines[at + 1], format!("image-contract: {CONTRACT_VERSION}"));
        assert_eq!(lines[at + 2], "image-built: unknown");
        assert!(
            lines[at + 3].starts_with("runner: 2.337.0, released "),
            "{status}"
        );
        assert_eq!(
            lines[at + 4],
            "warning: the image was not built by `marshal build`, so its build age is unknown"
        );
        assert!(
            lines[at + 5].starts_with("warning: runner 2.337.0 was released "),
            "{status}"
        );

        fs::write(&host.config, b"{}").unwrap();
        assert_eq!(host.client(&["restart"]).status.code(), Some(1));
        let stopped = host.wait_for_status("state: stopped", Duration::from_secs(5));
        assert!(
            !stopped.contains("pool: ") && !stopped.contains("slot: "),
            "{stopped}"
        );
        assert!(
            stopped.contains(&format!(
                "image: actions-runner:local\nimage-contract: {CONTRACT_VERSION}\n"
            )),
            "a stopped instance still shows its image: {stopped}"
        );
        assert_eq!(host.client(&["stop"]).status.code(), Some(0));
        let finished = started.finish(Duration::from_secs(40));
        assert_eq!(finished.status.code(), Some(0));
        assert!(
            finished
                .stderr
                .contains("WARNING: Image freshness: the image was not built by `marshal build`"),
            "{}",
            finished.stderr
        );
    }
}
