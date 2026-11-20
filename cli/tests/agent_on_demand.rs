//! on-demand pools behind the resident channel, with a loopback scale-set service.
#![cfg(debug_assertions)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use marshal_cli::agent::CallError;
use marshal_cli::agent::machine::Launch;
use marshal_cli::instance::lifecycle::StopTimes;
use marshal_cli::runtime::fake::{Call, HostCall};
use marshal_cli::token_store::{StoreError, StoreErrorKind};
use serde_json::{Value, json};
use support::agent::{JIT, ONE_POOL, OTHER_TOKEN, Rig, TOKEN};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

const CONFIG: &str = r#"schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"
[[pool]]
name = "logical"
repository = "o/r"
scale_set = "axpc-jobs"
runners = 2
"#;

/// the exit deadline of a stop that the slow scale-set cleanup outlasts.
const EXIT_DEADLINE: Duration = Duration::from_millis(300);

/// the budget the fake backend reserves for releasing a job host.
const RELEASE_BUDGET: Duration = Duration::from_millis(100);

#[derive(Default)]
struct ServiceState {
    assigned: u64,
    sent: Option<u64>,
    registrations: BTreeMap<u64, String>,
    jit_names: Vec<String>,
    owners: Vec<String>,
    closed: usize,
    active_session: Option<String>,
    session_serial: u64,
    session_deletes: Vec<String>,
    create_status: Option<u16>,
    close_status: Option<u16>,
    lose_create_response: bool,
    lose_close_response: bool,
    close_delay: Duration,
    close_in_flight: bool,
    polls: usize,
    reject_labels: bool,
    fail_deletion: bool,
    fail_listing: bool,
    jit_delay: Duration,
    jit_in_flight: bool,
    lose_jit_response: bool,
    lose_delete_response: bool,
    delete_delay: Duration,
    delete_in_flight: bool,
    requests: Vec<(String, String, bool)>,
}

struct Service {
    base: String,
    state: Arc<Mutex<ServiceState>>,
    task: JoinHandle<()>,
}

impl Service {
    async fn start(assigned: u64) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(ServiceState {
            assigned,
            ..ServiceState::default()
        }));
        let (own, shared) = (base.clone(), state.clone());
        let task = tokio::spawn(async move {
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let (base, state) = (own.clone(), shared.clone());
                        clients.spawn(async move { serve(stream, &base, &state).await });
                    }
                    Some(result) = clients.join_next() => { result.unwrap(); }
                }
            }
        });
        Self { base, state, task }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn statistics(assigned: u64) -> Value {
    json!({"totalAssignedJobs": assigned, "totalRunningJobs": 0, "totalIdleRunners": 0})
}

/// models registrations independently of the runtime, so stopping a container cannot erase one.
fn answer(
    method: &str,
    path: &str,
    body: &Value,
    base: &str,
    state: &mut ServiceState,
) -> (u16, Value) {
    let path = path.split('?').next().unwrap();
    if path.ends_with("registration-token") {
        return (201, json!({"token": "<registration-placeholder>"}));
    }
    if path.ends_with("runner-registration") {
        return (
            200,
            json!({"url": format!("{base}/tenant"), "token": "fake.eyJleHAiOjQxMDI0NDQ4MDB9.signature"}),
        );
    }
    if path.ends_with("runnerscalesets/23") {
        return (
            200,
            json!({"id": 23, "name": "axpc-jobs", "runnerGroupId": 1, "labels": [{"name": "axpc-jobs"}]}),
        );
    }
    if path.ends_with("runnerscalesets") {
        let labels = if state.reject_labels {
            json!([{"name": "axpc-jobs"}, {"name": "self-hosted"}])
        } else {
            json!([{"name": "axpc-jobs"}])
        };
        return (
            200,
            json!({"count": 1, "value": [{"id": 23, "name": "axpc-jobs", "runnerGroupId": 1, "labels": labels}]}),
        );
    }
    if path.ends_with("sessions") {
        if let Some(status) = state.create_status {
            return (status, Value::Null);
        }
        if state.active_session.is_some() {
            return (409, Value::Null);
        }
        state.session_serial += 1;
        let id = format!("01234567-89ab-cdef-0123-{:012x}", state.session_serial);
        state.active_session = Some(id.clone());
        state
            .owners
            .push(body["ownerName"].as_str().unwrap().to_string());
        return (
            200,
            json!({"sessionId": id, "messageQueueUrl": format!("{base}/queue"), "messageQueueAccessToken": "<queue-placeholder>", "statistics": statistics(state.assigned)}),
        );
    }
    if path.contains("/sessions/") && method == "DELETE" {
        state.close_in_flight = false;
        let id = path.rsplit('/').next().unwrap();
        state.session_deletes.push(id.to_string());
        if state.active_session.as_deref() != Some(id) {
            return (404, Value::Null);
        }
        if let Some(status) = state.close_status {
            if status == 404 {
                state.active_session = None;
            }
            return (status, Value::Null);
        }
        state.active_session = None;
        state.closed += 1;
        return (204, Value::Null);
    }
    if path == "/queue" {
        state.polls += 1;
        if state.sent == Some(state.assigned) {
            return (202, Value::Null);
        }
        state.sent = Some(state.assigned);
        return (
            200,
            json!({"messageId": state.polls, "messageType": "RunnerScaleSetJobMessages", "body": "[]", "statistics": statistics(state.assigned)}),
        );
    }
    if path.starts_with("/queue/") && method == "DELETE" {
        return (204, Value::Null);
    }
    if path.ends_with("generatejitconfig") {
        state.jit_in_flight = false;
        let name = body["name"].as_str().unwrap().to_string();
        state.jit_names.push(name.clone());
        let id = state.jit_names.len() as u64;
        state.registrations.insert(id, name.clone());
        return (
            200,
            json!({"runner": {"id": id, "name": name, "runnerScaleSetId": 23}, "encodedJITConfig": JIT}),
        );
    }
    if path == "/repos/o/r/actions/runners" {
        if state.fail_listing {
            return (500, Value::Null);
        }
        let runners: Vec<_> = state
            .registrations
            .iter()
            .map(|(id, name)| json!({"id": id, "name": name, "status": "online", "busy": false}))
            .collect();
        return (
            200,
            json!({"total_count": runners.len(), "runners": runners}),
        );
    }
    if path == "/repos/o/s/actions/runners" {
        return (200, json!({"total_count": 0, "runners": []}));
    }
    if let Some(id) = path
        .strip_prefix("/repos/o/r/actions/runners/")
        .and_then(|id| id.parse::<u64>().ok())
    {
        if method == "DELETE" {
            if state.fail_deletion {
                return (500, Value::Null);
            }
            state.registrations.remove(&id);
            return (204, Value::Null);
        }
        return state
            .registrations
            .get(&id)
            .map_or((404, Value::Null), |name| {
                (
                    200,
                    json!({"id": id, "name": name, "status": "online", "busy": false}),
                )
            });
    }
    panic!("unexpected request {method} {path}");
}

