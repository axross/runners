//! the macOS login item, kept in step with `start_at_login` in effect from the snapshots
//! the agent sends, against a fake login item: when it registers and unregisters, when it
//! leaves the maintainer's choice alone, where it changes nothing, when it tries again
//! after a failure, and how showing the window reads its state again.

use marshal_cli::agent::snapshot::Snapshot;
use marshal_session::{Act, LoginItemRecord, LoginItemRecordFile, LoginStatus};
use marshal_view::SettingRow;
use serde_json::json;

use crate::support::{
    Agent, Answer, FakeLoginItem, LoginCall, Running, World, patched, start_with_login_item,
    temp_dir,
};

const IN_APPLICATIONS: &str = "/Applications/Marshal.app/Contents/MacOS/marshal-app";
const IN_HOME_APPLICATIONS: &str = "/Users/me/Applications/Marshal.app/Contents/MacOS/marshal-app";

/// a snapshot of the agent in the state `name` whose settings in effect have
/// `start_at_login` and `keep_awake` as given.
fn settings_in(name: &str, start_at_login: bool, keep_awake: bool) -> Snapshot {
    patched(name, |value| {
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

/// a snapshot of the running agent whose settings in effect are as given.
fn settings(start_at_login: bool, keep_awake: bool) -> Snapshot {
    settings_in("running", start_at_login, keep_awake)
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
async fn the_setting_on_registers_a_login_item_macos_has_never_seen_and_records_it() {
    // macOS 27 reports a Marshal that has never registered as not found
    let login = FakeLoginItem::new(LoginStatus::NotFound, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    assert_eq!(login.calls(), [LoginCall::Register]);
    assert!(login.recorded());
}

#[tokio::test(start_paused = true)]
async fn a_login_item_not_found_after_marshal_registered_it_counts_as_removed() {
    let login = FakeLoginItem::new(LoginStatus::NotFound, true);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    assert!(login.calls().is_empty(), "the maintainer's choice stands");
    assert!(logged(&session, "turned off in Login Items"));
    send(&agent, &session, settings(false, true)).await;
    assert!(login.calls().is_empty());
    assert!(!login.recorded(), "the setting off forgets the record");
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
    login.fail_with(Some((4, "the app's code signature is not valid")));
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

#[tokio::test(start_paused = true)]
async fn a_start_all_with_unchanged_settings_reconciles_once_and_registers_what_macos_does_not_hold()
 {
    for status in [LoginStatus::NotRegistered, LoginStatus::NotFound] {
        let login = FakeLoginItem::new(LoginStatus::Enabled, false);
        let (agent, session) = connected(&login, IN_APPLICATIONS).await;
        send(&agent, &session, settings_in("stopped_by_you", true, true)).await;
        assert!(login.calls().is_empty(), "{status:?}: it is registered");

        login.set_status(status);
        session.handle.act(Act::StartAll).await.unwrap();
        // a snapshot from before the Start all does not show it
        let read = login.statuses_read();
        send(&agent, &session, settings_in("stopped_by_you", true, true)).await;
        assert_eq!(login.statuses_read(), read, "{status:?}: not yet shown");

        send(&agent, &session, settings(true, true)).await;
        assert_eq!(login.calls(), [LoginCall::Register], "{status:?}");
        assert!(login.recorded(), "{status:?}");

        // the Start all is reconciled once, and the same settings again change nothing
        let read = login.statuses_read();
        send(&agent, &session, settings(true, true)).await;
        assert_eq!(login.statuses_read(), read, "{status:?}: once");
    }
}

#[tokio::test(start_paused = true)]
async fn a_start_all_leaves_what_the_maintainer_removed_in_login_items_removed() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, true);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings_in("stopped_by_you", true, true)).await;
    session.handle.act(Act::StartAll).await.unwrap();
    send(&agent, &session, settings(true, true)).await;
    assert!(login.calls().is_empty(), "the maintainer's choice stands");
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Removed from Open at Login")
    );
}

#[tokio::test(start_paused = true)]
async fn showing_the_window_reads_a_state_changed_outside_marshal_and_registers_nothing() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    assert_eq!(login.calls(), [LoginCall::Register]);
    assert_eq!(login_line(&session).as_deref(), Some("On"));

    // the maintainer turns Marshal off in Login Items while it runs
    login.set_status(LoginStatus::NotRegistered);
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Removed from Open at Login")
    );
    assert_eq!(
        login.calls(),
        [LoginCall::Register],
        "nothing is registered"
    );

    // and approves it again
    login.set_status(LoginStatus::Enabled);
    session.handle.window_shown(false);
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(login_line(&session).as_deref(), Some("On"));
    assert_eq!(login.calls(), [LoginCall::Register]);

    let announced: Vec<String> = session
        .screen
        .announced()
        .into_iter()
        .filter(|line| line.starts_with("Login Items:"))
        .collect();
    assert_eq!(
        announced,
        [
            "Login Items: Removed from Open at Login.",
            "Login Items: On.",
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn showing_the_window_keeps_a_failed_registrations_code_and_reads_nothing_before_the_first_state()
 {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    login.fail_with(Some((4, "the app's code signature is not valid")));
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(login.statuses_read(), 0, "no state has been learned yet");

    send(&agent, &session, settings(true, true)).await;
    session.handle.window_shown(false);
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Could not be added (code 4)")
    );
    assert_eq!(login.calls(), [LoginCall::Register], "tried once");
}

#[tokio::test(start_paused = true)]
async fn a_failed_unregistration_is_logged_with_its_code_and_shown_as_not_removed() {
    let login = FakeLoginItem::new(LoginStatus::Enabled, true);
    login.fail_with(Some((4, "the app's code signature is not valid")));
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(false, true)).await;
    assert_eq!(login.calls(), [LoginCall::Unregister]);
    assert!(login.recorded(), "the record stays while it is registered");
    assert!(
        session
            .log
            .lines()
            .iter()
            .any(|line| line.starts_with("WARNING:")
                && line.contains("unregister")
                && line.contains("(code 4)")),
        "the failure and its code are logged: {:?}",
        session.log.lines()
    );
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Could not be removed (code 4)")
    );
    assert_eq!(
        open_button(&session).as_deref(),
        Some("Open Login Items Settings")
    );

    // showing the window keeps the code while macOS still holds the item
    session.handle.window_shown(false);
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Could not be removed (code 4)")
    );

    // the same settings again try nothing; the next change tries again
    login.fail_with(None);
    send(&agent, &session, settings(false, true)).await;
    assert_eq!(login.calls(), [LoginCall::Unregister]);
    send(&agent, &session, settings(false, false)).await;
    assert_eq!(
        login.calls(),
        [LoginCall::Unregister, LoginCall::Unregister]
    );
    assert_eq!(login_line(&session).as_deref(), Some("Off"));
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

