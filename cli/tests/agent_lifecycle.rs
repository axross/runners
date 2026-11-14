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
        assert!(!settings["start_at_login"].on && settings["start_at_login"].defaulted);
        let lifecycle = conn.status().await.lifecycle.unwrap();
        assert_eq!(lifecycle.sign_in.as_deref(), Some("not_registered"));
        assert!(rig.sign_in.command.borrow().is_none());
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
async fn a_start_all_that_keeps_the_last_valid_configuration_keeps_a_setting_written_since() {
    let rig = rig("");
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        conn.call(
            "lifecycle_set",
            json!({ "key": "keep_awake", "value": false }),
        )
        .await
        .unwrap();
        conn.eventually("the hold ended", || !rig.signals.is_held())
            .await;
        conn.ok("stop_all").await;
        rig.write_toml(&file(rig).replace("runners = 1", "runners = 0"));
        conn.ok("start_all").await;
        let snapshot = conn.until_runner_runs().await;
        assert_eq!(snapshot.configuration.in_use.unwrap().source, "last_valid");
        let lifecycle = snapshot.lifecycle.unwrap();
        assert!(!lifecycle.settings["keep_awake"].on);
        assert_eq!(lifecycle.keep_awake, "off");
        assert!(!rig.signals.is_held());
        rig.mark_running();
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
async fn a_file_changed_after_the_agent_read_it_is_refused_until_start_all_reads_it() {
    let rig = rig("");
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        // a hand edit saved after the agent read the file
        let edited = format!("# a hand edit\n{}", file(rig));
        rig.write_toml(&edited);
        let keep_awake = json!({ "key": "keep_awake", "value": false });
        assert_eq!(
            conn.code("lifecycle_set", keep_awake.clone()).await,
            "config_changed"
        );
        assert_eq!(file(rig), edited);
        let lifecycle = conn.status().await.lifecycle.unwrap();
        assert!(lifecycle.settings["keep_awake"].on);

        // once Start all has read it, the write goes through and keeps the hand edit
        conn.ok("start_all").await;
        conn.until_runner_runs().await;
        rig.mark_running();
        conn.call("lifecycle_set", keep_awake).await.unwrap();
        assert!(file(rig).starts_with("# a hand edit\n"));
        assert!(file(rig).contains("keep_awake = false"));
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn no_setting_can_be_changed_while_no_valid_configuration_was_read() {
    let rig = rig("\n[lifecycle]\nkeep_awake = 3\n");
    let rig = &rig;
    let before = file(rig);
    rig.drive(Launch::Stopped, |conn| async move {
        assert_eq!(
            conn.code(
                "lifecycle_set",
                json!({ "key": "keep_awake", "value": false })
            )
            .await,
            "config_unusable"
        );
        assert_eq!(file(rig), before);
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

        // the next Start all retries the write, even one that keeps the last valid
        // configuration because the file has problems
        *rig.sign_in.failure.borrow_mut() = None;
        rig.write_toml(&file(rig).replace("runners = 1", "runners = 0"));
        conn.ok("start_all").await;
        let snapshot = conn.until_runner_runs().await;
        assert_eq!(snapshot.configuration.in_use.unwrap().source, "last_valid");
        assert_eq!(
            snapshot.lifecycle.unwrap().sign_in.as_deref(),
            Some("registered")
        );
        assert!(rig.sign_in.command.borrow().is_some());
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}