/// one request per connection, allowing the long poll and runtime cleanup to overlap.
async fn serve(mut stream: TcpStream, base: &str, state: &Mutex<ServiceState>) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 1024 * 1024);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let size = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|size| size.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < end + size {
        let count = stream.read(&mut buffer).await.unwrap();
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let mut first = head.lines().next().unwrap().split_whitespace();
    let (method, path) = (first.next().unwrap(), first.next().unwrap());
    let path = path.split('?').next().unwrap();
    state.lock().unwrap().requests.push((
        method.to_string(),
        path.to_string(),
        head.lines()
            .any(|line| line == format!("authorization: Bearer {OTHER_TOKEN}")),
    ));
    let body = if size == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[end..end + size]).unwrap()
    };
    if path.starts_with("/queue") && method == "GET" {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    if path.ends_with("generatejitconfig") {
        let delay = {
            let mut state = state.lock().unwrap();
            state.jit_in_flight = true;
            state.jit_delay
        };
        tokio::time::sleep(delay).await;
    }
    if method == "DELETE" && path.contains("/actions/runners/") {
        let delay = {
            let mut state = state.lock().unwrap();
            state.delete_in_flight = true;
            state.delete_delay
        };
        tokio::time::sleep(delay).await;
        state.lock().unwrap().delete_in_flight = false;
    }
    if method == "DELETE" && path.contains("/sessions/") {
        let delay = {
            let mut state = state.lock().unwrap();
            state.close_in_flight = true;
            state.close_delay
        };
        tokio::time::sleep(delay).await;
    }
    let (status, body) = answer(method, path, &body, base, &mut state.lock().unwrap());
    {
        let state = state.lock().unwrap();
        if (path.ends_with("generatejitconfig") && state.lose_jit_response)
            || (path.ends_with("sessions") && state.lose_create_response)
            || (method == "DELETE" && path.contains("/sessions/") && state.lose_close_response)
            || (method == "DELETE"
                && path.contains("/actions/runners/")
                && state.lose_delete_response)
        {
            return;
        }
    }
    let body = if status == 202 || status == 204 {
        String::new()
    } else {
        body.to_string()
    };
    let response = format!(
        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

fn rig(service: &Service) -> Rig {
    let mut rig = Rig::new(ONE_POOL);
    rig.write_toml(CONFIG);
    rig.registrar.base_override = Some(service.base.clone());
    rig.clock.real.set(true);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig
}

fn containers(rig: &Rig) -> Vec<String> {
    rig.backend
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::RunContainer(spec) => Some(spec.name),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn hidden_window_demand_respects_capacity_and_stop_cleans_exact_registrations() {
    let service = Service::start(0).await;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("the idle listener", |snapshot| {
                snapshot.pools[0]
                    .on_demand
                    .as_ref()
                    .is_some_and(|listener| listener.state == "listening")
            })
            .await;
            assert!(containers(rig).is_empty());
            assert!(service.state.lock().unwrap().registrations.is_empty());
            service.state.lock().unwrap().assigned = 3;
            conn.until("two runners", |snapshot| snapshot.runners_up == 2)
                .await;
            assert_eq!(containers(rig).len(), 2);
            assert!(
                containers(rig)
                    .iter()
                    .all(|name| name.starts_with("axpc-jobs-"))
            );
            assert!(
                rig.registered_with().is_empty(),
                "always-on JIT must not be used"
            );
            assert_eq!(service.state.lock().unwrap().owners, ["axpc"]);
            rig.mark_running();
            assert_eq!(conn.ok("stop_all").await["state"], "stopped");
            let snapshot = conn.status().await;
            assert_eq!(
                snapshot.pools[0].on_demand.as_ref().unwrap().state,
                "stopped"
            );
            assert!(
                !snapshot.pools[0]
                    .on_demand
                    .as_ref()
                    .unwrap()
                    .cleanup_incomplete
            );
            {
                let state = service.state.lock().unwrap();
                assert!(state.registrations.is_empty());
                assert_eq!(state.jit_names.len(), 2);
                assert_eq!(state.closed, 1);
            }
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_scale_set_on_a_mac_reload_is_invalid_and_the_service_sees_nothing() {
    const MAC: &str = "schema_version = 1\nrouting_label = \"axmc\"\n\n[budget]\ncpus = 8\n\
         memory_gb = 32\ndisk_gb = 180\n\n[[pool]]\nname = \"logical\"\nrepository = \"o/r\"\n\
         runners = 1\nos = \"linux\"\ndisk_gb = 40\n";
    let service = Service::start(1).await;
    let mut rig = Rig::new(ONE_POOL);
    rig.remove("config.json");
    rig.profile.set(marshal_cli::profile::Profile::Axmc);
    rig.write_toml(MAC);
    rig.registrar.base_override = Some(service.base.clone());
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            rig.write_toml(
                &MAC.replace("runners = 1\n", "runners = 1\nscale_set = \"axmc-jobs\"\n"),
            );
            let revision = conn.ok("config_read").await["revision"]
                .as_str()
                .unwrap()
                .to_string();
            let error = conn
                .call("config_reload", json!({"revision": revision}))
                .await
                .unwrap_err();
            let CallError::Agent(body) = error else {
                panic!("the reload was not refused: {error:?}");
            };
            assert_eq!(body.code, "config_invalid");
            assert_eq!(body.problems[0].path.as_deref(), Some("pool[0].scale_set"));
            let snapshot = conn.status().await;
            assert_eq!(snapshot.configuration.status, "invalid");
            assert!(
                snapshot.configuration.problems[0].starts_with("line 13: pool[0].scale_set: "),
                "{:?}",
                snapshot.configuration.problems
            );
            assert_eq!(snapshot.configuration.in_use.unwrap().source, "last_valid");
            assert_eq!(snapshot.runners_up, 1);
            assert!(snapshot.pools[0].on_demand.is_none());
            conn.ok("quit").await;
        })
        .await;
    assert!(
        service.state.lock().unwrap().requests.is_empty(),
        "{:?}",
        service.state.lock().unwrap().requests
    );
}

#[tokio::test(flavor = "current_thread")]
async fn initial_statistics_start_without_a_job_event_and_failed_cleanup_stays_visible() {
    let service = Service::start(1).await;
    service.state.lock().unwrap().sent = Some(1);
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            assert_eq!(containers(rig).len(), 1);
            service.state.lock().unwrap().fail_deletion = true;
            rig.mark_running();
            conn.ok("stop_all").await;
            let snapshot = conn.status().await;
            let listener = snapshot.pools[0].on_demand.as_ref().unwrap();
            assert!(listener.cleanup_incomplete);
            assert_eq!(listener.registrations.len(), 1);
            assert_eq!(listener.registrations[0].runner_id, Some(1));
            assert_eq!(listener.registrations[0].name, containers(rig)[0]);
            assert_eq!(snapshot.runners_up, 0);
            assert_eq!(service.state.lock().unwrap().registrations.len(), 1);
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn incompatible_remote_labels_refuse_before_session_or_jit() {
    let service = Service::start(1).await;
    service.state.lock().unwrap().reject_labels = true;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn
                .until("label rejection", |snapshot| {
                    snapshot.pools[0]
                        .on_demand
                        .as_ref()
                        .is_some_and(|listener| listener.state == "rejected")
                })
                .await;
            let listener = snapshot.pools[0].on_demand.as_ref().unwrap();
            assert_eq!(listener.reason.as_deref(), Some("scale-set label mismatch"));
            assert_eq!(listener.status, None);
            assert!(containers(rig).is_empty());
            {
                let state = service.state.lock().unwrap();
                assert!(state.owners.is_empty());
                assert!(state.jit_names.is_empty());
            }
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stop_waits_for_late_jit_identity_and_cleans_it_without_launching() {
    let service = Service::start(1).await;
    service.state.lock().unwrap().jit_delay = Duration::from_millis(250);
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.eventually("JIT request in flight", || {
                service.state.lock().unwrap().jit_in_flight
            })
            .await;
            conn.ok("stop_all").await;
            assert!(containers(rig).is_empty());
            {
                let state = service.state.lock().unwrap();
                assert_eq!(state.jit_names.len(), 1);
                assert!(state.registrations.is_empty());
                assert_eq!(state.closed, 1);
            }
            let snapshot = conn.status().await;
            let listener = snapshot.pools[0].on_demand.as_ref().unwrap();
            assert!(!listener.cleanup_incomplete);
            assert!(listener.registrations.is_empty());
            assert_eq!(snapshot.runners_up, 0);
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn reload_preserves_failed_cleanup_instead_of_starting_a_replacement_listener() {
    for configuration in [
        CONFIG.replace("repository = \"o/r\"", "repository = \"o/s\""),
        CONFIG.replace("runners = 2", "runners = 2\ncpus = 3"),
        CONFIG.replace("axpc-jobs", "axpc-replacement"),
        CONFIG.replace("name = \"logical\"", "name = \"other\""),
        CONFIG
            .replace("name = \"logical\"", "name = \"other\"")
            .replace("repository = \"o/r\"", "repository = \"o/s\"")
            .replace("scale_set = \"axpc-jobs\"\n", ""),
    ] {
        let service = Service::start(1).await;
        let rig = rig(&service);
        let (rig, service) = (&rig, &service);
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                conn.until_runner_runs().await;
                rig.mark_running();
                service.state.lock().unwrap().fail_deletion = true;
                rig.write_toml(&configuration);
                let revision = conn.ok("config_read").await["revision"].clone();
                conn.call("config_reload", json!({"revision": revision}))
                    .await
                    .unwrap();
                let snapshot = conn
                    .until("reload cleanup failure", |snapshot| {
                        snapshot
                            .configuration
                            .last_apply
                            .as_ref()
                            .is_some_and(|apply| apply.outcome == "failed")
                    })
                    .await;
                assert!(
                    snapshot
                        .configuration
                        .last_apply
                        .as_ref()
                        .unwrap()
                        .reason
                        .as_ref()
                        .unwrap()
                        .contains("cleanup is incomplete")
                );
                let pool = snapshot
                    .pools
                    .iter()
                    .find(|pool| pool.name == "logical")
                    .unwrap();
                assert_eq!(pool.repository, "o/r");
                assert!(pool.has_token);
                let listener = pool.on_demand.as_ref().unwrap();
                assert_eq!(listener.name, "axpc-jobs");
                assert!(listener.cleanup_incomplete);
                assert_eq!(listener.registrations.len(), 1);
                assert_eq!(snapshot.runners_up, 0);
                {
                    let state = service.state.lock().unwrap();
                    assert_eq!(state.owners.len(), 1);
                    assert_eq!(state.jit_names.len(), 1);
                    assert_eq!(state.registrations.len(), 1);
                }
                conn.ok("stop_all").await;
                service.state.lock().unwrap().assigned = 0;
                conn.ok("start_all").await;
                let snapshot = conn.status().await;
                let pool = snapshot
                    .pools
                    .iter()
                    .find(|pool| pool.name == "logical")
                    .unwrap();
                assert_eq!(pool.repository, "o/r");
                assert!(pool.has_token);
                assert_eq!(pool.on_demand.as_ref().unwrap().registrations.len(), 1);
                assert_eq!(service.state.lock().unwrap().owners.len(), 1);
                conn.ok("stop_all").await;
                service.state.lock().unwrap().fail_deletion = false;
                conn.ok("start_all").await;
                conn.until("original cleanup recovered after reload", |snapshot| {
                    !snapshot.pools.iter().any(|pool| {
                        pool.on_demand
                            .as_ref()
                            .is_some_and(|info| info.cleanup_incomplete)
                    })
                })
                .await;
                assert!(service.state.lock().unwrap().registrations.is_empty());
                conn.ok("quit").await;
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn start_all_retains_exact_pending_cleanup_until_it_can_recover() {
    let service = Service::start(1).await;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            service.state.lock().unwrap().fail_deletion = true;
            conn.ok("stop_all").await;
            let pending = conn.status().await.pools[0].on_demand.clone().unwrap();
            assert_eq!(pending.registrations.len(), 1);
            service.state.lock().unwrap().assigned = 0;
            conn.ok("start_all").await;
            let snapshot = conn.status().await;
            assert_eq!(snapshot.pools[0].on_demand.as_ref(), Some(&pending));
            assert_eq!(service.state.lock().unwrap().owners.len(), 1);
            assert_eq!(service.state.lock().unwrap().jit_names.len(), 1);
            conn.ok("stop_all").await;
            service.state.lock().unwrap().fail_deletion = false;
            conn.client.token_remove("o/r").await.unwrap();
            let before = service.state.lock().unwrap().requests.len();
            conn.ok("start_all").await;
            let snapshot = conn.status().await;
            assert!(
                snapshot.pools[0]
                    .on_demand
                    .as_ref()
                    .unwrap()
                    .cleanup_incomplete
            );
            assert_eq!(service.state.lock().unwrap().requests.len(), before);
            conn.ok("stop_all").await;
            for (kind, word, code) in [
                (StoreErrorKind::NeedsUser, "needs_user", Some(1312)),
                (StoreErrorKind::AccessRefused, "access_refused", Some(5)),
                (StoreErrorKind::Other, "other", None),
            ] {
                rig.store
                    .fail_with_error(Some(StoreError::with(kind, code, "the vault said no")));
                conn.ok("start_all").await;
                let snapshot = conn
                    .until("cleanup store failure", |snapshot| {
                        snapshot.pools[0]
                            .on_demand
                            .as_ref()
                            .unwrap()
                            .store_error_kind
                            .as_deref()
                            == Some(word)
                    })
                    .await;
                let info = snapshot.pools[0].on_demand.as_ref().unwrap();
                assert!(info.cleanup_incomplete);
                assert_eq!(info.registrations, pending.registrations);
                assert_eq!(info.store_error_code, code);
                assert_eq!(service.state.lock().unwrap().requests.len(), before);
                conn.ok("stop_all").await;
            }
            rig.store.fail_with_error(None);
            rig.store_token("o", "r", OTHER_TOKEN);
            conn.ok("start_all").await;
            conn.until(
                "cleanup recovered before replacement listener",
                |snapshot| {
                    snapshot.pools[0]
                        .on_demand
                        .as_ref()
                        .is_some_and(|info| info.state == "listening" && !info.cleanup_incomplete)
                },
            )
            .await;
            assert!(service.state.lock().unwrap().registrations.is_empty());
            assert_eq!(service.state.lock().unwrap().owners.len(), 2);
            assert_eq!(service.state.lock().unwrap().jit_names.len(), 1);
            let cleanup_requests: Vec<_> = service.state.lock().unwrap().requests[before..]
                .iter()
                .filter(|(_, path, _)| path == "/repos/o/r/actions/runners/1")
                .cloned()
                .collect();
            assert!(!cleanup_requests.is_empty());
            assert!(
                cleanup_requests
                    .iter()
                    .all(|(_, _, replacement)| *replacement)
            );
            let snapshot_text = serde_json::to_string(&conn.status().await).unwrap();
            assert!(!snapshot_text.contains(OTHER_TOKEN));
            assert!(
                rig.log
                    .lines()
                    .iter()
                    .all(|line| !line.contains(OTHER_TOKEN))
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn removed_pool_pending_cleanup_remains_visible_after_stop_all() {
    let service = Service::start(1).await;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            service.state.lock().unwrap().fail_deletion = true;
            rig.write_toml(
                &CONFIG
                    .replace("name = \"logical\"", "name = \"other\"")
                    .replace("repository = \"o/r\"", "repository = \"o/s\"")
                    .replace("scale_set = \"axpc-jobs\"\n", ""),
            );
            let revision = conn.ok("config_read").await["revision"].clone();
            conn.call("config_reload", json!({"revision": revision}))
                .await
                .unwrap();
            conn.until("removed pool cleanup failure", |snapshot| {
                snapshot
                    .configuration
                    .last_apply
                    .as_ref()
                    .is_some_and(|apply| apply.outcome == "failed")
            })
            .await;
            conn.ok("stop_all").await;
            let snapshot = conn.status().await;
            let pool = snapshot
                .pools
                .iter()
                .find(|pool| pool.name == "logical")
                .expect("removed pool retains its incomplete registration");
            assert_eq!(pool.repository, "o/r");
            let info = pool.on_demand.as_ref().unwrap();
            assert!(info.cleanup_incomplete);
            assert_eq!(info.registrations.len(), 1);
            assert_eq!(info.registrations[0].runner_id, Some(1));
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cleanup_recovery_settles_unknown_jit_and_deletion_outcomes_without_touching_controls() {
    for lost_jit in [true, false] {
        let service = Service::start(1).await;
        {
            let mut state = service.state.lock().unwrap();
            state.lose_jit_response = lost_jit;
            state.fail_deletion = lost_jit;
            if lost_jit {
                state.jit_delay = Duration::from_millis(250);
            }
        }
        let rig = rig(&service);
        let (rig, service) = (&rig, &service);
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                if lost_jit {
                    conn.eventually("JIT sent before lookup fails", || {
                        service.state.lock().unwrap().jit_in_flight
                    })
                    .await;
                    service.state.lock().unwrap().fail_listing = true;
                    conn.until("unknown JIT identity", |snapshot| {
                        snapshot.pools[0]
                            .on_demand
                            .as_ref()
                            .is_some_and(|info| !info.registrations.is_empty())
                    })
                    .await;
                } else {
                    conn.until_runner_runs().await;
                    rig.mark_running();
                    service.state.lock().unwrap().lose_delete_response = true;
                }
                conn.ok("stop_all").await;
                let snapshot = conn.status().await;
                let info = snapshot.pools[0].on_demand.as_ref().unwrap();
                assert!(info.cleanup_incomplete);
                assert_eq!(info.registrations.len(), 1);
                assert_eq!(
                    info.registrations[0].runner_id,
                    if lost_jit { None } else { Some(1) }
                );
                {
                    let mut state = service.state.lock().unwrap();
                    assert_eq!(state.jit_names.len(), 1);
                    assert_eq!(state.registrations.len(), usize::from(lost_jit));
                    state
                        .registrations
                        .insert(999, "axpc-jobs-extra-1-20261117000000000".to_string());
                    state.assigned = 0;
                    state.lose_jit_response = false;
                    state.lose_delete_response = false;
                    state.fail_deletion = false;
                    state.fail_listing = false;
                    if lost_jit {
                        state.delete_delay = Duration::from_millis(250);
                    }
                }
                rig.store_token("o", "r", OTHER_TOKEN);
                conn.ok("start_all").await;
                if lost_jit {
                    conn.eventually("reconciled identity before interrupted deletion", || {
                        service.state.lock().unwrap().delete_in_flight
                    })
                    .await;
                    conn.ok("stop_all").await;
                    let snapshot = conn.status().await;
                    let info = snapshot.pools[0].on_demand.as_ref().unwrap();
                    assert!(info.cleanup_incomplete);
                    assert_eq!(info.registrations[0].runner_id, Some(1));
                    {
                        let mut state = service.state.lock().unwrap();
                        state.delete_delay = Duration::ZERO;
                        state.requests.clear();
                    }
                    conn.ok("start_all").await;
                }
                conn.until("unknown outcome settled", |snapshot| {
                    snapshot.pools[0]
                        .on_demand
                        .as_ref()
                        .is_some_and(|info| info.state == "listening" && !info.cleanup_incomplete)
                })
                .await;
                {
                    let state = service.state.lock().unwrap();
                    assert_eq!(
                        state.registrations.keys().copied().collect::<Vec<_>>(),
                        [999]
                    );
                    assert_eq!(state.jit_names.len(), 1);
                    assert!(
                        !state
                            .requests
                            .iter()
                            .any(|(method, path, _)| method == "DELETE" && path.ends_with("/999"))
                    );
                    if lost_jit {
                        assert_eq!(
                            state
                                .requests
                                .iter()
                                .find(|(_, path, _)| path.contains("/actions/runners"))
                                .map(|(_, path, _)| path.as_str()),
                            Some("/repos/o/r/actions/runners/1")
                        );
                    }
                }
                conn.ok("quit").await;
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cleanup_recovery_refuses_an_id_whose_exact_name_no_longer_matches() {
    let service = Service::start(1).await;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            service.state.lock().unwrap().fail_deletion = true;
            conn.ok("stop_all").await;
            let pending = conn.status().await.pools[0].on_demand.clone().unwrap();
            {
                let mut state = service.state.lock().unwrap();
                state.fail_deletion = false;
                state.assigned = 0;
                state
                    .registrations
                    .insert(1, "another-pools-runner".to_string());
                state.requests.clear();
            }
            conn.ok("start_all").await;
            conn.eventually("exact ID readback", || {
                service
                    .state
                    .lock()
                    .unwrap()
                    .requests
                    .iter()
                    .any(|(_, path, _)| path.ends_with("/runners/1"))
            })
            .await;
            conn.ok("stop_all").await;
            assert_eq!(
                conn.status().await.pools[0].on_demand.as_ref(),
                Some(&pending)
            );
            {
                let state = service.state.lock().unwrap();
                assert_eq!(state.owners.len(), 1);
                assert_eq!(
                    state.registrations.get(&1).map(String::as_str),
                    Some("another-pools-runner")
                );
                assert!(
                    !state
                        .requests
                        .iter()
                        .any(|(method, _, _)| method == "DELETE")
                );
            }
            service.state.lock().unwrap().registrations.remove(&1);
            conn.ok("start_all").await;
            conn.until("already absent identity settled", |snapshot| {
                snapshot.pools[0]
                    .on_demand
                    .as_ref()
                    .is_some_and(|info| info.state == "listening" && !info.cleanup_incomplete)
            })
            .await;
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_during_recovery_cannot_readmit_the_old_repository() {
    let service = Service::start(1).await;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            service.state.lock().unwrap().fail_deletion = true;
            conn.ok("stop_all").await;
            {
                let mut state = service.state.lock().unwrap();
                state.fail_deletion = false;
                state.delete_delay = Duration::from_millis(250);
                state.assigned = 0;
            }
            conn.ok("start_all").await;
            conn.eventually("cleanup before repository edit", || {
                service.state.lock().unwrap().delete_in_flight
            })
            .await;
            rig.write_toml(&CONFIG.replace("repository = \"o/r\"", "repository = \"o/s\""));
            let revision = conn.ok("config_read").await["revision"].clone();
            conn.call("config_reload", json!({"revision": revision}))
                .await
                .unwrap();
            conn.until("edit waits for cleanup", |snapshot| {
                snapshot
                    .configuration
                    .last_apply
                    .as_ref()
                    .is_some_and(|apply| apply.outcome == "failed")
            })
            .await;
            let snapshot = conn
                .until("recovery admits only the current repository", |snapshot| {
                    let pool = &snapshot.pools[0];
                    pool.repository == "o/s"
                        && pool.state == "skipped"
                        && pool.reason.as_deref() == Some("pool[0]: no token.")
                })
                .await;
            assert!(
                !snapshot.pools[0]
                    .on_demand
                    .as_ref()
                    .unwrap()
                    .cleanup_incomplete
            );
            assert!(service.state.lock().unwrap().registrations.is_empty());
            assert_eq!(service.state.lock().unwrap().owners.len(), 1);
            assert_eq!(service.state.lock().unwrap().jit_names.len(), 1);
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_change_after_failed_cleanup_uses_each_waiting_pools_current_repository() {
    let service = Service::start(1).await;
    let rig = rig(&service);
    let side = "\n[[pool]]\nname = \"side\"\nrepository = \"o/s\"\nrunners = 1\n";
    rig.write_toml(&format!("{CONFIG}{side}"));
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            service.state.lock().unwrap().fail_deletion = true;
            rig.write_toml(&format!(
                "{}{}",
                CONFIG.replace("runners = 2", "runners = 2\ncpus = 3"),
                side.replace("o/s", "o/t")
            ));
            let revision = conn.ok("config_read").await["revision"].clone();
            conn.call("config_reload", json!({"revision": revision}))
                .await
                .unwrap();
            conn.until("cleanup blocks the apply", |snapshot| {
                snapshot
                    .configuration
                    .last_apply
                    .as_ref()
                    .is_some_and(|apply| apply.outcome == "failed")
            })
            .await;
            conn.set_token("o/s", OTHER_TOKEN).await.unwrap();
            conn.eventually("waiting pool readmission after token notification", || {
                !rig.registered_with().is_empty()
                    || rig
                        .log
                        .lines()
                        .iter()
                        .any(|line| line.contains("o/t") && line.contains("no token"))
            })
            .await;
            assert!(rig.registered_with().is_empty());
            let snapshot = conn.status().await;
            let pool = snapshot
                .pools
                .iter()
                .find(|pool| pool.name == "side")
                .unwrap();
            assert_eq!(pool.repository, "o/t");
            assert_eq!(pool.state, "skipped");
            conn.set_token("o/t", OTHER_TOKEN).await.unwrap();
            conn.until(
                "unrelated waiting pool starts with its current token",
                |snapshot| {
                    snapshot
                        .pools
                        .iter()
                        .any(|pool| pool.name == "side" && pool.runners_up == 1)
                },
            )
            .await;
            assert_eq!(rig.registered_with(), [OTHER_TOKEN]);
            assert_eq!(service.state.lock().unwrap().owners.len(), 1);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stopping_recovery_does_not_delay_another_pool_or_extend_stop() {
    let service = Service::start(1).await;
    let rig = rig(&service);
    rig.times.set(marshal_cli::instance::lifecycle::StopTimes {
        exit_deadline: Duration::from_millis(300),
        ..rig.times.get()
    });
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            service.state.lock().unwrap().fail_deletion = true;
            conn.ok("stop_all").await;
            let pending = conn.status().await.pools[0].on_demand.clone().unwrap();
            {
                let mut state = service.state.lock().unwrap();
                state.fail_deletion = false;
                state.delete_delay = Duration::from_secs(30);
            }
            rig.write_toml(&format!(
                "{CONFIG}\n[[pool]]\nname = \"side\"\nrepository = \"o/s\"\nrunners = 1\n"
            ));
            rig.store_token("o", "s", TOKEN);
            conn.ok("start_all").await;
            conn.eventually("registration cleanup in flight", || {
                service.state.lock().unwrap().delete_in_flight
            })
            .await;
            conn.until("unrelated pool running during recovery", |snapshot| {
                snapshot
                    .pools
                    .iter()
                    .any(|pool| pool.name == "side" && pool.runners_up == 1)
            })
            .await;
            rig.mark_running();
            let began = tokio::time::Instant::now();
            conn.ok("stop_all").await;
            assert!(began.elapsed() < Duration::from_millis(700));
            assert_eq!(
                conn.status().await.pools[0].on_demand.as_ref(),
                Some(&pending)
            );
            assert_eq!(service.state.lock().unwrap().owners.len(), 1);
            conn.ok("quit").await;
        })
        .await;
}

/// what one stop-all asked of the backend, and how long it took.
struct StopAsks {
    /// the grace and the budget of each container stop request.
    container_stops: Vec<(Duration, Duration)>,
    releases: Vec<HostCall>,
    took: Duration,
}

/// runs two held runners under a backend that reserves a release budget, with every
/// piece of scale-set cleanup slower than the exit deadline, and stops them.
async fn stop_with_a_reserved_release(config: &str) -> StopAsks {
    let service = Service::start(2).await;
    {
        let mut state = service.state.lock().unwrap();
        state.delete_delay = Duration::from_secs(30);
        state.close_delay = Duration::from_secs(30);
    }
    let mut rig = rig(&service);
    rig.write_toml(config);
    rig.backend.release_budget = RELEASE_BUDGET;
    rig.times.set(StopTimes {
        exit_deadline: EXIT_DEADLINE,
        ..StopTimes::reserving(RELEASE_BUDGET)
    });
    let rig = &rig;
    let (_, took) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("two runners", |snapshot| snapshot.runners_up == 2)
                .await;
            rig.mark_running();
            let began = tokio::time::Instant::now();
            conn.ok("stop_all").await;
            let took = began.elapsed();
            conn.ok("quit").await;
            took
        })
        .await;
    let state = rig.backend.state.borrow();
    let releases = state
        .host_calls
        .iter()
        .filter(|call| matches!(call, HostCall::Release(..)))
        .cloned()
        .collect();
    let container_stops = rig
        .backend
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::StopContainers(request) => Some((request.grace, request.budget)),
            _ => None,
        })
        .collect();
    StopAsks {
        container_stops,
        releases,
        took,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_on_demand_stop_keeps_the_release_budget_and_the_exit_deadline_of_an_always_on_one() {
    let always_on =
        stop_with_a_reserved_release(&CONFIG.replace("scale_set = \"axpc-jobs\"\n", "")).await;
    let on_demand = stop_with_a_reserved_release(CONFIG).await;
    let reserved = StopTimes::reserving(RELEASE_BUDGET);
    assert_eq!(
        on_demand.container_stops,
        [(reserved.grace, reserved.stop_budget)]
    );
    assert_eq!(on_demand.container_stops, always_on.container_stops);
    assert!(!on_demand.releases.is_empty(), "no host was released");
    assert!(
        on_demand
            .releases
            .iter()
            .all(|call| matches!(call, HostCall::Release(_, budget) if *budget == RELEASE_BUDGET)),
        "{:?}",
        on_demand.releases
    );
    assert_eq!(on_demand.releases.len(), always_on.releases.len());
    assert!(
        on_demand.took >= EXIT_DEADLINE,
        "the slow cleanup did not hold the stop: {:?}",
        on_demand.took
    );
    assert!(
        on_demand.took < EXIT_DEADLINE + Duration::from_millis(400),
        "the stop took {:?}",
        on_demand.took
    );
}

#[tokio::test(flavor = "current_thread")]
async fn session_close_recovery_survives_stop_start_and_uses_the_fresh_token() {
    for (status, lose_response, delay) in [
        (Some(500), false, Duration::ZERO),
        (None, true, Duration::ZERO),
        (Some(404), false, Duration::ZERO),
        (Some(500), false, Duration::from_secs(30)),
    ] {
        let service = Service::start(0).await;
        let rig = rig(&service);
        rig.times.set(marshal_cli::instance::lifecycle::StopTimes {
            exit_deadline: Duration::from_millis(300),
            ..rig.times.get()
        });
        let (rig, service) = (&rig, &service);
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                conn.until("initial session", |snapshot| {
                    snapshot.pools[0].on_demand.as_ref().unwrap().state == "listening"
                })
                .await;
                let original = service
                    .state
                    .lock()
                    .unwrap()
                    .active_session
                    .clone()
                    .unwrap();
                {
                    let mut state = service.state.lock().unwrap();
                    state.close_status = status;
                    state.lose_close_response = lose_response;
                    state.close_delay = delay;
                }
                let began = tokio::time::Instant::now();
                assert_eq!(conn.ok("stop_all").await["state"], "stopped");
                assert!(began.elapsed() < Duration::from_millis(700));
                if !delay.is_zero() {
                    assert!(service.state.lock().unwrap().close_in_flight);
                }
                let snapshot = conn.status().await;
                assert_eq!(
                    snapshot.pools[0]
                        .on_demand
                        .as_ref()
                        .unwrap()
                        .cleanup_incomplete,
                    status != Some(404)
                );
                assert!(service.state.lock().unwrap().jit_names.is_empty());
                rig.store_token("o", "r", OTHER_TOKEN);
                let before = service.state.lock().unwrap().requests.len();
                {
                    let mut state = service.state.lock().unwrap();
                    state.lose_close_response = false;
                    state.close_delay = Duration::ZERO;
                    if !delay.is_zero() || status != Some(500) {
                        state.close_status = None;
                    }
                }
                conn.ok("start_all").await;
                if delay.is_zero() && status == Some(500) {
                    let snapshot = conn
                        .until("session cleanup backing off", |snapshot| {
                            snapshot.pools[0].on_demand.as_ref().unwrap().state == "backing_off"
                        })
                        .await;
                    assert!(
                        snapshot.pools[0]
                            .on_demand
                            .as_ref()
                            .unwrap()
                            .cleanup_incomplete
                    );
                    assert_eq!(service.state.lock().unwrap().owners.len(), 1);
                    service.state.lock().unwrap().close_status = None;
                }
                let recovered = conn
                    .until("session cleanup permits listening again", |snapshot| {
                        snapshot.pools[0].on_demand.as_ref().is_some_and(|info| {
                            info.state == "listening" && !info.cleanup_incomplete
                        })
                    })
                    .await;
                {
                    let state = service.state.lock().unwrap();
                    assert_eq!(state.owners.len(), 2);
                    assert_ne!(state.active_session.as_ref(), Some(&original));
                    assert!(state.session_deletes.iter().all(|id| id == &original));
                    assert!(state.registrations.is_empty());
                    assert!(
                        state.requests[before..]
                            .iter()
                            .any(|(_, path, fresh)| path.ends_with("registration-token") && *fresh)
                    );
                }
                let exposed = format!(
                    "{}\n{}",
                    serde_json::to_string(&recovered).unwrap(),
                    rig.log.lines().join("\n")
                );
                for private in [
                    TOKEN,
                    OTHER_TOKEN,
                    JIT,
                    "<queue-placeholder>",
                    "<registration-placeholder>",
                    "fake.eyJleHAiOjQxMDI0NDQ4MDB9.signature",
                    "/tenant",
                    &original,
                ] {
                    assert!(
                        !exposed.contains(private),
                        "private session material escaped"
                    );
                }
                conn.ok("quit").await;
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_session_creation_waits_out_conflict_without_deleting_another_session() {
    for lost_response in [false, true] {
        let service = Service::start(0).await;
        {
            let mut state = service.state.lock().unwrap();
            state.lose_create_response = lost_response;
            state.create_status = (!lost_response).then_some(503);
        }
        let rig = rig(&service);
        let (rig, service) = (&rig, &service);
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                let failed = conn
                    .until("uncertain session create", |snapshot| {
                        snapshot.pools[0].on_demand.as_ref().unwrap().state == "backing_off"
                    })
                    .await;
                assert!(
                    failed.pools[0]
                        .on_demand
                        .as_ref()
                        .unwrap()
                        .cleanup_incomplete
                );
                conn.ok("stop_all").await;
                assert!(
                    conn.status().await.pools[0]
                        .on_demand
                        .as_ref()
                        .unwrap()
                        .cleanup_incomplete
                );
                let foreign = "abcdef01-2345-6789-abcd-ef0123456789";
                {
                    let mut state = service.state.lock().unwrap();
                    state.lose_create_response = false;
                    state.create_status = None;
                    state.active_session = Some(foreign.to_owned());
                }
                conn.ok("start_all").await;
                let conflict = conn
                    .until("another session still holds the set", |snapshot| {
                        snapshot.pools[0].on_demand.as_ref().unwrap().state == "another_listener"
                    })
                    .await;
                assert!(
                    conflict.pools[0]
                        .on_demand
                        .as_ref()
                        .unwrap()
                        .cleanup_incomplete
                );
                {
                    let mut state = service.state.lock().unwrap();
                    assert_eq!(state.active_session.as_deref(), Some(foreign));
                    assert!(state.session_deletes.is_empty());
                    assert!(state.jit_names.is_empty());
                    state.active_session = None;
                }
                let recovered = conn
                    .until(
                        "unknown session expired and listener recovered",
                        |snapshot| {
                            snapshot.pools[0].on_demand.as_ref().is_some_and(|info| {
                                info.state == "listening" && !info.cleanup_incomplete
                            })
                        },
                    )
                    .await;
                {
                    let state = service.state.lock().unwrap();
                    assert_eq!(state.session_deletes.len(), 1);
                    assert_ne!(state.session_deletes[0], foreign);
                    assert_ne!(state.session_deletes.first(), state.active_session.as_ref());
                    assert!(state.jit_names.is_empty());
                }
                let exposed = format!(
                    "{}\n{}",
                    serde_json::to_string(&recovered).unwrap(),
                    rig.log.lines().join("\n")
                );
                assert!(!exposed.contains(foreign));
                assert!(!exposed.contains("01234567-89ab-cdef-0123-"));
                assert!(!exposed.contains("<queue-placeholder>"));
                conn.ok("quit").await;
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn known_session_recovery_never_deletes_the_session_that_replaced_it() {
    let service = Service::start(0).await;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("original listener", |snapshot| {
                snapshot.pools[0].on_demand.as_ref().unwrap().state == "listening"
            })
            .await;
            let original = service
                .state
                .lock()
                .unwrap()
                .active_session
                .clone()
                .unwrap();
            service.state.lock().unwrap().close_status = Some(500);
            conn.ok("stop_all").await;
            let foreign = "abcdef01-2345-6789-abcd-ef0123456789";
            {
                let mut state = service.state.lock().unwrap();
                state.close_status = None;
                state.active_session = Some(foreign.to_owned());
            }
            conn.ok("start_all").await;
            conn.until("replacement session conflict", |snapshot| {
                snapshot.pools[0].on_demand.as_ref().unwrap().state == "another_listener"
            })
            .await;
            {
                let mut state = service.state.lock().unwrap();
                assert_eq!(state.active_session.as_deref(), Some(foreign));
                assert!(state.session_deletes.iter().all(|id| id == &original));
                assert!(state.jit_names.is_empty());
                state.active_session = None;
            }
            conn.until("replacement session expired", |snapshot| {
                snapshot.pools[0]
                    .on_demand
                    .as_ref()
                    .is_some_and(|info| info.state == "listening" && !info.cleanup_incomplete)
            })
            .await;
            assert!(
                service
                    .state
                    .lock()
                    .unwrap()
                    .session_deletes
                    .iter()
                    .all(|id| id == &original)
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn session_recovery_cannot_erase_registration_ownership_after_a_repository_edit() {
    let service = Service::start(1).await;
    let rig = rig(&service);
    let (rig, service) = (&rig, &service);
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            {
                let mut state = service.state.lock().unwrap();
                state.close_status = Some(500);
                state.fail_deletion = true;
                state.assigned = 0;
            }
            conn.ok("stop_all").await;
            let pending = conn.status().await.pools[0].on_demand.clone().unwrap();
            assert!(pending.cleanup_incomplete);
            assert_eq!(pending.registrations.len(), 1);
            rig.write_toml(&CONFIG.replace("o/r", "o/s"));
            rig.store_token("o", "s", TOKEN);
            rig.store_token("o", "r", OTHER_TOKEN);
            let before = service.state.lock().unwrap().requests.len();
            service.state.lock().unwrap().close_status = None;
            conn.ok("start_all").await;
            conn.eventually("original session cleanup settled", || {
                service.state.lock().unwrap().active_session.is_none()
            })
            .await;
            let snapshot = conn.status().await;
            let pool = &snapshot.pools[0];
            assert_eq!(pool.repository, "o/r");
            assert!(pool.on_demand.as_ref().unwrap().cleanup_incomplete);
            assert_eq!(
                pool.on_demand.as_ref().unwrap().registrations,
                pending.registrations
            );
            {
                let state = service.state.lock().unwrap();
                assert_eq!(state.owners.len(), 1);
                assert_eq!(state.registrations.len(), 1);
                assert!(
                    state.requests[before..]
                        .iter()
                        .filter(|(_, path, _)| path.contains("/actions/runners/1")
                            || path.ends_with("registration-token"))
                        .all(|(_, path, fresh)| path.starts_with("/repos/o/r/") && *fresh)
                );
            }
            conn.ok("stop_all").await;
            service.state.lock().unwrap().fail_deletion = false;
            conn.ok("start_all").await;
            conn.until(
                "new repository admitted only after both cleanups",
                |snapshot| {
                    snapshot.pools[0].repository == "o/s"
                        && snapshot.pools[0].on_demand.as_ref().is_some_and(|info| {
                            info.state == "listening" && !info.cleanup_incomplete
                        })
                },
            )
            .await;
            assert!(service.state.lock().unwrap().registrations.is_empty());
            assert_eq!(service.state.lock().unwrap().owners.len(), 2);
            conn.ok("quit").await;
        })
        .await;
}