fn start_at_login_row(session: &Running) -> SettingRow {
    let view = session.screen.view();
    let list = view.list().expect("a list");
    list.settings
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .find(|row| row.key == "start_at_login")
        .cloned()
        .expect("the start_at_login row")
}

fn login_line(session: &Running) -> Option<String> {
    start_at_login_row(session).login_item
}

fn open_button(session: &Running) -> Option<String> {
    start_at_login_row(session).open_login_items
}

#[tokio::test(start_paused = true)]
async fn showing_the_window_reads_an_item_still_registered_with_the_setting_off_and_shows_its_button()
 {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(false, true)).await;
    assert_eq!(login_line(&session).as_deref(), Some("Off"));
    assert_eq!(open_button(&session), None);

    // the maintainer adds it in Login Items while Marshal runs
    login.set_status(LoginStatus::Enabled);
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(login_line(&session).as_deref(), Some("On"));
    assert_eq!(
        open_button(&session).as_deref(),
        Some("Open Login Items Settings")
    );
    assert!(login.calls().is_empty(), "nothing is unregistered");
}

#[tokio::test(start_paused = true)]
async fn the_view_shows_the_login_items_state_on_the_mac_and_announces_each_change() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(false, true)).await;
    assert_eq!(login_line(&session).as_deref(), Some("Off"));
    let first_label = session.screen.view().list().unwrap().settings.groups[0].rows[0]
        .label
        .clone();
    assert_eq!(first_label, "Open Marshal when I log in");

    send(&agent, &session, settings(true, true)).await;
    assert_eq!(login_line(&session).as_deref(), Some("On"));

    // macOS holds it for approval; the next change of the settings learns it
    login.set_status(LoginStatus::RequiresApproval);
    send(&agent, &session, settings(true, false)).await;
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Waiting for approval")
    );

    // the maintainer removed it from Open at Login
    login.set_status(LoginStatus::NotRegistered);
    send(&agent, &session, settings(true, true)).await;
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Removed from Open at Login")
    );

    let announced: Vec<String> = session
        .screen
        .announced()
        .into_iter()
        .filter(|line| line.starts_with("Login Items:"))
        .collect();
    assert_eq!(
        announced,
        [
            "Login Items: On.",
            "Login Items: Waiting for approval.",
            "Login Items: Removed from Open at Login.",
        ],
        "the first state learned is not announced"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_registration_shows_its_code() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    login.fail_with(Some((4, "the app's code signature is not valid")));
    let (agent, session) = connected(&login, IN_APPLICATIONS).await;
    send(&agent, &session, settings(true, true)).await;
    assert_eq!(
        login_line(&session).as_deref(),
        Some("Could not be added (code 4)")
    );
}

#[tokio::test(start_paused = true)]
async fn a_copy_outside_applications_shows_no_login_items_line() {
    let login = FakeLoginItem::new(LoginStatus::NotRegistered, false);
    let (agent, session) = connected(
        &login,
        "/Users/me/Downloads/Marshal.app/Contents/MacOS/marshal-app",
    )
    .await;
    send(&agent, &session, settings(true, true)).await;
    assert_eq!(login_line(&session), None);
}
