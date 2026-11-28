//! a read of a token that misses once or twice, as the Credential Manager can right
//! after a write: pool admission and the presence check read the token again, a token
//! this process knows is stored is reported as unreadable rather than missing, and
//! nothing else reads again.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use marshal_cli::agent::machine::Launch;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::tokens::STORED_UNREADABLE;
use marshal_cli::token_store::{StoreError, StoreErrorKind, TokenKey};
use support::agent::{OTHER_TOKEN, Rig, TOKEN, TWO_POOLS};

/// every read answers as the store holds it, as `script_reads` takes it.
const HIT: bool = true;
const MISS: bool = false;

fn key(name: &str) -> TokenKey {
    TokenKey::of("o", name)
}

fn pool<'a>(snapshot: &'a Snapshot, name: &str) -> &'a marshal_cli::agent::snapshot::PoolInfo {
    snapshot
        .pools
        .iter()
        .find(|pool| pool.name == name)
        .unwrap()
}

fn has_info(rig: &Rig, text: &str) -> bool {
    rig.log
        .lines()
        .iter()
        .any(|line| line.starts_with("INFO: ") && line.contains(text))
}

/// the first read after a save misses, as a Credential Manager that has not caught up;
/// the waiting pool starts on the re-read.
#[tokio::test(flavor = "current_thread")]
async fn a_waiting_pool_whose_first_read_after_a_save_misses_starts_on_the_re_read() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        let before = conn.until_runner_runs().await;
        assert_eq!(
            pool(&before, "d").reason.as_deref(),
            Some("pool[1]: no token.")
        );
        rig.store.script_reads(&key("d"), &[MISS]);
        conn.set_token("o/d", OTHER_TOKEN).await.unwrap();
        conn.until("pool d admitted", |snapshot| {
            pool(snapshot, "d").state != "skipped"
        })
        .await;
        conn.until("both pools running", |snapshot| snapshot.runners_up == 2)
            .await;
        assert!(has_info(
            rig,
            "the first read of the token for o/d missed; re-read 1 of 2 found it."
        ));
        assert!(rig.registered_with().contains(&OTHER_TOKEN.to_string()));
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}

/// at launch the presence check reads first and finds the token, and the admission
/// read right after it misses; the pool starts on the re-read.
#[tokio::test(flavor = "current_thread")]
async fn at_launch_a_stored_tokens_admission_read_that_misses_starts_its_pool_on_the_re_read() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.store_token("o", "d", OTHER_TOKEN);
    rig.store.script_reads(&key("r"), &[HIT, MISS, HIT]);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until("both pools running", |snapshot| snapshot.runners_up == 2)
            .await;
        assert!(has_info(
            rig,
            "the first read of the token for o/r missed; re-read 1 of 2 found it."
        ));
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}

/// the presence check's first read of a stored token misses and its re-read finds it:
/// the app is told the token is stored, which is what keeps its scope in GitHub
/// status collection.
#[tokio::test(flavor = "current_thread")]
async fn a_presence_read_that_misses_is_cleared_by_a_re_read_and_the_token_shows_as_stored() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.store.script_reads(&key("r"), &[MISS, HIT]);
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let snapshot = conn.status().await;
        assert!(
            snapshot
                .repositories
                .iter()
                .any(|r| r.repository == "o/r" && r.has_token)
        );
        assert!(pool(&snapshot, "r").has_token);
        assert_eq!(rig.store.reads_of(&key("r")), 2);
        assert!(has_info(
            rig,
            "the first read of the token for o/r missed; re-read 1 of 2 found it."
        ));
        conn.ok("quit").await;
    })
    .await;
}

/// a token this process saved and that then stays missing through both re-reads is
/// reported as stored but unreadable, keeps showing as stored, and a second save
/// admits the pool again.
#[tokio::test(flavor = "current_thread")]
async fn a_token_known_stored_that_stays_missing_skips_its_pool_with_its_own_reason_and_stays_stored()
 {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        let reads = rig.store.reads_of(&key("d"));
        rig.store.script_reads(&key("d"), &[MISS, MISS, MISS]);
        conn.set_token("o/d", OTHER_TOKEN).await.unwrap();
        let snapshot = conn
            .until("pool d skipped as unreadable", |snapshot| {
                pool(snapshot, "d").reason.as_deref()
                    == Some(format!("pool[1]: {STORED_UNREADABLE}.").as_str())
            })
            .await;
        assert_eq!(rig.store.reads_of(&key("d")), reads + 3);
        assert_eq!(snapshot.agent.state, "running");
        assert!(pool(&snapshot, "d").has_token);
        assert!(
            snapshot
                .repositories
                .iter()
                .any(|r| r.repository == "o/d" && r.has_token)
        );
        let expected = format!(
            "WARN: pool[1] (o/d) is not started and gets no slots: pool[1]: {STORED_UNREADABLE}."
        );
        assert!(
            rig.log.lines().contains(&expected),
            "{expected}\n{:?}",
            rig.log.lines()
        );
        assert!(has_info(
            rig,
            "the token for o/d was still missing after 2 re-reads."
        ));
        assert!(
            rig.log
                .lines()
                .iter()
                .all(|line| !line.contains(OTHER_TOKEN) && !line.contains(TOKEN))
        );
        conn.set_token("o/d", OTHER_TOKEN).await.unwrap();
        conn.until("both pools running", |snapshot| snapshot.runners_up == 2)
            .await;
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}

