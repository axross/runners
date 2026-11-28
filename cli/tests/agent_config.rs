//! the configuration methods over the channel: `config_read`, and `config_edit` and
//! `config_reload` on a running agent, which apply a change to the pools it touches
//! alone, and on a stopped one, which only write it; a write on an agent stopped for a
//! missing or invalid file ends that stop.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use marshal_cli::agent::CallError;
use std::time::Duration;

use marshal_cli::agent::machine::Launch;
use marshal_cli::image::check::ImageCheck;
use marshal_cli::instance::lifecycle::StopTimes;
use marshal_cli::runtime::backend::Backend;
use marshal_cli::runtime::fake::Call;
use serde_json::{Value, json};
use support::agent::{Conn, ONE_POOL, ONE_POOL_TOML, Rig, TOKEN, TWO_POOLS};

const TWO_POOLS_TOML: &str = r#"# the host's pools
schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"

# the app's pool
[[pool]]
name = "r"
repository = "o/r"
runners = 2

[[pool]]
name = "d"
repository = "o/d"
runners = 1
"#;

fn rig() -> Rig {
    let rig = Rig::new(ONE_POOL);
    rig.write_toml(TWO_POOLS_TOML);
    rig.store_token("o", "r", TOKEN);
    rig.store_token("o", "d", TOKEN);
    rig.hold_jobs();
    rig
}

fn file(rig: &Rig) -> String {
    std::fs::read_to_string(rig.root.path().join("config.toml")).unwrap()
}

/// the stable code and the error body of a refused call.
async fn refused(conn: &Conn, method: &str, params: Value) -> (String, Vec<String>) {
    match conn.call(method, params).await {
        Err(CallError::Agent(body)) => {
            let paths = body
                .problems
                .iter()
                .filter_map(|problem| problem.path.clone())
                .collect();
            (body.code, paths)
        }
        other => panic!("{method} was not refused: {other:?}"),
    }
}

/// each pool's change, as `pool:change:stops_runners`.
fn changes(answer: &Value) -> Vec<String> {
    answer["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|effect| {
            format!(
                "{}:{}:{}",
                effect["pool"].as_str().unwrap(),
                effect["change"].as_str().unwrap(),
                effect["stops_runners"]
            )
        })
        .collect()
}

/// the job containers the agent asked the engine to stop.
fn stopped(rig: &Rig) -> Vec<String> {
    rig.backend
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::StopContainers(request) => Some(request.names),
            _ => None,
        })
        .flatten()
        .collect()
}

