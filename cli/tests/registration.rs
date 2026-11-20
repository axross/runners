//! JIT registration against a loopback GitHub stub; the base-address override exists
//! only in debug builds, so this file is empty in a release build.
#![cfg(debug_assertions)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::{Duration, Instant};

use marshal_cli::json::{self, Value};
use marshal_cli::runtime::board::Board;
use marshal_cli::runtime::fake::{Call, FakeBackend, FakeClock, MemoryLog};
use marshal_cli::runtime::registration::{
    GitHubRegistrar, HttpLimits, JitRequest, REJECTED_MARK, Registrar,
};
use marshal_cli::runtime::secret::Secret;
use marshal_cli::runtime::worker::{run_slot, stop_channel};
use marshal_cli::validate::PoolTarget;
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
) -> Result<Secret, marshal_cli::runtime::registration::RegistrationError> {
    let registrar = GitHubRegistrar::new(Some(&stub.address), limits).unwrap();
    let labels = vec!["self-hosted".to_string(), "axpc".to_string()];
    let token = Secret::new(token.to_string());
    registrar
        .request_jit(&JitRequest {
            target: &PoolTarget::Repository {
                owner: "example-owner".to_string(),
                repository: "example-repo-one".to_string(),
            },
            name: "example-repo-one-2-20240305060708009",
            labels: &labels,
            token: &token,
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn registration_sends_the_request_and_reads_the_response() {
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
        Some(&*format!("marshal/{}", env!("CARGO_PKG_VERSION")))
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
    assert!(error.contains(REJECTED_MARK), "{error}");
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
                target: &PoolTarget::Repository {
                    owner: owner.to_string(),
                    repository: repository.to_string(),
                },
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

fn safe_group() -> serde_json::Value {
    serde_json::json!({"id": 47, "name": "selected-private", "default": false,
        "inherited": false, "visibility": "selected", "allows_public_repositories": false})
}

/// serves only group metadata: registration must not require repository-list
/// access from an organisation token with no repository permissions.
fn group_page(groups: Vec<serde_json::Value>, headers: Vec<(&'static str, String)>) -> Reply {
    Reply::Respond {
        status: "200 OK",
        headers,
        body: serde_json::to_vec(&serde_json::json!({"runner_groups": groups})).unwrap(),
    }
}

async fn register_organization(
    registrar: &GitHubRegistrar,
    group: &str,
    token: &str,
) -> Result<Secret, marshal_cli::runtime::registration::RegistrationError> {
    registrar
        .request_jit(&JitRequest {
            target: &PoolTarget::Organization {
                organization: "example-org".to_string(),
                runner_group: group.to_string(),
            },
            name: "org-linux-2-20240305060708009",
            labels: &["self-hosted".to_string(), "org-linux".to_string()],
            token: &Secret::new(token.to_string()),
        })
        .await
}

/// workflow restrictions may narrow either selected group.
#[tokio::test(flavor = "current_thread")]
async fn organization_registration_uses_the_checked_group_without_reading_its_repositories() {
    for restricted in [false, true] {
        let mut group = safe_group();
        group["restricted_to_workflows"] = restricted.into();
        let stub = Stub::start(vec![
            group_page(vec![group], vec![]),
            Reply::json(r#"{"encoded_jit_config":"org-config"}"#),
        ]);
        let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
        assert_eq!(
            register_organization(&registrar, "selected-private", TOKEN)
                .await
                .unwrap()
                .expose(),
            "org-config"
        );
        let requests = stub.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/orgs/example-org/actions/runner-groups?per_page=100"
        );
        assert_eq!(requests[1].method, "POST");
        assert_eq!(
            requests[1].path,
            "/orgs/example-org/actions/runners/generate-jitconfig"
        );
        let body: serde_json::Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"name":"org-linux-2-20240305060708009", "runner_group_id":47, "labels":["self-hosted","org-linux"]})
        );
        for request in requests {
            assert_eq!(
                request.header("authorization"),
                Some(format!("Bearer {TOKEN}").as_str())
            );
            assert_eq!(request.header("x-github-api-version"), Some("2022-11-28"));
        }
    }
}

/// one unsafe or malformed field must fail closed even when every other field
/// describes an admissible group.
#[tokio::test(flavor = "current_thread")]
async fn unsafe_or_unknown_group_flags_never_reach_registration() {
    for (field, value, cause) in [
        (
            "default",
            serde_json::json!(true),
            "the default group is refused",
        ),
        (
            "inherited",
            serde_json::json!(true),
            "an inherited group is refused",
        ),
        (
            "visibility",
            serde_json::json!("all"),
            "the group admits every repository",
        ),
        (
            "visibility",
            serde_json::json!("private"),
            "the group admits every private repository",
        ),
        (
            "visibility",
            serde_json::json!("unknown"),
            "visibility is not recognised",
        ),
        (
            "allows_public_repositories",
            serde_json::json!(true),
            "the group allows public repositories",
        ),
        ("id", serde_json::json!(0), "the runner group id is invalid"),
        (
            "default",
            serde_json::Value::Null,
            "invalid runner group list",
        ),
        (
            "inherited",
            serde_json::json!("false"),
            "invalid runner group list",
        ),
        (
            "name",
            serde_json::json!("renamed"),
            "runner group not found",
        ),
    ] {
        let mut group = safe_group();
        group[field] = value;
        let stub = Stub::start(vec![group_page(vec![group], vec![])]);
        let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
        let error = register_organization(&registrar, "selected-private", TOKEN)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(cause), "{field}: {error}");
        assert_eq!(stub.requests().len(), 1);
    }
}

/// conditional replies must preserve both safe and refused states; a later fresh
/// safe answer restores registration without reconstructing the registrar.
#[tokio::test(flavor = "current_thread")]
async fn group_checks_use_authorized_304s_and_recover_without_restart_or_stale_flags() {
    let mut unsafe_group = safe_group();
    unsafe_group["visibility"] = "private".into();
    let stub = Stub::start(vec![
        group_page(vec![safe_group()], vec![("ETag", "\"safe\"".to_string())]),
        Reply::json(r#"{"encoded_jit_config":"first"}"#),
        Reply::status("304 Not Modified", ""),
        Reply::json(r#"{"encoded_jit_config":"replacement"}"#),
        group_page(vec![unsafe_group], vec![("ETag", "\"unsafe\"".to_string())]),
        Reply::status("304 Not Modified", ""),
        group_page(vec![safe_group()], vec![("ETag", "\"fixed\"".to_string())]),
        Reply::json(r#"{"encoded_jit_config":"recovered"}"#),
    ]);
    let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
    assert_eq!(
        register_organization(&registrar, "selected-private", TOKEN)
            .await
            .unwrap()
            .expose(),
        "first"
    );
    assert_eq!(
        register_organization(&registrar, "selected-private", "replacement-token")
            .await
            .unwrap()
            .expose(),
        "replacement"
    );
    for _ in 0..2 {
        assert!(
            register_organization(&registrar, "selected-private", TOKEN)
                .await
                .unwrap_err()
                .to_string()
                .contains("every private repository")
        );
    }
    assert_eq!(
        register_organization(&registrar, "selected-private", TOKEN)
            .await
            .unwrap()
            .expose(),
        "recovered"
    );
    let requests = stub.requests();
    assert_eq!(requests.len(), 8);
    assert_eq!(requests[2].header("if-none-match"), Some("\"safe\""));
    assert_eq!(
        requests[2].header("authorization"),
        Some("Bearer replacement-token")
    );
    assert_eq!(
        requests[3].header("authorization"),
        Some("Bearer replacement-token")
    );
    assert_eq!(requests[5].header("if-none-match"), Some("\"unsafe\""));
}

/// a cached safe answer is not authority after a failed read, and an unsolicited
/// 304 is not authority when this registrar has no matching cached answer.
#[tokio::test(flavor = "current_thread")]
async fn an_unauthorized_or_missing_group_read_cannot_reuse_a_previous_permission() {
    for (status, cause) in [
        ("401 Unauthorized", REJECTED_MARK),
        ("403 Forbidden", "403"),
        ("404 Not Found", "organisation not found"),
    ] {
        let stub = Stub::start(vec![
            group_page(vec![safe_group()], vec![("ETag", "\"old\"".to_string())]),
            Reply::json(r#"{"encoded_jit_config":"old"}"#),
            Reply::status(
                status,
                &serde_json::json!({"message": format!("refused {TOKEN}")}).to_string(),
            ),
        ]);
        let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
        register_organization(&registrar, "selected-private", TOKEN)
            .await
            .unwrap();
        let error = register_organization(&registrar, "selected-private", TOKEN)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(cause), "{error}");
        assert!(!error.contains(TOKEN), "{error}");
        assert_eq!(stub.requests().len(), 3);
    }
    let stub = Stub::start(vec![Reply::status("304 Not Modified", "")]);
    let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
    assert!(
        register_organization(&registrar, "selected-private", TOKEN)
            .await
            .unwrap_err()
            .to_string()
            .contains("no cached answer")
    );
}

/// finding a safe group on an early page is insufficient until every next page
/// succeeds under the same origin and endpoint boundary.
#[tokio::test(flavor = "current_thread")]
async fn group_paging_is_complete_and_confined_to_its_origin_and_path() {
    let next = "<{stub}/orgs/example-org/actions/runner-groups?per_page=100&page=2>; rel=\"next\"";
    let stub = Stub::start(vec![
        group_page(vec![], vec![("Link", next.to_string())]),
        group_page(vec![safe_group()], vec![]),
        Reply::json(r#"{"encoded_jit_config":"page-two"}"#),
    ]);
    let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
    assert_eq!(
        register_organization(&registrar, "selected-private", TOKEN)
            .await
            .unwrap()
            .expose(),
        "page-two"
    );
    assert_eq!(
        stub.requests()[1].path,
        "/orgs/example-org/actions/runner-groups?per_page=100&page=2"
    );
    for link in [
        "<http://example.invalid/orgs/example-org/actions/runner-groups?page=2>; rel=\"next\"",
        "<{stub}/orgs/other/actions/runner-groups?page=2>; rel=\"next\"",
        "<{stub}/repos/example-org/repo/actions/runners?page=2>; rel=\"next\"",
        "<{stub}/orgs/example-org/actions/runner-groups?per_page=100>; rel=\"next\"",
    ] {
        let stub = Stub::start(vec![group_page(
            vec![safe_group()],
            vec![("Link", link.to_string())],
        )]);
        let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
        assert!(
            register_organization(&registrar, "selected-private", TOKEN)
                .await
                .is_err(),
            "{link}"
        );
        assert_eq!(stub.requests().len(), 1);
    }
    let stub = Stub::start(vec![
        group_page(vec![safe_group()], vec![("Link", next.to_string())]),
        Reply::status("403 Forbidden", ""),
    ]);
    let registrar = GitHubRegistrar::new(Some(&stub.address), short()).unwrap();
    assert!(
        register_organization(&registrar, "selected-private", TOKEN)
            .await
            .is_err()
    );
    assert!(
        stub.requests()
            .iter()
            .all(|request| request.method == "GET")
    );
}

/// the extra group read must not weaken the registrar's request limits.
#[tokio::test(flavor = "current_thread")]
async fn organization_group_and_jit_reads_share_the_redirect_timeout_and_size_bounds() {
    for at_jit in [false, true] {
        for reply in [Reply::RedirectToSelf, Reply::Stall, Reply::Chunked(1025)] {
            let mut replies = Vec::new();
            if at_jit {
                replies.push(group_page(vec![safe_group()], vec![]));
            }
            replies.push(reply);
            let stub = Stub::start(replies);
            let registrar = GitHubRegistrar::new(
                Some(&stub.address),
                HttpLimits {
                    max_response_bytes: 1024,
                    ..short()
                },
            )
            .unwrap();
            let started = Instant::now();
            assert!(
                register_organization(&registrar, "selected-private", TOKEN)
                    .await
                    .is_err()
            );
            assert!(started.elapsed() < Duration::from_secs(3));
            assert_eq!(stub.requests().len(), if at_jit { 2 } else { 1 });
        }
    }
}
