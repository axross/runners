//! JIT registration against a loopback GitHub stub. The base-address override exists
//! only in debug builds, so this file is empty in a release build; the release
//! profile has its own test, `base_address`.
#![cfg(debug_assertions)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::{Duration, Instant};

use runners_cli::json::{self, Value};
use runners_cli::runtime::board::Board;
use runners_cli::runtime::fake::{Call, FakeBackend, FakeClock, MemoryLog};
use runners_cli::runtime::registration::{GitHubRegistrar, HttpLimits, JitRequest, Registrar};
use runners_cli::runtime::secret::Secret;
use runners_cli::runtime::worker::{run_slot, stop_channel};
use support::stub::{Reply, Stub};
use support::{TempDir, slot_spec};

const TOKEN: &str = "ghp_stubtoken0123456789";

fn short() -> HttpLimits {
    HttpLimits {
        total_timeout: Duration::from_millis(600),
        ..HttpLimits::default()
    }
}

async fn register(
    stub: &Stub,
    limits: HttpLimits,
    token: &str,
) -> Result<Secret, runners_cli::runtime::registration::RegistrationError> {
    let registrar = GitHubRegistrar::new(Some(&stub.address), limits).unwrap();
    let labels = vec!["self-hosted".to_string(), "axpc".to_string()];
    let token = Secret::new(token.to_string());
    registrar
        .request_jit(&JitRequest {
            owner: "example-owner",
            repository: "example-repo-one",
            name: "example-repo-one-2-20240305060708009",
            labels: &labels,
            token: &token,
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn registration_sends_the_powershell_request_and_reads_the_response() {
    let stub = Stub::start(vec![Reply::json(
        r#"{"runner":{"id":1},"encoded_jit_config":"abc123=="}"#,
    )]);
    let jit = register(&stub, HttpLimits::default(), TOKEN).await.unwrap();
    assert_eq!(jit.expose(), "abc123==");

    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        "/repos/example-owner/example-repo-one/actions/runners/generate-jitconfig"
    );
    assert_eq!(
        request.header("authorization"),
        Some(&*format!("Bearer {TOKEN}"))
    );
    assert_eq!(
        request.header("accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(request.header("x-github-api-version"), Some("2022-11-28"));
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(
        request.header("user-agent"),
        Some(&*format!("runners/{}", env!("CARGO_PKG_VERSION")))
    );
    let Value::Object(body) = json::parse(&request.body).unwrap() else {
        panic!("the body is not an object");
    };
    let members: Vec<(&str, &Value)> = body.iter().collect();
    assert_eq!(members[0].0, "name");
    assert_eq!(
        members[0].1,
        &Value::String("example-repo-one-2-20240305060708009".to_string())
    );
    assert_eq!(members[1].0, "runner_group_id");
    assert!(matches!(members[1].1, Value::Number(n) if n.literal() == "1"));
    assert_eq!(
        members[2].1,
        &Value::Array(vec![
            Value::String("self-hosted".to_string()),
            Value::String("axpc".to_string())
        ])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_error_response_is_scrubbed_of_the_token_and_cut() {
    let body = format!(
        "{{\"message\":\"Bad   credentials\nfor {TOKEN}\",\"detail\":\"{}\"}}",
        "x".repeat(2000)
    );
    let stub = Stub::start(vec![Reply::status("401 Unauthorized", &body)]);
    let error = register(&stub, HttpLimits::default(), TOKEN)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("Response status code does not indicate success: 401 (Unauthorized)."),
        "{error}"
    );
    assert!(error.contains("Bad credentials for <token>"), "{error}");
    assert!(!error.contains(TOKEN));
    assert_eq!(error.chars().count(), 503);
    assert!(error.ends_with("..."));
}

#[tokio::test(flavor = "current_thread")]
async fn a_short_error_is_kept_whole_on_one_line() {
    let stub = Stub::start(vec![Reply::status(
        "404 Not Found",
        "{\n  \"message\": \"Not Found\"\n}",
    )]);
    let error = register(&stub, HttpLimits::default(), TOKEN)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "Response status code does not indicate success: 404 (Not Found). { \"message\": \"Not Found\" }"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_redirect_is_a_failure_and_is_not_followed() {
    let stub = Stub::start(vec![
        Reply::RedirectToSelf,
        Reply::json(r#"{"encoded_jit_config":"reached-by-following"}"#),
    ]);
    let started = Instant::now();
    let error = register(&stub, HttpLimits::default(), TOKEN)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("307"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(stub.requests().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_stalled_response_fails_at_the_total_timeout() {
    let stub = Stub::start(vec![Reply::Stall]);
    let started = Instant::now();
    let error = register(&stub, short(), TOKEN)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("timed out"), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert!(!error.contains(TOKEN));
}

#[tokio::test(flavor = "current_thread")]
async fn a_response_over_one_mebibyte_fails_whether_declared_or_streamed() {
    let declared = Stub::start(vec![Reply::Respond {
        status: "200 OK",
        headers: Vec::new(),
        body: vec![b'a'; 1_048_577],
    }]);
    let error = register(&declared, HttpLimits::default(), TOKEN)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("larger than 1048576 bytes"), "{error}");

    let streamed = Stub::start(vec![Reply::Chunked(3 * 1024 * 1024)]);
    let error = register(&streamed, HttpLimits::default(), TOKEN)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("larger than 1048576 bytes"), "{error}");

    let (prefix, suffix) = ("{\"encoded_jit_config\":\"", "\"}");
    let body = format!(
        "{prefix}{}{suffix}",
        "a".repeat(1_048_576 - prefix.len() - suffix.len())
    )
    .into_bytes();
    assert_eq!(body.len(), 1_048_576);
    let exact = Stub::start(vec![Reply::Respond {
        status: "200 OK",
        headers: Vec::new(),
        body,
    }]);
    assert!(register(&exact, HttpLimits::default(), TOKEN).await.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn a_response_without_a_configuration_is_a_failure() {
    for body in [r#"{"encoded_jit_config":""}"#, "{}", "not json"] {
        let stub = Stub::start(vec![Reply::json(body)]);
        let error = register(&stub, HttpLimits::default(), TOKEN)
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(error, "GitHub returned no JIT configuration", "{body}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_that_cannot_be_a_header_fails_without_naming_it() {
    let stub = Stub::start(Vec::new());
    let error = register(&stub, HttpLimits::default(), "ghp_a\nb")
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("ghp_"), "{error}");
    assert!(stub.requests().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn an_owner_or_repository_that_would_leave_the_repos_path_is_refused_unsent() {
    let stub = Stub::start(Vec::new());
    let registrar = GitHubRegistrar::new(Some(&stub.address), HttpLimits::default()).unwrap();
    let labels = vec!["self-hosted".to_string()];
    let token = Secret::new(TOKEN.to_string());
    for (owner, repository) in [
        (".", "example-repo-one"),
        ("..", "example-repo-one"),
        ("example-owner", "."),
        ("example-owner", ".."),
        ("", "example-repo-one"),
        ("example-owner", ""),
        ("example/owner", "example-repo-one"),
        ("example-owner", "example/repo"),
    ] {
        let error = registrar
            .request_jit(&JitRequest {
                owner,
                repository,
                name: "example-repo-one-2-20240305060708009",
                labels: &labels,
                token: &token,
            })
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(
            error, "the repository owner or name is not usable in a URL",
            "{owner:?}/{repository:?}"
        );
    }
    assert!(stub.requests().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_base_address_that_is_not_loopback_is_refused_in_a_debug_build() {
    for bad in [
        "https://api.github.com",
        "http://localhost:80",
        "http://127.0.0.1",
        "http://evil.example:1",
    ] {
        assert!(
            GitHubRegistrar::new(Some(bad), HttpLimits::default()).is_err(),
            "{bad}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn each_registration_failure_goes_into_the_slots_backoff_and_a_later_success_runs() {
    let stub = Stub::start(vec![
        Reply::RedirectToSelf,
        Reply::Stall,
        Reply::Chunked(2 * 1024 * 1024),
        Reply::json(r#"{"encoded_jit_config":"the-jit-config"}"#),
    ]);
    let scratch = TempDir::new();
    let token = scratch.write_token("token", format!("{TOKEN}\n").as_bytes());
    let slot = slot_spec(token);
    let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
    let backend = FakeBackend::default();
    let clock = FakeClock::at(1_709_618_828_009);
    let log = MemoryLog::default();
    let (stop, receiver) = stop_channel();
    backend.state.borrow_mut().stop_when_exits_run_out = Some(stop);

    run_slot(
        &slot,
        &backend,
        &clock,
        &registrar,
        &log,
        &Board::default(),
        receiver,
    )
    .await;

    assert_eq!(
        clock.sleeps(),
        [
            Duration::from_secs(5),
            Duration::from_secs(10),
            Duration::from_secs(20)
        ]
    );
    let warnings: Vec<String> = log
        .lines()
        .into_iter()
        .filter(|l| l.starts_with("WARN"))
        .collect();
    assert_eq!(warnings.len(), 3);
    assert!(
        warnings[0].contains("failed to request a JIT registration") && warnings[0].contains("307"),
        "{warnings:?}"
    );
    assert!(warnings[0].ends_with("retrying in 5s."));
    assert!(warnings[1].ends_with("retrying in 10s."));
    assert!(warnings[2].ends_with("retrying in 20s."));
    assert!(warnings.iter().all(|line| !line.contains(TOKEN)));
    let state = backend.state.borrow();
    assert_eq!(state.jit_values, ["the-jit-config"]);
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|call| matches!(call, Call::RunContainer(_)))
            .count(),
        1
    );
    assert_eq!(stub.requests().len(), 4);
}