/// the container of each slot that runs, as `pool slot`.
fn workloads(snapshot: &marshal_cli::agent::snapshot::Snapshot) -> Vec<(String, String)> {
    snapshot
        .pools
        .iter()
        .flat_map(|pool| {
            pool.runners.iter().filter_map(|runner| {
                runner
                    .workload
                    .clone()
                    .map(|name| (format!("{} {}", pool.name, runner.slot), name))
            })
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn config_read_sends_the_files_values_and_never_a_value_it_does_not_define() {
    let rig = Rig::new(ONE_POOL);
    rig.write_toml(
        r#"schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"

[[pool]]
name = "r"
repository = "o/r"
runners = "ghp_wrongtype000000000000000000000000"
tokn = "ghp_undefined00000000000000000000000"
"#,
    );
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let read = conn.ok("config_read").await;
            assert!(!read.to_string().contains("ghp_"), "{read}");
            assert_eq!(read["status"], "invalid");
            assert_eq!(read["settings"]["routing_label"], "axpc");
            assert_eq!(read["pools"][0]["name"], "r");
            assert!(read["pools"][0].get("runners").is_none());
            let paths: Vec<&str> = read["problems"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|problem| problem["path"].as_str())
                .collect();
            assert!(paths.contains(&"pool[0].runners"), "{paths:?}");
            assert!(paths.contains(&"pool[0].tokn"), "{paths:?}");
            assert_eq!(read["too_large"], false);
            conn.ok("quit").await;
        })
        .await;
}

/// removing an organisation pool leaves a repository pool and its token in place,
/// and does not report the organisation key as a released repository.
#[tokio::test(flavor = "current_thread")]
async fn removing_an_organization_pool_reports_no_released_repository() {
    let rig = Rig::new(ONE_POOL);
    rig.write_toml(
        r#"schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"
[[pool]]
name = "r"
repository = "o/r"
runners = 1
[[pool]]
name = "org"
organization = "O"
runner_group = "private-workers"
labels = ["org-route"]
runners = 1
"#,
    );
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        conn.client
            .organization_token_set("o", TOKEN)
            .await
            .unwrap();
        conn.until("both pools running", |snapshot| snapshot.runners_up == 2)
            .await;
        rig.mark_running();
        let read = conn.ok("config_read").await;
        assert_eq!(read["pools"][1]["organization"], "O");
        assert_eq!(read["pools"][1]["runner_group"], "private-workers");
        assert!(read["pools"][1].get("repository").is_none());
        let applied = conn
            .call(
                "config_edit",
                json!({
                    "revision": read["revision"],
                    "edits": [{"op":"remove_entry", "path":"pool[1]"}],
                }),
            )
            .await
            .unwrap();
        assert_eq!(applied["repositories_released"], json!([]));
        let snapshot = conn
            .until("only the repository pool remains", |snapshot| {
                snapshot.pools.len() == 1 && snapshot.pools[0].name == "r"
            })
            .await;
        assert!(snapshot.organizations.is_empty());
        assert_eq!(snapshot.repositories[0].repository, "o/r");
        assert!(snapshot.repositories[0].has_token);
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn raising_runners_starts_the_new_slot_alone_and_keeps_the_comments() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            let revision = conn.config_revision().await;
            let edit = json!({
                "revision": revision,
                "edits": [{"op": "set", "path": "pool[1].runners", "value": 2}],
            });
            let mut check = edit.clone();
            check["check"] = json!(true);
            let checked = conn.call("config_edit", check).await.unwrap();
            assert_eq!(
                changes(&checked),
                ["r:unchanged:false", "d:runners_added:false"]
            );
            assert_eq!(file(rig), TWO_POOLS_TOML, "a check wrote the file");

            let applied = conn.call("config_edit", edit).await.unwrap();
            assert_eq!(applied["applies"], true);
            assert_ne!(applied["revision"], json!(revision));
            // only the value changed: the comments and the layout stay
            assert_eq!(
                file(rig),
                TWO_POOLS_TOML.replace("runners = 1", "runners = 2")
            );
            let snapshot = conn
                .until("the new slot running and the apply done", |snapshot| {
                    snapshot.runners_up == 4
                        && snapshot
                            .configuration
                            .last_apply
                            .as_ref()
                            .is_some_and(|apply| apply.outcome == "applied")
                })
                .await;
            assert_eq!(snapshot.pools[1].slots, 2);
            assert_eq!(
                snapshot.configuration.last_apply.unwrap().revision,
                applied["revision"].as_str().unwrap()
            );
            assert!(stopped(rig).is_empty(), "an added slot stopped a runner");
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn lowering_runners_and_removing_a_pool_stop_only_their_slots() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let before = conn
                .until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            let held: std::collections::BTreeMap<_, _> = workloads(&before).into_iter().collect();
            let revision = conn.config_revision().await;
            let applied = conn
                .call(
                    "config_edit",
                    json!({
                        "revision": revision,
                        "edits": [
                            {"op": "set", "path": "pool[0].runners", "value": 1},
                            {"op": "remove_entry", "path": "pool[1]"},
                        ],
                    }),
                )
                .await
                .unwrap();
            assert_eq!(
                changes(&applied),
                ["r:runners_removed:true", "d:removed:true"]
            );
            assert_eq!(applied["repositories_released"], json!(["o/d"]));
            let snapshot = conn
                .until("the apply done", |snapshot| {
                    snapshot
                        .configuration
                        .last_apply
                        .as_ref()
                        .is_some_and(|apply| apply.outcome == "applied")
                })
                .await;
            assert_eq!(snapshot.pools.len(), 1);
            assert_eq!(snapshot.pools[0].runners.len(), 1);
            let stopped = stopped(rig);
            assert!(stopped.contains(&held["r 2"]), "{stopped:?}");
            assert!(stopped.contains(&held["d 1"]), "{stopped:?}");
            assert!(!stopped.contains(&held["r 1"]), "{stopped:?}");
            assert!(!file(rig).contains("o/d"));
            assert!(file(rig).contains("# the app's pool"));
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_edit_writes_nothing_and_says_why() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            let revision = conn.config_revision().await;
            let edit = |path: &str, value: Value| {
                json!({
                    "revision": revision,
                    "edits": [{"op": "set", "path": path, "value": value}],
                })
            };
            assert_eq!(
                refused(&conn, "config_edit", edit("pool[1].runners", json!(0))).await,
                (
                    "config_invalid".to_string(),
                    vec!["pool[1].runners".to_string()]
                )
            );
            assert_eq!(
                refused(&conn, "config_edit", edit("schema_version", json!(2)))
                    .await
                    .0,
                "config_refused"
            );
            rig.backend
                .state
                .borrow_mut()
                .image_answers
                .push_back(ImageCheck::Missing);
            assert_eq!(
                refused(&conn, "config_edit", edit("image_name", json!("other:1")))
                    .await
                    .0,
                "image_unusable"
            );
            assert_eq!(file(rig), TWO_POOLS_TOML);

            // a text editor changes the file after the read
            let changed = TWO_POOLS_TOML.replace("runners = 1", "runners = 3");
            rig.write_toml(&changed);
            assert_eq!(
                refused(&conn, "config_edit", edit("pool[1].runners", json!(2)))
                    .await
                    .0,
                "config_changed"
            );
            assert_eq!(file(rig), changed);
            assert!(stopped(rig).is_empty());
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_on_a_stopped_agent_is_written_and_used_at_the_next_start() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let revision = conn.config_revision().await;
            let applied = conn
                .call(
                    "config_edit",
                    json!({
                        "revision": revision,
                        "edits": [{"op": "set", "path": "pool[0].runners", "value": 3}],
                    }),
                )
                .await
                .unwrap();
            assert_eq!(applied["applies"], false);
            assert_eq!(changes(&applied)[0], "r:runners_added:false");
            let snapshot = conn.status().await;
            assert_eq!(snapshot.agent.state, "stopped");
            assert_eq!(snapshot.pools[0].slots, 3);
            assert!(snapshot.configuration.last_apply.is_none());
            assert!(
                !rig.backend
                    .calls()
                    .iter()
                    .any(|call| matches!(call, Call::RunContainer(_)))
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_clients_config_methods_read_the_agents_answers() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let read = conn.client.config_read().await.unwrap();
            assert_eq!(read.status, "valid");
            assert_eq!(read.settings["routing_label"], "axpc");
            assert_eq!(read.pools.len(), 2);
            assert!(read.problems.is_empty() && !read.too_large);
            let before = file(rig);
            let checked = conn
                .client
                .config_edit(
                    &read.revision,
                    json!([{"op": "remove_entry", "path": "pool[1]"}]),
                    true,
                )
                .await
                .unwrap();
            let d = checked.effects.iter().find(|e| e.pool == "d").unwrap();
            assert_eq!((d.change.as_str(), d.runners_before), ("removed", 1));
            assert_eq!(checked.repositories_released, ["o/d"]);
            assert!(!checked.applies);
            assert_eq!(file(rig), before);
            let reload = conn
                .client
                .config_reload(&read.revision, true)
                .await
                .unwrap();
            assert!(reload.effects.iter().all(|e| e.change == "unchanged"));
            match conn.client.config_reload("stale", true).await {
                Err(CallError::Agent(body)) => assert_eq!(body.code, "config_changed"),
                other => panic!("a stale revision was not refused: {other:?}"),
            }
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_reload_of_a_bad_file_keeps_the_pools_running_and_a_good_one_applies() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            let bad = TWO_POOLS_TOML.replace("runners = 1", "runners = 1\nlimit = 2");
            rig.write_toml(&bad);
            let revision = conn.config_revision().await;
            let (code, paths) = refused(
                &conn,
                "config_reload",
                json!({"revision": revision, "check": true}),
            )
            .await;
            assert_eq!(code, "config_invalid");
            assert_eq!(paths, ["pool[1].limit"]);
            let snapshot = conn.status().await;
            assert_eq!(snapshot.configuration.status, "invalid");
            assert_eq!(snapshot.configuration.in_use.unwrap().source, "last_valid");
            assert_eq!(snapshot.runners_up, 3);

            let good = TWO_POOLS_TOML.replace("runners = 1", "runners = 2");
            rig.write_toml(&good);
            let revision = conn.config_revision().await;
            let checked = conn
                .call(
                    "config_reload",
                    json!({"revision": revision, "check": true}),
                )
                .await
                .unwrap();
            assert_eq!(
                changes(&checked),
                ["r:unchanged:false", "d:runners_added:false"]
            );
            conn.call("config_reload", json!({"revision": revision}))
                .await
                .unwrap();
            let snapshot = conn
                .until("the reload applied", |snapshot| snapshot.runners_up == 4)
                .await;
            assert_eq!(snapshot.configuration.status, "valid");
            assert_eq!(snapshot.configuration.in_use.unwrap().source, "file");
            assert!(stopped(rig).is_empty());
            assert!(!rig.log.lines().join("\n").contains(TOKEN));
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_pool_waiting_for_its_token_takes_its_new_runners_and_starts_with_them() {
    let rig = Rig::new(ONE_POOL);
    rig.write_toml(TWO_POOLS_TOML);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("pool r running", |snapshot| snapshot.runners_up == 2)
                .await;
            for runners in [3, 2] {
                let revision = conn.config_revision().await;
                let applied = conn
                    .call(
                        "config_edit",
                        json!({
                            "revision": revision,
                            "edits": [{"op": "set", "path": "pool[1].runners", "value": runners}],
                        }),
                    )
                    .await
                    .unwrap();
                assert_eq!(applied["applies"], true);
                conn.until("the apply done", |snapshot| {
                    snapshot
                        .configuration
                        .last_apply
                        .as_ref()
                        .is_some_and(|apply| apply.outcome == "applied")
                })
                .await;
            }
            // the waiting pool started nothing without its token
            let snapshot = conn.status().await;
            assert_eq!(snapshot.runners_up, 2, "{snapshot:#?}");
            assert_eq!(snapshot.pools[1].state, "skipped");
            // no slot of it was started to register without a token
            assert!(
                snapshot.pools[1]
                    .runners
                    .iter()
                    .all(|runner| runner.state == "stopped"),
                "{snapshot:#?}"
            );
            conn.set_token("o/d", TOKEN).await.unwrap();
            let snapshot = conn
                .until("pool d running its two runners", |snapshot| {
                    snapshot.pools[1].runners_up == 2
                })
                .await;
            assert_eq!(snapshot.pools[1].runners.len(), 2, "{snapshot:#?}");
            assert_eq!(snapshot.runners_up, 4);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_changed_pool_restarts_alone() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let before = conn
                .until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            let held: std::collections::BTreeMap<_, _> = workloads(&before).into_iter().collect();
            let revision = conn.config_revision().await;
            let applied = conn
                .call(
                    "config_edit",
                    json!({
                        "revision": revision,
                        "edits": [{"op": "set", "path": "pool[0].labels", "value": ["big"]}],
                    }),
                )
                .await
                .unwrap();
            assert_eq!(changes(&applied), ["r:restarted:true", "d:unchanged:false"]);
            let after = conn
                .until("pool r running again", |snapshot| {
                    snapshot.runners_up == 3
                        && snapshot
                            .configuration
                            .last_apply
                            .as_ref()
                            .is_some_and(|apply| apply.outcome == "applied")
                })
                .await;
            let stopped = stopped(rig);
            assert!(stopped.contains(&held["r 1"]), "{stopped:?}");
            assert!(stopped.contains(&held["r 2"]), "{stopped:?}");
            assert!(!stopped.contains(&held["d 1"]), "{stopped:?}");
            let now: std::collections::BTreeMap<_, _> = workloads(&after).into_iter().collect();
            assert_eq!(now["d 1"], held["d 1"], "pool d was restarted");
            assert!(last_labels(rig, "r").contains(&"big".to_string()));
            assert_eq!(after.pools[0].runners.len(), 2);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn removing_a_pool_with_diagnostics_exports_its_evidence_first() {
    let rig = Rig::new(ONE_POOL);
    rig.write_json(&TWO_POOLS.replacen(
        r#""volumes":[]}"#,
        r#""volumes":[],"diagnostics":true,"diagnosticDirectory":"C:\\diag"}"#,
        1,
    ));
    rig.store_token("o", "r", TOKEN);
    rig.store_token("o", "d", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let before = conn
                .until("two runners", |snapshot| snapshot.runners_up == 2)
                .await;
            rig.mark_running();
            let held: std::collections::BTreeMap<_, _> = workloads(&before).into_iter().collect();
            let revision = conn.config_revision().await;
            conn.call(
                "config_edit",
                json!({
                    "revision": revision,
                    "edits": [{"op": "remove_entry", "path": "pool[0]"}],
                }),
            )
            .await
            .unwrap();
            conn.until("the apply done", |snapshot| {
                snapshot
                    .configuration
                    .last_apply
                    .as_ref()
                    .is_some_and(|apply| apply.outcome == "applied")
            })
            .await;
            let exported: Vec<String> = rig
                .backend
                .calls()
                .into_iter()
                .filter_map(|call| match call {
                    Call::ExportDiagnostic(name, _) => Some(name),
                    _ => None,
                })
                .collect();
            assert_eq!(exported, [held["r 1"].clone()]);
            assert!(rig.backend.retained().names().is_empty());
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_slot_whose_stop_runs_late_is_finished_when_its_runner_ends() {
    let rig = rig();
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(1),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            rig.backend.state.borrow_mut().stop_hangs = true;
            let lower = |runners: u32, revision: String| {
                json!({
                    "revision": revision,
                    "edits": [{"op": "set", "path": "pool[0].runners", "value": runners}],
                })
            };
            conn.call("config_edit", lower(1, conn.config_revision().await))
                .await
                .unwrap();
            // the apply waits on the stop, so another edit meanwhile is refused
            assert_eq!(
                refused(&conn, "config_edit", lower(2, conn.config_revision().await))
                    .await
                    .0,
                "config_applying"
            );
            assert_eq!(
                refused(
                    &conn,
                    "lifecycle_set",
                    json!({ "key": "keep_awake", "value": false })
                )
                .await
                .0,
                "busy"
            );
            // the stop hangs past the deadline, so slot 2 stays listed as leaving
            let late = conn
                .until("the apply done", |snapshot| {
                    snapshot
                        .configuration
                        .last_apply
                        .as_ref()
                        .is_some_and(|apply| apply.outcome == "applied")
                })
                .await;
            assert_eq!(late.pools[0].runners.len(), 2, "{late:#?}");
            assert_eq!(late.pools[0].runners[1].state, "stopped");

            // its job ends, so its runner ends and the slot leaves the list
            rig.backend.state.borrow_mut().stop_hangs = false;
            rig.end_jobs().await;
            conn.until("slot 2 gone", |snapshot| {
                snapshot.pools[0].runners.len() == 1
            })
            .await;
            // and raising the runners again starts it
            conn.call("config_edit", lower(2, conn.config_revision().await))
                .await
                .unwrap();
            conn.until("slot 2 running again", |snapshot| {
                snapshot.pools[0]
                    .runners
                    .iter()
                    .any(|runner| runner.slot == 2 && runner.state == "running")
            })
            .await;
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

/// sets `path` to `value` in the file as it is now.
async fn set(conn: &Conn, path: &str, value: Value) -> Result<Value, CallError> {
    let revision = conn.config_revision().await;
    conn.call(
        "config_edit",
        json!({
            "revision": revision,
            "edits": [{"op": "set", "path": path, "value": value}],
        }),
    )
    .await
}

async fn until_applied(conn: &Conn) -> marshal_cli::agent::snapshot::Snapshot {
    conn.until("the apply done", |snapshot| {
        snapshot
            .configuration
            .last_apply
            .as_ref()
            .is_some_and(|apply| apply.outcome == "applied")
    })
    .await
}

/// the labels of the latest registration of `pool`'s runners.
fn last_labels(rig: &Rig, pool: &str) -> Vec<String> {
    rig.registrar
        .seen
        .borrow()
        .iter()
        .rev()
        .find(|seen| seen.name.starts_with(&format!("{pool}-")))
        .map(|seen| seen.labels.clone())
        .unwrap_or_default()
}

#[tokio::test(flavor = "current_thread")]
async fn added_pools_start_or_wait_and_a_routing_label_restarts_the_pools_without_labels() {
    let rig = Rig::new(ONE_POOL);
    rig.write_toml(
        r#"schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"

[[pool]]
name = "r"
repository = "o/r"
runners = 1
labels = ["big"]

[[pool]]
name = "d"
repository = "o/d"
runners = 1
"#,
    );
    for name in ["r", "d", "n"] {
        rig.store_token("o", name, TOKEN);
    }
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("two runners", |snapshot| snapshot.runners_up == 2)
                .await;
            rig.mark_running();
            let revision = conn.config_revision().await;
            let applied = conn
                .call(
                    "config_edit",
                    json!({
                        "revision": revision,
                        "edits": [
                            {"op": "add_entry", "array": "pool",
                             "values": {"name": "n", "repository": "o/n", "runners": 1}},
                            {"op": "add_entry", "array": "pool",
                             "values": {"name": "m", "repository": "o/m", "runners": 1}},
                        ],
                    }),
                )
                .await
                .unwrap();
            assert_eq!(
                changes(&applied),
                [
                    "r:unchanged:false",
                    "d:unchanged:false",
                    "n:added:false",
                    "m:added:false"
                ]
            );
            let snapshot = conn
                .until("pool n running", |snapshot| snapshot.runners_up == 3)
                .await;
            assert_eq!(snapshot.pools[3].name, "m");
            assert_eq!(snapshot.pools[3].state, "skipped");
            assert!(stopped(rig).is_empty());
            rig.mark_running();

            let before: std::collections::BTreeMap<_, _> =
                workloads(&conn.status().await).into_iter().collect();
            let applied = set(&conn, "routing_label", json!("other")).await.unwrap();
            assert_eq!(
                changes(&applied),
                [
                    "r:unchanged:false",
                    "d:restarted:true",
                    "n:restarted:true",
                    "m:restarted:false"
                ]
            );
            until_applied(&conn).await;
            conn.until("d and n running again", |snapshot| snapshot.runners_up == 3)
                .await;
            let stopped = stopped(rig);
            assert!(!stopped.contains(&before["r 1"]), "{stopped:?}");
            assert!(stopped.contains(&before["d 1"]), "{stopped:?}");
            assert!(last_labels(rig, "d").contains(&"other".to_string()));
            assert!(!last_labels(rig, "r").contains(&"other".to_string()));
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn on_a_stopped_agent_an_image_is_written_unchecked_and_build_uses_it() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let applied = set(&conn, "image_name", json!("other:2")).await.unwrap();
            assert_eq!(applied["applies"], false);
            assert!(rig.backend.state.borrow().image_checks.is_empty());
            conn.ok("build").await;
            conn.eventually("the build", || !rig.builder.images.borrow().is_empty())
                .await;
            assert_eq!(*rig.builder.images.borrow(), ["other:2"]);
            conn.ok("quit").await;
        })
        .await;
}

/// whether this process can create a file in a folder set to 0o500. the kernel refuses
/// that to an ordinary user but not to root (on Linux, to a process that holds
/// CAP_DAC_OVERRIDE), which a cloud container runs the tests as.
#[cfg(unix)]
fn can_write_into_a_read_only_folder() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let beside = support::TempDir::new();
    let folder = beside.path().join("read-only-probe");
    std::fs::create_dir(&folder).expect("the read-only probe folder could not be created");
    let written = std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500))
        .map(|()| std::fs::File::create(folder.join("probe")).is_ok());
    let _ = std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700));
    written.expect("the read-only probe folder could not be made read-only")
}

