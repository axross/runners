//! the agent's `[lifecycle]` settings on fake seams behind its real channel: `launch`
//! follows `start_runners_on_launch`, `lifecycle_set` writes one key through the
//! write path and puts it in effect, the sign-in item follows `start_at_login`, and
//! the snapshot shows what is in effect.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::time::{Duration, Instant};

use marshal_cli::agent::machine::{Launch, NOT_STARTED_AT_LAUNCH};
use marshal_cli::runtime::fake::Call;
use serde_json::json;
use support::agent::{ONE_POOL_TOML, Rig, TOKEN};

/// a rig whose `config.toml` is [`ONE_POOL_TOML`] with `lifecycle` before the pools.
fn rig(lifecycle: &str) -> Rig {
    let rig = Rig::new("");
    rig.remove("config.json");
    rig.write_toml(&ONE_POOL_TOML.replace("\n[[pool]]", &format!("{lifecycle}\n[[pool]]")));
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig
}

fn file(rig: &Rig) -> String {
    std::fs::read_to_string(rig.root.path().join("config.toml")).unwrap()
}

fn containers(rig: &Rig) -> usize {
    rig.backend
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::RunContainer(_)))
        .count()
}

#[tokio::test(flavor = "current_thread")]
async fn launch_with_the_setting_off_stays_stopped_by_you_and_starts_nothing() {
    let rig = rig("\n[lifecycle]\nstart_runners_on_launch = false\n");
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let answer = conn.ok("launch").await;
        assert_eq!(answer["state"], "stopped");
        let snapshot = conn.status().await;
        let reason = snapshot.agent.reason.unwrap();
        assert_eq!(reason.code, "by_you");
        assert_eq!(reason.message, NOT_STARTED_AT_LAUNCH);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(containers(rig), 0, "a runner started at launch");
        // Start all still starts them
        conn.ok("start_all").await;
        conn.until_runner_runs().await;
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn launch_with_the_setting_at_its_default_starts_the_runners() {
    let rig = rig("");
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let snapshot = conn.status().await;
        let settings = snapshot.lifecycle.unwrap().settings;
        let launch = settings["start_runners_on_launch"];
        assert!(launch.on && launch.defaulted);
        conn.ok("launch").await;
        conn.until_runner_runs().await;
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_set_writes_one_key_keeps_the_rest_and_takes_effect_while_running() {
    let rig = rig("\n# my settings\n[lifecycle]\nresume_after_stop = true # later\n");
    let rig = &rig;
    let before = file(rig);
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        assert!(rig.signals.is_held());

        let asked = Instant::now();
        let answer = conn
            .call(
                "lifecycle_set",
                json!({ "key": "keep_awake", "value": false }),
            )
            .await
            .unwrap();
        assert_eq!(answer["lifecycle"]["settings"]["keep_awake"]["on"], false);
        assert_eq!(
            file(rig),
            before.replace(
                "resume_after_stop = true # later\n",
                "resume_after_stop = true # later\nkeep_awake = false\n"
            )
        );
        conn.eventually("the hold ended", || !rig.signals.is_held())
            .await;
        assert!(asked.elapsed() < Duration::from_secs(1));
        let snapshot = conn.status().await;
        assert_eq!(snapshot.agent.state, "running", "a runner was stopped");
        let lifecycle = snapshot.lifecycle.unwrap();
        assert_eq!(lifecycle.keep_awake, "off");
        assert!(!lifecycle.settings["keep_awake"].defaulted);

        // unsetting removes only its line
        conn.call(
            "lifecycle_set",
            json!({ "key": "keep_awake", "value": null }),
        )
        .await
        .unwrap();
        assert_eq!(file(rig), before);
        conn.eventually("the hold taken again", || rig.signals.is_held())
            .await;
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_lifecycle_written_over_several_lines_is_refused_as_the_files_form_and_left_alone() {
    let rig = rig("lifecycle = {\n  keep_awake = true,\n}\n");
    let rig = &rig;
    let before = file(rig);
    rig.drive(Launch::Stopped, |conn| async move {
        let code = conn
            .code(
                "lifecycle_set",
                json!({ "key": "keep_awake", "value": false }),
            )
            .await;
        assert_eq!(code, "config_file_form");
        assert_eq!(file(rig), before);
        assert_eq!(
            conn.code("lifecycle_set", json!({ "key": "later", "value": true }))
                .await,
            "invalid_request"
        );
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_sign_in_item_follows_start_at_login() {
    let rig = rig("\n[lifecycle]\nstart_at_login = true\n");
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        // the agent kept it in step with the file it read at launch
        let command = rig.sign_in.command.borrow().clone().unwrap();
        assert!(command.ends_with("\" --sign-in"), "{command}");
        let lifecycle = conn.status().await.lifecycle.unwrap();
        assert_eq!(lifecycle.sign_in.as_deref(), Some("registered"));

        conn.call(
            "lifecycle_set",
            json!({ "key": "start_at_login", "value": false }),
        )
        .await
        .unwrap();
        assert!(rig.sign_in.command.borrow().is_none());

        *rig.sign_in.failure.borrow_mut() = Some("access is denied".to_string());
        conn.call(
            "lifecycle_set",
            json!({ "key": "start_at_login", "value": true }),
        )
        .await
        .unwrap();
        let lifecycle = conn.status().await.lifecycle.unwrap();
        assert_eq!(lifecycle.sign_in.as_deref(), Some("failed"));
        assert_eq!(lifecycle.sign_in_error.as_deref(), Some("access is denied"));
        // the setting stays as the file says
        assert!(lifecycle.settings["start_at_login"].on);
        conn.ok("quit").await;
    })
    .await;
}
