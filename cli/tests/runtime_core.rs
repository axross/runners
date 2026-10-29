//! the runtime core `start` runs, on the fake backend and registrar.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use marshal_cli::image::contract::CONTRACT_VERSION;
use marshal_cli::instance::lifecycle::{Core, Ending};
use marshal_cli::instance::runtime_core::RuntimeCore;
use marshal_cli::profile::Profile;
use marshal_cli::runtime::fake::{Call, FakeBackend, FakeClock, FakeRegistrar, MemoryLog};
use marshal_cli::runtime::worker::stop_channel;
use marshal_cli::validate::{Plan, validate};
use support::TempDir;

const START: u64 = 1_709_618_828_009;

fn reported_names(core: &impl Core) -> Vec<&'static str> {
    core.report().into_iter().map(|(name, _)| name).collect()
}

fn plan(scratch: &TempDir) -> Plan {
    let config = r#"{"imageName":"actions-runner:local","repositories":[{"owner":"o","repository":"r",
        "name":"r","slots":1,"tokenPath":"C:\\r.token","volumes":[]}]}"#;
    let mut plan = validate(config.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    plan.entries[0].token_path = scratch.path().join("token").to_str().unwrap().to_string();
    plan
}

#[tokio::test(flavor = "current_thread")]
async fn no_startable_pool_is_a_failure_that_says_there_is_nothing_to_run() {
    let scratch = TempDir::new();
    let (backend, clock, registrar, log) = (
        FakeBackend::default(),
        FakeClock::at(START),
        FakeRegistrar::default(),
        MemoryLog::default(),
    );
    let core = RuntimeCore::new(&backend, &clock, &registrar, &log);
    let (_stop, receiver) = stop_channel();
    let failure = core.run(&plan(&scratch), receiver).await.unwrap_err();
    assert_eq!(failure.ending, Ending::NothingToRun);
    assert!(failure.containers.is_empty());
    assert!(
        failure.message.contains("nothing to run"),
        "{}",
        failure.message
    );
    assert_eq!(
        reported_names(&core),
        ["image", "image-contract", "image-built", "runner"],
        "the pools are cleared when the run ends, and the image stays"
    );
    assert!(
        core.report()
            .contains(&("image-contract", "not checked".to_string()))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_core_runs_each_slot_until_the_stop_and_reports_only_the_image_after_it() {
    let scratch = TempDir::new();
    scratch.write_token("token", b"ghp_coretoken\n");
    let (backend, clock, registrar, log) = (
        FakeBackend::default(),
        FakeClock::at(START),
        FakeRegistrar::default(),
        MemoryLog::default(),
    );
    let core = RuntimeCore::new(&backend, &clock, &registrar, &log);
    let (stop, receiver) = stop_channel();
    backend.state.borrow_mut().stop_when_exits_run_out = Some(stop);
    core.run(&plan(&scratch), receiver).await.unwrap();
    assert!(
        backend
            .calls()
            .iter()
            .any(|call| matches!(call, Call::RunContainer(spec) if spec.name.starts_with("r-1-")))
    );
    assert_eq!(registrar.seen.borrow()[0].token, "ghp_coretoken");
    let names = reported_names(&core);
    assert!(
        !names.contains(&"pool") && !names.contains(&"slot"),
        "{names:?}"
    );
    assert!(
        core.report()
            .contains(&("image-contract", CONTRACT_VERSION.to_string())),
        "{:?}",
        core.report()
    );
}