#[tokio::test(flavor = "current_thread")]
async fn a_bad_diagnostic_directory_and_a_file_that_cannot_be_replaced_are_refused() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let edit = |path: &str, value: Value| {
                let conn = &conn;
                let path = path.to_string();
                async move {
                    match set(conn, &path, value).await {
                        Err(CallError::Agent(body)) => (
                            body.code,
                            body.problems
                                .iter()
                                .filter_map(|problem| problem.path.clone())
                                .collect::<Vec<_>>(),
                        ),
                        other => panic!("{path} was not refused: {other:?}"),
                    }
                }
            };
            assert_eq!(
                edit("pool[0].diagnostic_directory", json!("relative/dir")).await,
                (
                    "config_invalid".to_string(),
                    vec!["pool[0].diagnostic_directory".to_string()]
                )
            );
            // a well-formed directory that fails the sink checks: it does not exist
            let body = match conn
                .call(
                    "config_edit",
                    json!({
                        "revision": conn.config_revision().await,
                        "edits": [
                            {"op": "set", "path": "pool[0].diagnostics", "value": true},
                            {"op": "set", "path": "pool[0].diagnostic_directory",
                             "value": "C:\\no-such-sink"},
                        ],
                    }),
                )
                .await
            {
                Err(CallError::Agent(body)) => body,
                other => panic!("the sink was not refused: {other:?}"),
            };
            assert_eq!(body.code, "config_invalid");
            assert_eq!(
                body.problems[0].path.as_deref(),
                Some("pool[0].diagnostic_directory")
            );
            assert!(
                body.problems[0].message.contains("does not exist"),
                "{body:?}"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if can_write_into_a_read_only_folder() {
                    assert!(
                        std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true"),
                        "the read-only-folder step cannot run here: this process can write into a read-only folder"
                    );
                    eprintln!(
                        "note: the read-only-folder step is skipped because this process can write into a read-only folder"
                    );
                } else {
                    let folder = rig.root.path();
                    let mode = std::fs::metadata(folder).unwrap().permissions().mode();
                    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o500))
                        .unwrap();
                    let refused = edit("pool[1].runners", json!(2)).await;
                    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode))
                        .unwrap();
                    assert_eq!(refused.0, "config_unwritable");
                }
            }
            assert_eq!(file(rig), TWO_POOLS_TOML);
            // the same sink written by hand: a reload marks the file invalid, keeps the
            // configuration in use, and names the key's line
            rig.write_toml(&TWO_POOLS_TOML.replace(
                "runners = 2\n",
                "runners = 2\ndiagnostics = true\ndiagnostic_directory = 'C:\\no-such-sink'\n",
            ));
            let body = match conn
                .call(
                    "config_reload",
                    json!({"revision": conn.config_revision().await}),
                )
                .await
            {
                Err(CallError::Agent(body)) => body,
                other => panic!("the reload was not refused: {other:?}"),
            };
            assert_eq!(body.code, "config_invalid");
            assert_eq!(
                body.problems[0].path.as_deref(),
                Some("pool[0].diagnostic_directory")
            );
            assert_eq!(body.problems[0].line, Some(12));
            let snapshot = conn.status().await;
            assert_eq!(snapshot.configuration.status, "invalid");
            assert!(
                snapshot.configuration.problems[0]
                    .starts_with("line 12: pool[0].diagnostic_directory: "),
                "{:?}",
                snapshot.configuration.problems
            );
            assert_eq!(snapshot.configuration.in_use.unwrap().source, "last_valid");
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_is_busy_while_the_agent_resumes() {
    let rig = rig();
    let gate = std::rc::Rc::new(tokio::sync::Notify::new());
    rig.backend.state.borrow_mut().available_gate = Some(gate.clone());
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_state("resuming").await;
            assert_eq!(
                refused(
                    &conn,
                    "config_reload",
                    json!({"revision": conn.config_revision().await})
                )
                .await
                .0,
                "busy"
            );
            assert_eq!(file(rig), TWO_POOLS_TOML);
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_is_refused_while_the_runtime_waits_to_restart() {
    let rig = rig();
    rig.clock.pause.set(Duration::from_millis(400));
    rig.backend.state.borrow_mut().on_run = Some(Box::new(|| panic!("the core broke")));
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("a restart due", |s| s.agent.restart_due.is_some())
                .await;
            let (code, _) = refused(
                &conn,
                "config_edit",
                json!({
                    "revision": conn.config_revision().await,
                    "edits": [{"op": "set", "path": "pool[1].runners", "value": 2}],
                }),
            )
            .await;
            assert_eq!(code, "runtime_restarting");
            assert_eq!(file(rig), TWO_POOLS_TOML);
            conn.ok("quit").await;
        })
        .await;
}

