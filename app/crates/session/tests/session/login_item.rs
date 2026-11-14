//! the macOS login item, kept in step with `start_at_login` in effect from the snapshots
//! the agent sends, against a fake login item: when it registers and unregisters, when it
//! leaves the maintainer's choice alone, where it changes nothing, and when it tries
//! again after a failure.

use marshal_cli::agent::snapshot::Snapshot;
use marshal_session::{LoginItemRecord, LoginItemRecordFile, LoginStatus};
use serde_json::json;

use crate::support::{
    Agent, Answer, FakeLoginItem, LoginCall, Running, World, patched, start_with_login_item,
    temp_dir,
};

const IN_APPLICATIONS: &str = "/Applications/Marshal.app/Contents/MacOS/marshal-app";
const IN_HOME_APPLICATIONS: &str = "/Users/me/Applications/Marshal.app/Contents/MacOS/marshal-app";

/// a snapshot whose settings in effect have `start_at_login` and `keep_awake` as given.
fn settings(start_at_login: bool, keep_awake: bool) -> Snapshot {
    patched("running", |value| {
        value["lifecycle"] = json!({
            "settings": {
                "start_at_login": { "on": start_at_login, "defaulted": false },
                "start_runners_on_launch": { "on": true, "defaulted": true },
                "keep_awake": { "on": keep_awake, "defaulted": false },
                "stop_on_network_change": { "on": true, "defaulted": true },
                "resume_after_stop": { "on": false, "defaulted": true },
            },
            "keep_awake": if keep_awake { "held" } else { "off" },
        });
    })
}

async fn connected(login: &FakeLoginItem, executable: &str) -> (Agent, Running) {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start_with_login_item(world, login.at(executable));
    session.settle().await;
    (agent, session)
}

async fn send(agent: &Agent, session: &Running, snapshot: Snapshot) {
    agent.push(snapshot);
    session.settle().await;
}

fn logged(session: &Running, words: &str) -> bool {
    session.log.lines().iter().any(|line| line.contains(words))
}

#[tokio::test(start_paused = true)]
async fn the_setting_on_registers_a_login_item_macos_reports_not_registered_and_records_it() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    let (agent, session) = connected(&login, IN_HOME_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    assert_eq!(login.calls(), [LoginCall::Register]);
    assert_eq!(login.status_now(), LoginStatus::Enabled);
    assert!(
        login.recorded(),
        "Marshal records that it registered itself"
    );
    assert!(logged(&session, "registered its login item"));
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_with_no_lifecycle_part_changes_nothing() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, patched("running", |_| {})).await;
    assert!(login.calls().is_empty());
    assert_eq!(login.statuses_read(), 0);
}

#[tokio::test(start_paused = true)]
async fn the_setting_on_leaves_an_enabled_login_item_alone() {
    let login = FakeLoginItem::new(LoginStatus::Enabled, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    assert!(login.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_setting_off_unregisters_a_registered_login_item_and_clears_the_record() {
    let login = FakeLoginItem::new(LoginStatus::Enabled, true);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(false, true)).await;
    assert_eq!(login.calls(), [LoginCall::Unregister]);
    assert_eq!(login.status_now(), LoginStatus::NotRegistered);
    assert!(!login.recorded(), "the record is cleared");
    assert!(logged(&session, "unregistered its login item"));
}

#[tokio::test(start_paused = true)]
async fn the_setting_off_leaves_a_login_item_that_is_not_registered_alone() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(false, true)).await;
    assert!(login.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_setting_on_does_not_register_over_requires_approval() {
    let login = FakeLoginItem::new(LoginStatus::RequiresApproval, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    // a change of another setting brings it in step again, and still leaves it
    send(&agent, &session, settings(true, false)).await;
    assert!(login.calls().is_empty());
    assert!(!login.recorded());
    assert!(logged(&session, "waits for approval"));
}

#[tokio::test(start_paused = true)]
async fn a_login_item_turned_off_in_login_items_stays_off_until_the_setting_is_turned_off_and_on() {
    // Marshal registered it, and the maintainer turned it off in Login Items
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, true);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    send(&agent, &session, settings(true, false)).await;
    assert!(login.calls().is_empty(), "the maintainer's choice stands");
    assert!(logged(&session, "turned off in Login Items"));

    send(&agent, &session, settings(false, false)).await;
    assert!(login.calls().is_empty(), "nothing is registered to remove");
    assert!(!login.recorded(), "the setting off forgets the record");
    send(&agent, &session, settings(true, false)).await;
    assert_eq!(login.calls(), [LoginCall::Register]);
    assert!(login.recorded());
}

#[tokio::test(start_paused = true)]
async fn a_copy_outside_applications_neither_registers_nor_unregisters() {
    for executable in [
        "/Users/me/Downloads/Marshal.app/Contents/MacOS/marshal-app",
        "/Applications/Tools/Marshal.app/Contents/MacOS/marshal-app",
        "/Applications/marshal-app",
        "/Users/other/Applications/Marshal.app/Contents/MacOS/marshal-app",
        "/Applications/Marshal.app/Contents/Resources/marshal-app",
    ] {
        let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
        let (agent, session) = connected(&login, executable).await;
        send(&agent, &session, settings(true, true)).await;
        send(&agent, &session, settings(false, true)).await;
        assert!(login.calls().is_empty(), "{executable} changes nothing");
        assert_eq!(login.statuses_read(), 0, "{executable} asks macOS nothing");
        assert!(logged(&session, "outside Applications"));
    }
}

#[tokio::test(start_paused = true)]
async fn a_failed_registration_is_logged_and_tried_again_at_the_next_change_of_the_settings() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    login.fail_with(Some("the app's code signature is not valid (code 4)"));
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    assert_eq!(login.calls(), [LoginCall::Register]);
    assert!(!login.recorded());
    assert!(
        session
            .log
            .lines()
            .iter()
            .any(|line| line.starts_with("WARNING:") && line.contains("(code 4)")),
        "the failure and its code are logged: {:?}",
        session.log.lines()
    );

    // the same settings again, as each snapshot carries them, try nothing
    login.fail_with(None);
    for _ in 0..3 {
        send(&agent, &session, settings(true, true)).await;
    }
    assert_eq!(login.calls(), [LoginCall::Register]);

    send(&agent, &session, settings(true, false)).await;
    assert_eq!(login.calls(), [LoginCall::Register, LoginCall::Register]);
    assert!(login.recorded());
}

#[test]
fn the_record_file_is_the_record_and_removing_it_twice_succeeds() {
    let directory = temp_dir("login-item-record");
    let record = LoginItemRecordFile::new(directory.join("login-item-registered-by-marshal"));
    assert!(!record.exists());
    record.write().unwrap();
    assert!(record.exists());
    record.remove().unwrap();
    assert!(!record.exists());
    record.remove().unwrap();
}
