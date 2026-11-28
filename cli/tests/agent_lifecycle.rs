//! the agent's `[lifecycle]` settings on fake seams behind its real channel: `launch`
//! follows `start_runners_on_launch`, `lifecycle_set` writes one key through the
//! write path and puts it in effect, the sign-in item follows `start_at_login`, and
//! the snapshot shows what is in effect, and an applied `config_edit` or
//! `config_reload` puts the file's `[lifecycle]` in effect as Start all does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::rc::Rc;
use std::time::{Duration, Instant};

use marshal_cli::agent::CallError;
use marshal_cli::agent::machine::{Launch, NOT_STARTED_AT_LAUNCH};
use marshal_cli::profile::Profile;
use marshal_cli::runtime::fake::Call;
use serde_json::{Value, json};
use support::agent::{Conn, ONE_POOL_TOML, Rig, TOKEN};
use tokio::sync::Notify;

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
async fn a_config_toml_deleted_after_the_agent_read_it_is_unusable_as_for_config_edit() {
    let rig = rig("");
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        rig.remove("config.toml");
        assert_eq!(
            conn.code(
                "lifecycle_set",
                json!({ "key": "keep_awake", "value": false })
            )
            .await,
            "config_unusable"
        );
        assert!(!rig.root.path().join("config.toml").exists());
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_sign_in_item_turned_off_in_startup_apps_is_shown_as_turned_off() {
    let rig = rig("\n[lifecycle]\nstart_at_login = true\n");
    rig.sign_in.turned_off.set(true);
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let lifecycle = conn.status().await.lifecycle.unwrap();
        assert_eq!(lifecycle.sign_in.as_deref(), Some("turned_off"));
        assert_eq!(lifecycle.sign_in_error, None);
        // the value is left as it is, not removed
        assert!(rig.sign_in.command.borrow().is_some());
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

// an applied edit or reload puts `[lifecycle]` in effect ---------------------------

/// a `[lifecycle]` table that differs from the defaults in three settings.
const HAND_EDIT: &str =
    "\n[lifecycle]\nkeep_awake = false\nstart_at_login = true\nstop_on_network_change = false\n";

/// the rig's file with `lifecycle` written by hand before the pools.
fn hand_edited(lifecycle: &str) -> String {
    ONE_POOL_TOML.replace("\n[[pool]]", &format!("{lifecycle}\n[[pool]]"))
}

async fn revision(conn: &Conn) -> String {
    conn.ok("config_read").await["revision"]
        .as_str()
        .unwrap()
        .to_string()
}

/// how a change of the file is sent: `config_reload`, or a `config_edit` of the
/// first pool's runners.
#[derive(Clone, Copy, Debug)]
enum Via {
    Reload,
    Edit,
}

impl Via {
    const BOTH: [Self; 2] = [Self::Reload, Self::Edit];

    async fn send(self, conn: &Conn, check: bool) -> Result<Value, CallError> {
        let revision = revision(conn).await;
        match self {
            Self::Reload => {
                conn.call(
                    "config_reload",
                    json!({"revision": revision, "check": check}),
                )
                .await
            }
            Self::Edit => {
                conn.call(
                    "config_edit",
                    json!({
                        "revision": revision,
                        "check": check,
                        "edits": [{"op": "set", "path": "pool[0].runners", "value": 2}],
                    }),
                )
                .await
            }
        }
    }
}

/// a running agent whose first runner runs, so a stop has something to stop.
async fn running(conn: &Conn, rig: &Rig) {
    conn.until_runner_runs().await;
    rig.mark_running();
}

async fn until_applied(conn: &Conn) {
    conn.until("the apply done", |snapshot| {
        snapshot
            .configuration
            .last_apply
            .as_ref()
            .is_some_and(|apply| apply.outcome == "applied")
    })
    .await;
}

/// a hand edit of `[lifecycle]`, sent through `via` on an agent that `launch` leaves
/// stopped or running, puts the file's settings in effect: the snapshot, the sign-in
/// item and the keep-awake hold follow them, and a setting changed next keeps them.
async fn a_hand_edit_applied(launch: Launch, via: Via) {
    let rig = rig("");
    let rig = &rig;
    rig.drive(launch, |conn| async move {
        let running_mode = launch == Launch::Running;
        if running_mode {
            running(&conn, rig).await;
            assert!(rig.signals.is_held());
        }
        let before = conn.status().await.lifecycle.unwrap();
        assert!(before.settings["keep_awake"].defaulted);
        rig.write_toml(&hand_edited(HAND_EDIT));
        via.send(&conn, false).await.unwrap();
        if running_mode && matches!(via, Via::Edit) {
            until_applied(&conn).await;
        }

        let lifecycle = conn.status().await.lifecycle.unwrap();
        for (key, on) in [
            ("keep_awake", false),
            ("start_at_login", true),
            ("stop_on_network_change", false),
        ] {
            let setting = lifecycle.settings[key];
            assert!(
                setting.on == on && !setting.defaulted,
                "{via:?} {launch:?}: {key} is {setting:?}"
            );
        }
        assert_eq!(lifecycle.keep_awake, "off", "{via:?} {launch:?}");
        assert_eq!(lifecycle.sign_in.as_deref(), Some("registered"));
        assert!(rig.sign_in.command.borrow().is_some());
        if running_mode {
            conn.eventually("the hold ended", || !rig.signals.is_held())
                .await;
        }

        // a setting changed next keeps the file's values of the others
        let answer = conn
            .call(
                "lifecycle_set",
                json!({ "key": "resume_after_stop", "value": true }),
            )
            .await
            .unwrap();
        let settings = &answer["lifecycle"]["settings"];
        assert_eq!(settings["keep_awake"]["on"], false, "{via:?} {launch:?}");
        assert_eq!(settings["start_at_login"]["on"], true);
        let written = file(rig);
        assert!(written.contains("keep_awake = false"), "{written}");
        assert!(written.contains("start_at_login = true"), "{written}");
        assert!(written.contains("resume_after_stop = true"), "{written}");
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_reload_on_a_stopped_agent_puts_the_files_lifecycle_in_effect() {
    a_hand_edit_applied(Launch::Stopped, Via::Reload).await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_edit_on_a_stopped_agent_puts_the_files_lifecycle_in_effect() {
    a_hand_edit_applied(Launch::Stopped, Via::Edit).await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_reload_on_a_running_agent_puts_the_files_lifecycle_in_effect() {
    a_hand_edit_applied(Launch::Running, Via::Reload).await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_edit_on_a_running_agent_puts_the_files_lifecycle_in_effect() {
    a_hand_edit_applied(Launch::Running, Via::Edit).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_check_a_refusal_and_a_stale_revision_leave_the_settings_in_effect() {
    for launch in [Launch::Stopped, Launch::Running] {
        let rig = rig("");
        let rig = &rig;
        rig.drive(launch, |conn| async move {
            if launch == Launch::Running {
                running(&conn, rig).await;
            }
            let hand_edited_file = hand_edited(HAND_EDIT);
            rig.write_toml(&hand_edited_file);
            for via in Via::BOTH {
                via.send(&conn, true).await.unwrap();
                let lifecycle = conn.status().await.lifecycle.unwrap();
                assert!(
                    lifecycle.settings["keep_awake"].defaulted,
                    "a check of {via:?} put the file's settings in effect"
                );
                assert!(rig.sign_in.command.borrow().is_none());
            }

            // a revision the file has left is refused
            let stale = revision(&conn).await;
            rig.write_toml(&format!("# edited again\n{hand_edited_file}"));
            let code = conn.code("config_reload", json!({"revision": stale})).await;
            assert_eq!(code, "config_changed");

            // a file that breaks the rules is refused, and keeps what is in effect
            rig.write_toml(&hand_edited(&format!("{HAND_EDIT}keep_awake = 3\n")));
            let code = conn
                .code("config_reload", json!({"revision": revision(&conn).await}))
                .await;
            assert!(code.starts_with("config_"), "{code}");
            let lifecycle = conn.status().await.lifecycle.unwrap();
            assert!(lifecycle.settings["keep_awake"].defaulted);
            assert!(!lifecycle.settings["start_at_login"].on);
            assert!(rig.sign_in.command.borrow().is_none());
            if launch == Launch::Running {
                assert!(rig.signals.is_held());
            }
            conn.ok("quit").await;
        })
        .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_at_a_revision_read_before_a_setting_change_is_refused_as_changed() {
    let rig = rig("");
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let read = revision(&conn).await;
        conn.call(
            "lifecycle_set",
            json!({ "key": "keep_awake", "value": false }),
        )
        .await
        .unwrap();
        let edit = json!({
            "revision": read,
            "edits": [{"op": "set", "path": "pool[0].runners", "value": 2}],
        });
        assert_eq!(conn.code("config_edit", edit).await, "config_changed");
        assert_eq!(
            conn.code("config_reload", json!({"revision": read})).await,
            "config_changed"
        );
        conn.ok("quit").await;
    })
    .await;
}

/// the Mac profile has no `[lifecycle]`: an applied reload or edit takes no setting
/// and writes no sign-in item.
#[tokio::test(flavor = "current_thread")]
async fn on_the_mac_profile_an_applied_reload_or_edit_changes_no_setting() {
    for launch in [Launch::Stopped, Launch::Running] {
        for via in Via::BOTH {
            let rig = Rig::new("");
            rig.remove("config.json");
            rig.profile.set(Profile::Axmc);
            let config = "schema_version = 1\nrouting_label = \"axmc\"\n\n[budget]\ncpus = 8\n\
                memory_gb = 32\ndisk_gb = 180\n\n[[pool]]\nname = \"r\"\nrepository = \"o/r\"\n\
                runners = 1\nos = \"linux\"\ndisk_gb = 40\n";
            rig.write_toml(config);
            rig.store_token("o", "r", TOKEN);
            rig.hold_jobs();
            let rig = &rig;
            rig.drive(launch, |conn| async move {
                if launch == Launch::Running {
                    running(&conn, rig).await;
                }
                rig.write_toml(&format!("# a hand edit\n{config}"));
                via.send_axmc(&conn).await;
                let snapshot = conn.status().await;
                assert!(snapshot.lifecycle.is_none(), "{via:?} {launch:?}");
                assert!(rig.sign_in.command.borrow().is_none());
                assert!(rig.in_effect.get().is_none());
                conn.ok("quit").await;
            })
            .await;
        }
    }
}

impl Via {
    /// sends the change to an axmc agent, whose pool is `os = "linux"` with a disk.
    async fn send_axmc(self, conn: &Conn) {
        let revision = revision(conn).await;
        match self {
            Self::Reload => {
                conn.call("config_reload", json!({"revision": revision}))
                    .await
                    .unwrap();
            }
            Self::Edit => {
                conn.call(
                    "config_edit",
                    json!({
                        "revision": revision,
                        "edits": [{"op": "set", "path": "pool[0].runners", "value": 2}],
                    }),
                )
                .await
                .unwrap();
            }
        }
    }
}

/// sends a change that names another image, which the agent checks before it writes:
/// a reload of a file whose `image_name` was changed by hand, or an edit of it.
async fn image_change(conn: &Conn, rig: &Rig, via: Via) -> Result<Value, CallError> {
    match via {
        Via::Reload => {
            rig.write_toml(&file(rig).replace("actions-runner:local", "other:2"));
            conn.call("config_reload", json!({"revision": revision(conn).await}))
                .await
        }
        Via::Edit => {
            conn.call(
                "config_edit",
                json!({
                    "revision": revision(conn).await,
                    "edits": [{"op": "set", "path": "image_name", "value": "other:2"}],
                }),
            )
            .await
        }
    }
}

/// waits, on `other`, until the agent has begun to check the image `other:2`.
async fn until_the_image_is_checked(other: &Conn, rig: &Rig) {
    other
        .until("the image check", |_| {
            rig.backend
                .state
                .borrow()
                .image_checks
                .last()
                .map(String::as_str)
                == Some("other:2")
        })
        .await;
}

async fn a_setting_waits_for_the_image_check_of(via: Via) {
    {
        let rig = rig("");
        let rig = &rig;
        let gate = &Rc::new(Notify::new());
        rig.drive(Launch::Running, |conn| async move {
            running(&conn, rig).await;
            rig.backend.state.borrow_mut().image_gate = Some(Rc::clone(gate));
            let other = conn.another().await;
            let change = image_change(&conn, rig, via);
            let meanwhile = async {
                until_the_image_is_checked(&other, rig).await;
                let code = other
                    .code(
                        "lifecycle_set",
                        json!({ "key": "keep_awake", "value": false }),
                    )
                    .await;
                gate.notify_one();
                code
            };
            let (changed, code) = tokio::join!(change, meanwhile);
            assert_eq!(code, "busy", "{via:?}");
            changed.unwrap();

            // the settings in effect and the file agree: nothing was saved meanwhile,
            // and a setting changed once the change is applied is written
            until_applied_or_written(&conn).await;
            let lifecycle = conn.status().await.lifecycle.unwrap();
            assert!(lifecycle.settings["keep_awake"].defaulted, "{via:?}");
            assert!(!file(rig).contains("keep_awake"), "{via:?}");
            assert!(file(rig).contains("other:2"), "{via:?}");
            conn.call(
                "lifecycle_set",
                json!({ "key": "keep_awake", "value": false }),
            )
            .await
            .unwrap();
            assert!(file(rig).contains("keep_awake = false"), "{via:?}");
            conn.ok("quit").await;
        })
        .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_setting_is_refused_as_busy_while_a_reload_waits_for_its_image_check() {
    a_setting_waits_for_the_image_check_of(Via::Reload).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_setting_is_refused_as_busy_while_an_edit_waits_for_its_image_check() {
    a_setting_waits_for_the_image_check_of(Via::Edit).await;
}

/// waits until no apply is under way, so a setting is not refused as busy for it.
async fn until_applied_or_written(conn: &Conn) {
    conn.until("no apply under way", |snapshot| {
        snapshot
            .configuration
            .last_apply
            .as_ref()
            .is_none_or(|apply| apply.outcome != "applying")
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_setting_is_accepted_again_when_a_stop_ends_the_change_that_waited() {
    let rig = rig("");
    let rig = &rig;
    let gate = &Rc::new(Notify::new());
    rig.drive(Launch::Running, |conn| async move {
        running(&conn, rig).await;
        rig.backend.state.borrow_mut().image_gate = Some(Rc::clone(gate));
        let other = conn.another().await;
        let change = image_change(&conn, rig, Via::Edit);
        let meanwhile = async {
            until_the_image_is_checked(&other, rig).await;
            other.ok("stop_all").await;
        };
        let (changed, ()) = tokio::join!(change, meanwhile);
        assert!(changed.is_err(), "the edit was answered as written");
        conn.until_state("stopped").await;
        // the dropped edit left nothing that holds a setting back
        conn.call(
            "lifecycle_set",
            json!({ "key": "keep_awake", "value": false }),
        )
        .await
        .unwrap();
        conn.ok("quit").await;
    })
    .await;
}