/// the presence check reads a token it knows is stored again too, keeps it stored when
/// the re-reads miss, and warns naming the repository.
#[tokio::test(flavor = "current_thread")]
async fn the_presence_check_keeps_a_token_known_stored_as_stored_and_warns_naming_it() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.store_token("o", "d", OTHER_TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        assert!(pool(&conn.status().await, "d").has_token);
        let reads = rig.store.reads_of(&key("d"));
        rig.store.script_reads(&key("d"), &[MISS, MISS, MISS]);
        conn.ok("start_all").await;
        conn.until_runner_runs().await;
        let snapshot = conn.status().await;
        assert!(pool(&snapshot, "d").has_token);
        assert!(
            snapshot
                .repositories
                .iter()
                .any(|r| r.repository == "o/d" && r.has_token)
        );
        assert!(rig.store.reads_of(&key("d")) >= reads + 3);
        let warning = format!("WARN: o/d: {STORED_UNREADABLE}.");
        assert!(
            rig.log.lines().contains(&warning),
            "{warning}\n{:?}",
            rig.log.lines()
        );
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}

/// a token that was never stored is read again only the first time this process reads
/// it: its presence read pays the re-reads, and every later read is a single one.
#[tokio::test(flavor = "current_thread")]
async fn a_token_never_stored_is_re_read_only_on_its_first_read_in_the_process() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        let snapshot = conn.until_runner_runs().await;
        assert_eq!(
            pool(&snapshot, "d").reason.as_deref(),
            Some("pool[1]: no token.")
        );
        // the presence read and its two re-reads, then the admission's single read
        assert_eq!(rig.store.reads_of(&key("d")), 4);
        // a save of another token admits the waiting pool again, with a single read
        conn.set_token("o/r", OTHER_TOKEN).await.unwrap();
        conn.eventually("the waiting pool read again", || {
            rig.store.reads_of(&key("d")) == 5
        })
        .await;
        assert_eq!(
            pool(&conn.status().await, "d").reason.as_deref(),
            Some("pool[1]: no token.")
        );
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}

/// two pools that share a never-stored organization key pay one sequence of re-reads
/// between them, not one each.
#[tokio::test(flavor = "current_thread")]
async fn two_pools_sharing_a_token_never_stored_pay_for_one_sequence_of_re_reads() {
    let rig = Rig::new(TWO_POOLS);
    rig.write_toml(
        r#"schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"
[[pool]]
name = "r"
repository = "o/r"
runners = 1
[[pool]]
name = "org-a"
organization = "Example-Org"
runner_group = "group-a"
labels = ["a"]
runners = 1
[[pool]]
name = "org-b"
organization = "Example-Org"
runner_group = "group-b"
labels = ["b"]
runners = 1
"#,
    );
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let organization = TokenKey::parse_organization("example-org").unwrap();
    rig.drive(Launch::Running, |conn| async move {
        let snapshot = conn.until_runner_runs().await;
        assert_eq!(
            pool(&snapshot, "org-a").reason.as_deref(),
            Some("pool[1]: no token.")
        );
        assert_eq!(
            pool(&snapshot, "org-b").reason.as_deref(),
            Some("pool[2]: no token.")
        );
        // two presence reads and two admission reads, and only the first presence read
        // is followed by re-reads
        assert_eq!(rig.store.reads_of(&organization), 6);
        rig.mark_running();
        conn.ok("quit").await;
    })
    .await;
}

/// a store error that is not a missing entry is reported as before and is not read
/// again, by the presence check or by admission.
#[tokio::test(flavor = "current_thread")]
async fn a_store_error_is_reported_as_before_and_is_not_read_again() {
    let rig = Rig::new(TWO_POOLS);
    rig.store.fail_with_error(Some(StoreError::with(
        StoreErrorKind::NeedsUser,
        Some(1312),
        "the vault is locked",
    )));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        let snapshot = conn.until_state("stopped").await;
        let reason = pool(&snapshot, "r").reason.clone().unwrap();
        assert!(
            reason.starts_with("pool[0]: the token store failed: "),
            "{reason}"
        );
        assert!(!pool(&snapshot, "r").has_token);
        // one presence read and one admission read each
        assert_eq!(rig.store.reads_of(&key("r")), 2);
        assert_eq!(rig.store.reads_of(&key("d")), 2);
        conn.ok("quit").await;
    })
    .await;
}
