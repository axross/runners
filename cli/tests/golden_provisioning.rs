//! the golden VM's guest calls run through the real `tart` client against the fake
//! `tart`, in the order `build` will sequence them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use marshal_cli::golden::key::{KeyCheckError, check_key_listing};
use marshal_cli::golden::pins::DOCKER_KEY_FINGERPRINT;
use marshal_cli::golden::steps::{
    GuestCall, LAUNCHER, disable_ssh_call, fetch_key_call, install_docker_call,
    install_launcher_call, list_key_call, prepare_call,
};
use support::tart::{Fake, Invocation};

const VM: &str = "marshal-build-linux-20240305060708009";
const KEY_LISTING: &str = include_str!("../src/golden/fixtures/docker-key-listing.txt");

async fn run(fake: &Fake, call: &GuestCall) -> String {
    let answer = fake
        .client
        .exec(VM, &call.command, call.input.as_deref(), call.budget)
        .await
        .unwrap();
    assert!(answer.succeeded());
    answer.stdout
}

/// the calls `build` makes, up to and including the first refusal of the key.
async fn provision(fake: &Fake) -> Result<(), KeyCheckError> {
    run(fake, &prepare_call()).await;
    run(fake, &fetch_key_call()).await;
    let listing = run(fake, &list_key_call()).await;
    let key = check_key_listing(&listing)?;
    run(fake, &install_docker_call(&key)).await;
    run(fake, &install_launcher_call()).await;
    run(fake, &disable_ssh_call()).await;
    Ok(())
}

fn exec_calls(fake: &Fake) -> Vec<Invocation> {
    fake.invocations()
        .into_iter()
        .filter(|call| call.arguments.first().map(String::as_str) == Some("exec"))
        .collect()
}

fn script_names(calls: &[Invocation]) -> Vec<String> {
    calls
        .iter()
        .map(|call| {
            let at = call.arguments.iter().position(|word| word == "-c").unwrap();
            call.arguments[at + 2].clone()
        })
        .collect()
}

#[tokio::test]
async fn the_calls_run_in_order_with_the_launcher_and_the_source_on_standard_input() {
    let fake = Fake::new();
    fake.seed(&[(VM, "running", "local")]);
    fake.script("exec_stdout", KEY_LISTING.trim_end_matches('\n'));

    provision(&fake).await.unwrap();

    let calls = exec_calls(&fake);
    assert_eq!(
        script_names(&calls),
        [
            "marshal-prepare",
            "marshal-fetch-key",
            "marshal-list-key",
            "marshal-install-docker",
            "marshal-install-launcher",
            "marshal-disable-ssh",
        ]
    );
    for call in &calls {
        assert!(call.arguments.contains(&VM.to_string()));
    }
    let stdin: Vec<Option<Vec<u8>>> = calls.iter().map(|call| call.stdin.clone()).collect();
    assert_eq!(stdin[0], None);
    assert_eq!(stdin[2], None);
    assert_eq!(stdin[3].as_deref(), Some(b"Types: deb\nURIs: https://download.docker.com/linux/ubuntu\nSuites: noble\nComponents: stable\nArchitectures: arm64\nSigned-By: /etc/apt/keyrings/docker.asc\n".as_slice()));
    assert_eq!(stdin[4].as_deref(), Some(LAUNCHER.as_bytes()));
    assert_eq!(
        calls[4].arguments.last().unwrap(),
        "/usr/local/bin/marshal-launch"
    );
    assert!(
        calls
            .iter()
            .all(|call| !call.jit_present && !call.stdin_terminal)
    );
}

#[tokio::test]
async fn a_key_with_another_fingerprint_stops_the_build_before_anything_is_installed() {
    let fake = Fake::new();
    fake.seed(&[(VM, "running", "local")]);
    let mismatched = KEY_LISTING.replacen(
        DOCKER_KEY_FINGERPRINT,
        "0123456789ABCDEF0123456789ABCDEF01234567",
        1,
    );
    fake.script("exec_stdout", mismatched.trim_end_matches('\n'));

    let error = provision(&fake).await.unwrap_err();

    assert!(matches!(error, KeyCheckError::Mismatch { .. }));
    assert_eq!(
        script_names(&exec_calls(&fake)),
        ["marshal-prepare", "marshal-fetch-key", "marshal-list-key"]
    );
}

#[tokio::test]
async fn an_empty_listing_stops_the_build_before_anything_is_installed() {
    let fake = Fake::new();
    fake.seed(&[(VM, "running", "local")]);
    fake.script("exec_stdout", "");

    let error = provision(&fake).await.unwrap_err();

    assert!(matches!(error, KeyCheckError::Malformed(_)));
    assert_eq!(exec_calls(&fake).len(), 3);
}