/// how often the agent listed `pool`'s containers, as a preparation does.
fn listings(rig: &Rig, pool: &str) -> usize {
    rig.backend
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::ListPoolContainers(name) if name == pool))
        .count()
}

#[tokio::test(flavor = "current_thread")]
async fn a_restart_waits_for_its_held_slots_before_it_prepares_the_pool() {
    let rig = rig();
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(1),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            rig.backend.state.borrow_mut().stop_hangs = true;
            let prepared = listings(rig, "r");
            set(&conn, "pool[0].labels", json!(["big"])).await.unwrap();
            // both slots' stops run late, so the pool is not prepared over them
            let held = until_applied(&conn).await;
            assert_eq!(held.pools[0].state, "skipped", "{held:#?}");
            assert_eq!(listings(rig, "r"), prepared);

            rig.backend.state.borrow_mut().stop_hangs = false;
            rig.end_jobs().await;
            let after = conn
                .until("pool r running again", |snapshot| {
                    snapshot.pools[0].runners_up == 2
                })
                .await;
            assert_eq!(after.pools[0].state, "running");
            assert_eq!(listings(rig, "r"), prepared + 1);
            assert!(last_labels(rig, "r").contains(&"big".to_string()));
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_slot_that_keeps_evidence_holds_its_restarted_pool_until_the_next_start() {
    use marshal_cli::runtime::backend::DiagnosticRecord;
    let rig = rig();
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(1),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            rig.backend.state.borrow_mut().stop_hangs = true;
            let prepared = listings(rig, "r");
            set(&conn, "pool[0].labels", json!(["big"])).await.unwrap();
            until_applied(&conn).await;
            // slot 1's host still holds a diagnostic container when its runner ends
            rig.backend.retained.adopt(
                "r-1-20261112091204118",
                &DiagnosticRecord {
                    pool: "r".to_string(),
                    directory: rig.root.path().to_str().unwrap().to_string(),
                    raw_records: false,
                },
            );
            rig.backend.state.borrow_mut().stop_hangs = false;
            rig.end_jobs().await;
            let held = conn
                .until("slot 1 kept for its evidence", |_| {
                    rig.log
                        .lines()
                        .iter()
                        .any(|line| line.contains("r slot 1 keeps its job host"))
                })
                .await;
            assert_eq!(held.pools[0].state, "skipped", "{held:#?}");
            assert!(
                held.pools[0]
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("until the next Start all")),
                "{held:#?}"
            );
            assert_eq!(listings(rig, "r"), prepared);
            assert!(!last_labels(rig, "r").contains(&"big".to_string()));
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_deferred_pool_is_not_prepared_during_a_drain_and_is_once_it_is_cancelled() {
    use marshal_cli::runtime::backend::StopRequest;
    let rig = rig();
    rig.backend.state.borrow_mut().hold_runs_by_name = true;
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(1),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            let names: Vec<String> = rig
                .backend
                .calls()
                .into_iter()
                .filter_map(|call| match call {
                    Call::RunContainer(spec) => Some(spec.name),
                    _ => None,
                })
                .collect();
            let (pool_r, pool_d): (Vec<_>, Vec<_>) =
                names.into_iter().partition(|name| name.starts_with("r-"));
            // a busy runner keeps the drain open
            rig.backend
                .print(&pool_d[0], "2026-11-12 08:03:00Z: Running job: build");
            rig.backend.state.borrow_mut().stop_hangs = true;
            let prepared = listings(rig, "r");
            set(&conn, "pool[0].labels", json!(["big"])).await.unwrap();
            until_applied(&conn).await;
            let requester = conn.another().await;
            requester
                .call("drain_for_update", json!({ "version": "1.4.0" }))
                .await
                .unwrap();
            rig.backend.state.borrow_mut().stop_hangs = false;
            rig.backend
                .stop_containers(&StopRequest {
                    names: pool_r,
                    grace: Duration::ZERO,
                    budget: Duration::from_secs(1),
                })
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(600)).await;
            assert_eq!(listings(rig, "r"), prepared, "prepared during the drain");
            drop(requester);
            conn.until("the drain cancelled", |s| s.agent.update.is_none())
                .await;
            conn.eventually("pool r prepared", || listings(rig, "r") > prepared)
                .await;
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

/// restarts both pools over late stops, then ends only `ending`'s runners: that pool
/// is prepared again while the other's late slots still run.
async fn one_pool_of_two_late_ones_ends(ending: &str) {
    use marshal_cli::runtime::backend::StopRequest;
    let held = if ending == "r" { "d" } else { "r" };
    let rig = rig();
    rig.backend.state.borrow_mut().hold_runs_by_name = true;
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(1),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            let prefix = format!("{ending}-");
            let names: Vec<String> = rig
                .backend
                .calls()
                .into_iter()
                .filter_map(|call| match call {
                    Call::RunContainer(spec) if spec.name.starts_with(&prefix) => Some(spec.name),
                    _ => None,
                })
                .collect();
            rig.backend.state.borrow_mut().stop_hangs = true;
            let (ending_before, held_before) = (listings(rig, ending), listings(rig, held));
            conn.call(
                "config_edit",
                json!({
                    "revision": conn.config_revision().await,
                    "edits": [
                        {"op": "set", "path": "pool[0].labels", "value": ["big"]},
                        {"op": "set", "path": "pool[1].labels", "value": ["big"]},
                    ],
                }),
            )
            .await
            .unwrap();
            until_applied(&conn).await;
            rig.backend.state.borrow_mut().stop_hangs = false;
            rig.backend
                .stop_containers(&StopRequest {
                    names,
                    grace: Duration::ZERO,
                    budget: Duration::from_secs(1),
                })
                .await
                .unwrap();
            conn.eventually("the pool whose runners ended prepared again", || {
                listings(rig, ending) > ending_before
            })
            .await;
            assert_eq!(listings(rig, held), held_before);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_late_slot_that_ends_is_finished_while_another_pools_late_slot_runs_on() {
    one_pool_of_two_late_ones_ends("r").await;
    one_pool_of_two_late_ones_ends("d").await;
}

/// the slot numbers `pool` lists, or `None` when the snapshot does not list it.
fn listed(snapshot: &marshal_cli::agent::snapshot::Snapshot, pool: &str) -> Option<Vec<u32>> {
    snapshot
        .pools
        .iter()
        .find(|info| info.name == pool)
        .map(|info| info.runners.iter().map(|runner| runner.slot).collect())
}

#[tokio::test(flavor = "current_thread")]
async fn every_late_slot_is_listed_and_a_removed_pool_stays_listed_after_the_apply() {
    use marshal_cli::runtime::backend::StopRequest;
    let rig = rig();
    rig.backend.state.borrow_mut().hold_runs_by_name = true;
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(1),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            set(&conn, "pool[0].runners", json!(3)).await.unwrap();
            let before = conn
                .until("four runners", |snapshot| snapshot.runners_up == 4)
                .await;
            rig.mark_running();
            let held: std::collections::BTreeMap<_, _> = workloads(&before).into_iter().collect();
            rig.backend.state.borrow_mut().stop_hangs = true;
            conn.call(
                "config_edit",
                json!({
                    "revision": conn.config_revision().await,
                    "edits": [
                        {"op": "set", "path": "pool[0].runners", "value": 1},
                        {"op": "remove_entry", "path": "pool[1]"},
                    ],
                }),
            )
            .await
            .unwrap();
            let late = until_applied(&conn).await;
            assert_eq!(listed(&late, "r"), Some(vec![1, 2, 3]), "{late:#?}");
            // the removed pool is listed after its apply ended, while its slot runs
            assert_eq!(listed(&late, "d"), Some(vec![1]), "{late:#?}");

            // slot 2 ends, so slot 3 is listed above the gap it leaves
            rig.backend.state.borrow_mut().stop_hangs = false;
            rig.backend
                .stop_containers(&StopRequest {
                    names: vec![held["r 2"].clone()],
                    grace: Duration::ZERO,
                    budget: Duration::from_secs(1),
                })
                .await
                .unwrap();
            let gap = conn
                .until("slot 2 gone", |snapshot| {
                    listed(snapshot, "r") != Some(vec![1, 2, 3])
                })
                .await;
            assert_eq!(listed(&gap, "r"), Some(vec![1, 3]), "{gap:#?}");
            assert_eq!(listed(&gap, "d"), Some(vec![1]), "{gap:#?}");

            rig.end_jobs().await;
            let gone = conn
                .until("every late slot gone", |snapshot| {
                    listed(snapshot, "d").is_none() && listed(snapshot, "r") == Some(vec![1])
                })
                .await;
            assert_eq!(gone.pools.len(), 1);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_valid_reload_on_a_stopped_agent_makes_the_file_the_one_in_use() {
    let rig = rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            rig.write_toml(&TWO_POOLS_TOML.replace("runners = 1", "runners = 2"));
            let reloaded = conn
                .call(
                    "config_reload",
                    json!({"revision": conn.config_revision().await}),
                )
                .await
                .unwrap();
            assert_eq!(reloaded["applies"], false);
            assert_eq!(
                changes(&reloaded),
                ["r:unchanged:false", "d:runners_added:false"]
            );
            let snapshot = conn.status().await;
            assert_eq!(snapshot.agent.state, "stopped");
            assert_eq!(snapshot.configuration.status, "valid");
            assert_eq!(snapshot.configuration.in_use.unwrap().source, "file");
            assert_eq!(snapshot.pools[1].slots, 2);
            assert!(
                !rig.backend
                    .calls()
                    .iter()
                    .any(|call| matches!(call, Call::RunContainer(_)))
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_and_a_reload_are_busy_while_the_agent_stops() {
    let rig = rig();
    rig.backend.state.borrow_mut().stop_hangs = true;
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(2),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("three runners", |snapshot| snapshot.runners_up == 3)
                .await;
            rig.mark_running();
            // the stop hangs until the exit deadline, so the calls are made on another
            // connection
            let other = conn.another().await;
            let stop = async { conn.ok("stop_all").await };
            let meanwhile = async {
                other.until_state("stopping").await;
                let revision = other.config_revision().await;
                assert_eq!(
                    refused(
                        &other,
                        "config_edit",
                        json!({
                            "revision": revision,
                            "edits": [{"op": "set", "path": "pool[0].runners", "value": 3}],
                        })
                    )
                    .await
                    .0,
                    "busy"
                );
                assert_eq!(
                    refused(&other, "config_reload", json!({"revision": revision}))
                        .await
                        .0,
                    "busy"
                );
            };
            tokio::join!(stop, meanwhile);
            assert_eq!(file(rig), TWO_POOLS_TOML);
            conn.ok("quit").await;
        })
        .await;
}

/// a rig with no `config.toml` and no `config.json`, or with `file` as its `config.toml`.
fn without_a_usable_file(file: Option<&str>) -> Rig {
    let rig = Rig::new(ONE_POOL);
    rig.remove("config.json");
    if let Some(file) = file {
        rig.write_toml(file);
    }
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig
}

/// the stop reason code and message the snapshot names.
async fn stop_of(conn: &Conn) -> (String, String) {
    let reason = conn.status().await.agent.reason.unwrap();
    (reason.code, reason.message)
}

/// a reload, or an edit of the first pool's runners, at the file's current revision.
async fn write_change(conn: &Conn, edit: bool) -> Result<Value, CallError> {
    if edit {
        conn.edit_first_pool_runners(false).await
    } else {
        conn.reload_config(false).await
    }
}

/// the file the agent was stopped for is fixed, and an edit or a reload is written:
/// the agent is stopped by you, shows the file as the configuration in use, and
/// starts the runners at the next Start all.
async fn a_fixed_file_ends_its_stop(file: Option<&str>, stopped_for: &str, edit: bool) {
    let rig = without_a_usable_file(file);
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_state("stopped").await;
        assert_eq!(stop_of(&conn).await.0, stopped_for);
        // the file is not fixed yet, so the change is refused and the stop is as it was
        let (code, _) = refused(
            &conn,
            "config_reload",
            json!({"revision": conn.config_revision().await}),
        )
        .await;
        assert!(code.starts_with("config_"), "{code}");
        assert_eq!(stop_of(&conn).await.0, stopped_for);

        rig.write_toml(ONE_POOL_TOML);
        write_change(&conn, edit).await.unwrap();
        let snapshot = conn.status().await;
        assert_eq!(snapshot.agent.state, "stopped");
        let reason = snapshot.agent.reason.unwrap();
        assert_eq!(
            (reason.code.as_str(), reason.message.as_str()),
            ("by_you", "stopped by you")
        );
        assert_eq!(snapshot.configuration.status, "valid");
        assert_eq!(snapshot.configuration.in_use.unwrap().source, "file");
        assert!(
            !rig.backend
                .calls()
                .iter()
                .any(|call| matches!(call, Call::RunContainer(_))),
            "a runner started"
        );

        conn.ok("start_all").await;
        conn.until_runner_runs().await;
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_reload_that_finds_a_missing_file_created_ends_the_stop_for_it() {
    a_fixed_file_ends_its_stop(None, "no_configuration", false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_that_finds_a_missing_file_created_ends_the_stop_for_it() {
    a_fixed_file_ends_its_stop(None, "no_configuration", true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_reload_that_finds_an_invalid_file_fixed_ends_the_stop_for_it() {
    a_fixed_file_ends_its_stop(Some("not = [toml"), "invalid_configuration", false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_that_finds_an_invalid_file_fixed_ends_the_stop_for_it() {
    a_fixed_file_ends_its_stop(Some("not = [toml"), "invalid_configuration", true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_for_a_config_json_rejected_at_its_migration_is_not_ended() {
    let rig = Rig::new(&ONE_POOL.replace("\"slots\":1", "\"slots\":0"));
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_state("stopped").await;
        let before = stop_of(&conn).await;
        assert_eq!(before.0, "invalid_configuration");
        // a valid config.toml beside it is not read until Start all, so the change is
        // refused
        rig.write_toml(ONE_POOL_TOML);
        for edit in [false, true] {
            let refusal = write_change(&conn, edit).await;
            let Err(CallError::Agent(body)) = refusal else {
                panic!("the change was not refused: {refusal:?}");
            };
            assert_eq!(body.code, "config_unusable");
            assert_eq!(stop_of(&conn).await, before);
        }
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_written_change_keeps_every_other_stop_reason() {
    // stopped by you at launch, and for an image that is not usable
    let rig = without_a_usable_file(Some(ONE_POOL_TOML));
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let by_you = stop_of(&conn).await;
        assert_eq!(by_you.0, "by_you");
        write_change(&conn, false).await.unwrap();
        assert_eq!(stop_of(&conn).await, by_you);

        rig.backend
            .state
            .borrow_mut()
            .image_answers
            .push_back(marshal_cli::image::check::ImageCheck::Missing);
        let answer = conn.ok("start_all").await;
        assert_eq!(answer["reason"]["code"], "image_unusable");
        let unusable = stop_of(&conn).await;
        write_change(&conn, true).await.unwrap();
        assert_eq!(stop_of(&conn).await, unusable);
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_written_change_keeps_a_stop_for_no_pool_started() {
    // no token is stored, so the only pool is skipped
    let rig = Rig::new(ONE_POOL);
    rig.remove("config.json");
    rig.write_toml(ONE_POOL_TOML);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_state("stopped").await;
        let stopped = stop_of(&conn).await;
        assert_eq!(stopped.0, "no_pool_started");
        for edit in [false, true] {
            write_change(&conn, edit).await.unwrap();
            assert_eq!(stop_of(&conn).await, stopped, "edit: {edit}");
        }
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_written_change_keeps_a_stop_for_restart_limit() {
    // the core fails each time a job container is run, so it stops at the fourth failure
    let rig = Rig::new(ONE_POOL);
    rig.remove("config.json");
    rig.write_toml(ONE_POOL_TOML);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig.clock.pause.set(Duration::from_millis(400));
    rig.backend.state.borrow_mut().on_run = Some(Box::new(|| panic!("the core broke")));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_state("stopped").await;
        let stopped = stop_of(&conn).await;
        assert_eq!(stopped.0, "restart_limit");
        for edit in [false, true] {
            write_change(&conn, edit).await.unwrap();
            assert_eq!(stop_of(&conn).await, stopped, "edit: {edit}");
        }
        conn.ok("quit").await;
    })
    .await;
}
