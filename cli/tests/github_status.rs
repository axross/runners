//! the GitHub status collector against a scripted fake GitHub, on tokio's paused
//! clock: when it asks, what it asks, and what it publishes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::github_status::GithubStatus;
use marshal_cli::github_status::answer::{Answer, Cause};
use marshal_cli::github_status::budget::RateHeaders;
use marshal_cli::github_status::collector::Collector;
use marshal_cli::github_status::http::{Fetch, Get};
use marshal_cli::runtime::fake::MemoryLog;
use marshal_cli::runtime::secret::Secret;
use marshal_cli::token_store::{MemoryTokenStore, TokenKey, TokenStore};
use tokio::sync::watch;
use tokio::time::{Instant, sleep};

const TOKEN: &str = "github_pat_fake_token_for_the_status_tests";
const BASE: &str = "http://127.0.0.1:9";
/// the wall time the tests start at, in milliseconds since the epoch.
const START_MS: u64 = 1_800_000_000_000;
const RUNNERS: &str = "/repos/owner/app/actions/runners?per_page=100";
const RUNS: &str = "/repos/owner/app/actions/runs?status=in_progress&per_page=10";

/// one request the fake read.
#[derive(Debug, Clone)]
struct Seen {
    at: Duration,
    path: String,
    etag: Option<String>,
    token: String,
}

type Handler = Box<dyn FnMut(&str, Option<&str>, Duration) -> Result<Answer, Cause>>;

struct FakeGitHub {
    start: Instant,
    handler: RefCell<Handler>,
    seen: RefCell<Vec<Seen>>,
    in_flight: RefCell<u32>,
}

impl FakeGitHub {
    fn new(
        handler: impl FnMut(&str, Option<&str>, Duration) -> Result<Answer, Cause> + 'static,
    ) -> Self {
        Self {
            start: Instant::now(),
            handler: RefCell::new(Box::new(handler)),
            seen: RefCell::new(Vec::new()),
            in_flight: RefCell::new(0),
        }
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.borrow().clone()
    }

    fn paths(&self) -> Vec<String> {
        self.seen().into_iter().map(|seen| seen.path).collect()
    }
}

impl Fetch for FakeGitHub {
    async fn get(&self, request: &Get<'_>) -> Result<Answer, Cause> {
        *self.in_flight.borrow_mut() += 1;
        assert_eq!(
            *self.in_flight.borrow(),
            1,
            "two display requests in flight"
        );
        let at = self.start.elapsed();
        self.seen.borrow_mut().push(Seen {
            at,
            path: request.path.to_string(),
            etag: request.etag.map(str::to_string),
            token: request.token.expose().to_string(),
        });
        // a request takes a moment, so another could overlap it if the collector
        // sent two at once.
        sleep(Duration::from_millis(100)).await;
        let answer = (self.handler.borrow_mut())(request.path, request.etag, at);
        *self.in_flight.borrow_mut() -= 1;
        answer
    }

    fn base_url(&self) -> &str {
        BASE
    }
}

fn rate(remaining: u64) -> RateHeaders {
    RateHeaders {
        limit: Some(5000),
        remaining: Some(remaining),
        reset: Some(START_MS / 1000 + 3600),
        used: Some(5000 - remaining),
    }
}

fn json(body: &str, etag: &str) -> Answer {
    Answer {
        status: 200,
        rate: rate(4000),
        etag: Some(etag.to_string()),
        body: body.as_bytes().to_vec(),
        ..Answer::default()
    }
}

fn status(code: u16) -> Answer {
    Answer {
        status: code,
        rate: rate(4000),
        ..Answer::default()
    }
}

fn runners(list: &[(&str, &str, bool)]) -> String {
    let items: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(id, (name, status, busy))| {
            format!(
                r#"{{"id":{id},"name":"{name}","status":"{status}","busy":{busy},"labels":[]}}"#
            )
        })
        .collect();
    format!(
        r#"{{"total_count":{},"runners":[{}]}}"#,
        list.len(),
        items.join(",")
    )
}

fn snapshot(runners: &[&str], has_token: bool) -> Arc<Snapshot> {
    let items: Vec<String> = runners
        .iter()
        .enumerate()
        .map(|(slot, name)| {
            format!(
                r#"{{"slot":{},"state":"running","workload":"{name}","since":{START_MS}}}"#,
                slot + 1
            )
        })
        .collect();
    let text = format!(
        r#"{{"repositories":[{{"repository":"owner/app","has_token":{has_token},"pools":["app"]}}],
            "pools":[{{"name":"app","repository":"owner/app","state":"running","has_token":{has_token},
            "runners":[{}]}}]}}"#,
        items.join(",")
    );
    Arc::new(serde_json::from_str(&text).unwrap())
}

struct World {
    fetch: FakeGitHub,
    log: MemoryLog,
    store: Arc<MemoryTokenStore>,
}

impl World {
    fn new(
        handler: impl FnMut(&str, Option<&str>, Duration) -> Result<Answer, Cause> + 'static,
    ) -> Self {
        let store = Arc::new(MemoryTokenStore::default());
        store
            .set(
                &TokenKey::parse("owner/app").unwrap(),
                &Secret::new(TOKEN.to_string()),
            )
            .unwrap();
        Self {
            fetch: FakeGitHub::new(handler),
            log: MemoryLog::default(),
            store,
        }
    }

    fn collector(
        &self,
        published: watch::Sender<Arc<GithubStatus>>,
        active: watch::Receiver<bool>,
    ) -> Collector<'_, FakeGitHub> {
        let start = Instant::now();
        let store: Arc<dyn TokenStore> = self.store.clone();
        Collector::new(
            &self.fetch,
            Some(store),
            &self.log,
            Box::new(move || START_MS + u64::try_from(start.elapsed().as_millis()).unwrap()),
            published,
            active,
        )
    }
}

/// runs the collector beside `script` until the script ends.
async fn drive<S: std::future::Future<Output = ()>>(
    world: &World,
    snapshots: watch::Receiver<Arc<Snapshot>>,
    active: watch::Receiver<bool>,
    published: watch::Sender<Arc<GithubStatus>>,
    script: S,
) {
    let collector = world.collector(published, active);
    tokio::select! {
        () = collector.run(snapshots) => panic!("the collector ended"),
        () = script => {}
    }
}

/// the collector's inputs and output, with the ends the test keeps.
type Channels = (
    watch::Sender<Arc<Snapshot>>,
    watch::Receiver<Arc<Snapshot>>,
    watch::Sender<bool>,
    watch::Receiver<bool>,
    watch::Sender<Arc<GithubStatus>>,
    watch::Receiver<Arc<GithubStatus>>,
);

fn channels(snapshot: Arc<Snapshot>, active: bool) -> Channels {
    let (snapshot_tx, snapshot_rx) = watch::channel(snapshot);
    let (active_tx, active_rx) = watch::channel(active);
    let (status_tx, status_rx) = watch::channel(Arc::new(GithubStatus::default()));
    (
        snapshot_tx,
        snapshot_rx,
        active_tx,
        active_rx,
        status_tx,
        status_rx,
    )
}

fn idle_runner(path: &str, _etag: Option<&str>, _at: Duration) -> Result<Answer, Cause> {
    assert_eq!(path, RUNNERS);
    Ok(json(
        &runners(&[("app-1-20261112000000000", "online", false)]),
        "\"r1\"",
    ))
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn nothing_is_asked_while_no_subscription_wants_it_and_asking_stops_when_it_ends() {
    let world = World::new(idle_runner);
    let (_snapshot_tx, snapshot_rx, active_tx, active_rx, status_tx, _status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), false);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(300)).await;
        assert!(world.fetch.seen().is_empty(), "asked while inactive");
        active_tx.send_replace(true);
        sleep(Duration::from_secs(50)).await;
        active_tx.send_replace(false);
        sleep(Duration::from_secs(300)).await;
    })
    .await;
    let at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .map(|seen| seen.at.as_secs())
        .collect();
    // checked at once, then every 15 s, never after the subscription ended at 350 s
    assert_eq!(at, [300, 315, 330, 345]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_runner_list_is_conditional_per_page_and_matched_by_name() {
    let world = World::new(|path, etag, _| {
        let page2 = "/repositories/7/actions/runners?per_page=100&page=2";
        match (path, etag) {
            (RUNNERS, None) => {
                let mut answer = json(
                    &runners(&[
                        ("app-1-20261112000000000", "online", true),
                        ("someone-elses-runner", "online", false),
                    ]),
                    "\"p1\"",
                );
                answer.link = Some(format!("<{BASE}{page2}>; rel=\"next\""));
                Ok(answer)
            }
            (RUNNERS, Some("\"p1\"")) => Ok(status(304)),
            (path, None) if path == page2 => Ok(json(
                &runners(&[("app-2-20261112000000001", "offline", false)]),
                "\"p2\"",
            )),
            (path, Some("\"p2\"")) if path == page2 => Ok(status(304)),
            (RUNS, _) => Ok(json(r#"{"workflow_runs":[]}"#, "\"runs\"")),
            other => panic!("unexpected request {other:?}"),
        }
    });
    let (_snapshot_tx, snapshot_rx, _active_tx, active_rx, status_tx, status_rx) = channels(
        snapshot(
            &["app-1-20261112000000000", "app-2-20261112000000001"],
            true,
        ),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(16)).await;
    })
    .await;
    let seen = world.fetch.seen();
    let pairs: Vec<(String, Option<String>)> = seen
        .iter()
        .filter(|seen| seen.path.contains("runners"))
        .map(|seen| (seen.path.clone(), seen.etag.clone()))
        .collect();
    assert_eq!(
        pairs,
        [
            (RUNNERS.to_string(), None),
            (
                "/repositories/7/actions/runners?per_page=100&page=2".to_string(),
                None
            ),
            (RUNNERS.to_string(), Some("\"p1\"".to_string())),
            (
                "/repositories/7/actions/runners?per_page=100&page=2".to_string(),
                Some("\"p2\"".to_string())
            ),
        ]
    );
    assert!(seen.iter().all(|seen| seen.token == TOKEN));
    let status = status_rx.borrow().clone();
    let repository = &status.repositories[0];
    assert_eq!(repository.repository, "owner/app");
    assert_eq!(repository.status, "ok");
    let states: BTreeMap<&str, &str> = repository
        .runners
        .iter()
        .map(|runner| (runner.name.as_str(), runner.github.as_str()))
        .collect();
    // the 304s kept both pages, and a runner of another host is not shown
    assert_eq!(
        states,
        BTreeMap::from([
            ("app-1-20261112000000000", "busy"),
            ("app-2-20261112000000001", "offline")
        ])
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_next_page_elsewhere_is_never_requested() {
    let world = World::new(|path, _, _| {
        assert_eq!(path, RUNNERS, "followed a refused next page");
        let mut answer = json(&runners(&[]), "\"p1\"");
        answer.link = Some(
            "<https://evil.example/repos/owner/app/actions/runners?page=2>; rel=\"next\""
                .to_string(),
        );
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(1)).await;
    })
    .await;
    assert_eq!(status_rx.borrow().repositories[0].status, "error");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_read_holds_one_repository_id_across_its_pages() {
    // page 2 names id 7, page 3 goes back to the first request's form, and page 4
    // names id 8, which the read must not follow
    let world = World::new(|path, _, _| {
        let next = match path {
            RUNNERS => "/repositories/7/actions/runners?per_page=100&page=2",
            "/repositories/7/actions/runners?per_page=100&page=2" => {
                "/repos/owner/app/actions/runners?per_page=100&page=3"
            }
            "/repos/owner/app/actions/runners?per_page=100&page=3" => {
                "/repositories/8/actions/runners?per_page=100&page=4"
            }
            other => panic!("followed {other}"),
        };
        let mut answer = json(&runners(&[]), "\"p\"");
        answer.etag = None;
        answer.link = Some(format!("<{BASE}{next}>; rel=\"next\""));
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(1)).await;
    })
    .await;
    assert_eq!(world.fetch.paths().len(), 3);
    assert_eq!(status_rx.borrow().repositories[0].status, "error");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_repository_without_a_registered_runner_or_a_token_is_not_asked() {
    let world = World::new(idle_runner);
    let (snapshot_tx, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&[], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(60)).await;
        snapshot_tx.send_replace(snapshot(&["app-1-20261112000000000"], false));
        sleep(Duration::from_secs(60)).await;
    })
    .await;
    assert!(world.fetch.seen().is_empty());
    assert!(status_rx.borrow().repositories.is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_busy_runners_job_is_looked_up_with_at_most_two_requests_a_turn() {
    let busy = "app-1-20261112000000000";
    let world = World::new(move |path, _, at| match path {
        RUNNERS => Ok(json(&runners(&[(busy, "online", true)]), "\"r\"")),
        RUNS => Ok(json(
            r#"{"total_count":2,"workflow_runs":[{"id":30},{"id":20}]}"#,
            "\"runs\"",
        )),
        "/repos/owner/app/actions/runs/30/jobs?filter=latest&per_page=30" => Ok(json(
            r#"{"jobs":[{"id":301,"run_id":30,"runner_name":"other","name":"x","status":"in_progress"}]}"#,
            "\"j30\"",
        )),
        "/repos/owner/app/actions/runs/20/jobs?filter=latest&per_page=30" => {
            // the runner took a job of the older run; GitHub's text is cleaned
            assert!(at >= Duration::from_secs(15));
            Ok(json(
                &format!(
                    r#"{{"jobs":[{{"id":201,"run_id":20,"runner_name":"{busy}","workflow_name":"CI\u202e","name":"test\n(ubuntu)","status":"in_progress"}}]}}"#
                ),
                "\"j20\"",
            ))
        }
        other => panic!("unexpected request {other}"),
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&[busy], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(50)).await;
    })
    .await;
    let paths = world.fetch.paths();
    assert_eq!(
        paths,
        [
            RUNNERS,
            RUNS,
            "/repos/owner/app/actions/runs/30/jobs?filter=latest&per_page=30",
            RUNNERS,
            RUNS,
            "/repos/owner/app/actions/runs/20/jobs?filter=latest&per_page=30",
            // the job is known, so later turns read only the runner list
            RUNNERS,
            RUNNERS,
        ]
    );
    let status = status_rx.borrow().clone();
    let repository = &status.repositories[0];
    assert_eq!(repository.job_names, "shown");
    let job = repository.runners[0].job.clone().unwrap();
    assert_eq!(
        (job.workflow.as_str(), job.name.as_str()),
        ("CI", "test (ubuntu)")
    );
    assert_eq!((job.run_id, job.job_id), (20, 201));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_job_in_a_run_whose_jobs_were_read_before_the_runner_took_it_is_found() {
    const JOBS: &str = "/repos/owner/app/actions/runs/30/jobs?filter=latest&per_page=30";
    let busy = "app-1-20261112000000000";
    let world = World::new(move |path, etag, at| match path {
        RUNNERS => Ok(json(&runners(&[(busy, "online", true)]), "\"r\"")),
        RUNS => Ok(json(
            r#"{"total_count":1,"workflow_runs":[{"id":30}]}"#,
            "\"runs\"",
        )),
        JOBS if at < Duration::from_secs(15) => Ok(json(
            r#"{"jobs":[{"id":301,"run_id":30,"runner_name":"other","name":"x","status":"in_progress"}]}"#,
            "\"j1\"",
        )),
        JOBS => {
            // asked again with the first answer's tag, which no longer matches
            assert_eq!(etag, Some("\"j1\""));
            Ok(json(
                &format!(
                    r#"{{"jobs":[{{"id":302,"run_id":30,"runner_name":"{busy}","workflow_name":"CI","name":"build","status":"in_progress"}}]}}"#
                ),
                "\"j2\"",
            ))
        }
        other => panic!("unexpected request {other}"),
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&[busy], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(20)).await;
    })
    .await;
    assert_eq!(
        world.fetch.paths(),
        [RUNNERS, RUNS, JOBS, RUNNERS, RUNS, JOBS]
    );
    let job = status_rx.borrow().repositories[0].runners[0]
        .job
        .clone()
        .unwrap();
    assert_eq!((job.run_id, job.job_id), (30, 302));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_run_no_longer_in_progress_keeps_no_jobs_page() {
    const JOBS: &str = "/repos/owner/app/actions/runs/30/jobs?filter=latest&per_page=30";
    let world = World::new(|path, _, at| match path {
        RUNNERS => Ok(json(
            &runners(&[("app-1-20261112000000000", "online", true)]),
            "\"r\"",
        )),
        // run 30 is in progress, then not, then in progress again
        RUNS if (15..30).contains(&at.as_secs()) => Ok(json(r#"{"workflow_runs":[]}"#, "\"none\"")),
        RUNS => Ok(json(r#"{"workflow_runs":[{"id":30}]}"#, "\"one\"")),
        JOBS => Ok(json(
            r#"{"jobs":[{"id":301,"run_id":30,"runner_name":"other","name":"x","status":"in_progress"}]}"#,
            "\"j30\"",
        )),
        other => panic!("unexpected request {other}"),
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(40)).await;
    })
    .await;
    let jobs: Vec<(u64, Option<String>)> = world
        .fetch
        .seen()
        .into_iter()
        .filter(|seen| seen.path == JOBS)
        .map(|seen| (seen.at.as_secs(), seen.etag))
        .collect();
    // the page read at 0 s was dropped when the run left at 15 s, so it is read
    // again in full at 30 s
    assert_eq!(jobs, [(0, None), (30, None)]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_lookup_stops_after_20_turns_without_a_match() {
    let world = World::new(|path, _, _| match path {
        RUNNERS => Ok(json(
            &runners(&[("app-1-20261112000000000", "online", true)]),
            "\"r\"",
        )),
        RUNS => Ok(json(r#"{"workflow_runs":[]}"#, "\"runs\"")),
        other => panic!("unexpected request {other}"),
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(15 * 30)).await;
    })
    .await;
    let lookups = world
        .fetch
        .paths()
        .iter()
        .filter(|path| *path == RUNS)
        .count();
    assert_eq!(lookups, 20);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn hidden_job_names_are_tried_again_only_when_a_runner_becomes_busy_and_once_a_minute() {
    // the runner is busy, then idle at 30 s, busy again at 45 s, idle at 60 s, and
    // busy again at 75 s: three times busy, but within a minute of the last try once
    let world = World::new(|path, _, at| match path {
        RUNNERS => {
            let busy = !matches!(at.as_secs(), 30..=44 | 60..=74);
            Ok(json(
                &runners(&[("app-1-20261112000000000", "online", busy)]),
                &format!("\"r{busy}\""),
            ))
        }
        RUNS => {
            let mut answer = status(403);
            answer.message = Some("Resource not accessible by personal access token".to_string());
            answer.accepted_permissions = Some("actions=read".to_string());
            Ok(answer)
        }
        other => panic!("unexpected request {other}"),
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(80)).await;
    })
    .await;
    let runs_at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .filter(|seen| seen.path == RUNS)
        .map(|seen| seen.at.as_secs())
        .collect();
    // the first lookup finds names hidden at 0 s; becoming busy at 45 s is within a
    // minute of that, and becoming busy at 75 s is not
    assert_eq!(runs_at, [0, 75]);
    let status = status_rx.borrow().clone();
    assert_eq!(status.repositories[0].job_names, "hidden");
    assert_eq!(status.repositories[0].runners[0].github, "busy");
    assert!(
        world
            .log
            .lines()
            .iter()
            .any(|line| line.contains("actions=read")
                && line.contains("Resource not accessible by personal access token"))
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_runner_busy_within_the_minute_gets_the_hidden_names_retry_when_the_minute_ends() {
    // idle from 15 s to 29 s, then busy for good: the retry waits for the minute
    let world = World::new(|path, _, at| match path {
        RUNNERS => {
            let busy = !(15..30).contains(&at.as_secs());
            Ok(json(
                &runners(&[("app-1-20261112000000000", "online", busy)]),
                &format!("\"r{busy}\""),
            ))
        }
        RUNS => Ok(status(403)),
        other => panic!("unexpected request {other}"),
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(140)).await;
    })
    .await;
    let runs_at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .filter(|seen| seen.path == RUNS)
        .map(|seen| seen.at.as_secs())
        .collect();
    // names became hidden just after 0 s, so the first check after the minute is at
    // 75 s; without the waiting retry the runner busy since 30 s would get none
    assert_eq!(runs_at, [0, 75]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_failed_lookup_backs_off_and_says_when_it_tries_again() {
    // the runs answer 200 with a body that is no list of runs
    let world = World::new(|path, _, _| match path {
        RUNNERS => Ok(json(
            &runners(&[("app-1-20261112000000000", "online", true)]),
            "\"r\"",
        )),
        RUNS => Ok(json("not json", "\"x\"")),
        other => panic!("unexpected request {other}"),
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(160)).await;
    })
    .await;
    let runs_at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .filter(|seen| seen.path == RUNS)
        .map(|seen| seen.at.as_secs())
        .collect();
    // 15 s, then 30 s, then 60 s after each failure, at the next check
    assert_eq!(runs_at, [0, 30, 75, 150]);
    let status = status_rx.borrow().clone();
    let repository = &status.repositories[0];
    assert_eq!(repository.status, "ok");
    assert_eq!(repository.runners[0].github, "busy");
    assert_eq!(repository.job_names, "unreadable");
    assert_eq!(repository.job_names_cause.as_deref(), Some("error"));
    assert!(repository.job_names_retry_at.unwrap() > START_MS + 150_000);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_runner_list_that_is_not_one_is_an_error_and_keeps_the_last_runners() {
    let world = World::new(|_, _, at| {
        if at.as_secs() == 0 {
            Ok(json(
                &runners(&[("app-1-20261112000000000", "online", false)]),
                "\"r1\"",
            ))
        } else {
            Ok(json("<html>", "\"r2\""))
        }
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(20)).await;
    })
    .await;
    let status = status_rx.borrow().clone();
    assert_eq!(status.repositories[0].status, "error");
    assert_eq!(status.repositories[0].runners[0].github, "idle");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn checks_slow_at_25_percent_and_pause_at_10_percent_until_the_reset() {
    let world = World::new(|path, _, at| {
        assert_eq!(path, RUNNERS);
        let remaining = match at.as_secs() {
            0..=14 => 1250,
            15..=99 => 500,
            _ => 4999,
        };
        let mut answer = json(&runners(&[]), "\"r\"");
        answer.rate = rate(remaining);
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(3590)).await;
        assert_eq!(status_rx.borrow().budget.as_ref().unwrap().level, "paused");
        // ends between checks, so the check at 3630 s does not race the end
        sleep(Duration::from_secs(45)).await;
    })
    .await;
    let at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .map(|seen| seen.at.as_secs())
        .collect();
    // slowed after the first answer, so the next check waits 60 s; paused after it,
    // so nothing until the reset at 3600 s, then normal again
    assert_eq!(at, [0, 60, 3600, 3615, 3630]);
    assert_eq!(status_rx.borrow().budget.as_ref().unwrap().level, "normal");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_reset_far_away_pauses_for_at_most_an_hour() {
    let world = World::new(|_, _, _| {
        let mut answer = json(&runners(&[]), "\"r\"");
        answer.etag = None;
        answer.rate = RateHeaders {
            reset: Some(u64::MAX),
            ..rate(100)
        };
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(3610)).await;
    })
    .await;
    let at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .map(|seen| seen.at.as_secs())
        .collect();
    assert_eq!(at, [0, 3600]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_failure_is_tried_again_after_15_s_doubling_and_keeps_the_last_states() {
    let world = World::new(|path, _, at| {
        assert_eq!(path, RUNNERS);
        if at.as_secs() == 0 {
            Ok(json(
                &runners(&[("app-1-20261112000000000", "online", false)]),
                "\"r\"",
            ))
        } else if at.as_secs() < 100 {
            Ok(status(502))
        } else {
            Err(Cause::Unreachable)
        }
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(200)).await;
    })
    .await;
    let at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .map(|seen| seen.at.as_secs())
        .collect();
    assert_eq!(
        at,
        [0, 15, 30, 60, 120, 240]
            .into_iter()
            .take_while(|at| *at <= 200)
            .collect::<Vec<_>>()
    );
    let status = status_rx.borrow().clone();
    let repository = &status.repositories[0];
    assert_eq!(repository.status, "unreachable");
    assert_eq!(repository.runners[0].github, "idle");
    // when GitHub answered the last list, 100 ms after it was asked
    assert_eq!(repository.checked_at, Some(START_MS + 100));
    // the last failure, at about 120 s, waits 120 s
    let retry_at = repository.retry_at.unwrap();
    assert!(
        (START_MS + 240_000..START_MS + 241_000).contains(&retry_at),
        "{retry_at}"
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_secondary_limit_holds_every_request_for_a_minute_then_twice_as_long() {
    let world = World::new(|_, _, at| {
        if at.as_secs() < 200 {
            let mut answer = status(403);
            answer.message = Some("You have exceeded a secondary rate limit.".to_string());
            Ok(answer)
        } else {
            Ok(json(&runners(&[]), "\"r\""))
        }
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(400)).await;
    })
    .await;
    let at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .map(|seen| seen.at.as_secs())
        .take(4)
        .collect();
    assert_eq!(
        at,
        [0, 60, 180, 420]
            .into_iter()
            .take_while(|at| *at <= 400)
            .collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_token_never_reaches_the_value_or_the_log() {
    let world = World::new(|path, _, _| match path {
        RUNNERS => Ok(json(
            &runners(&[("app-1-20261112000000000", "online", true)]),
            "\"r\"",
        )),
        _ => {
            let mut answer = status(404);
            answer.message = Some(format!("Not Found {TOKEN}"));
            Ok(answer)
        }
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(1)).await;
    })
    .await;
    let value = serde_json::to_string(&*status_rx.borrow().clone()).unwrap();
    assert!(!value.contains(TOKEN));
    assert!(world.log.lines().iter().all(|line| !line.contains(TOKEN)));
    assert_eq!(status_rx.borrow().repositories[0].job_names, "unreadable");
    assert_eq!(
        status_rx.borrow().repositories[0]
            .job_names_cause
            .as_deref(),
        Some("not_found")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn retry_after_is_waited_out_and_a_poll_interval_lengthens_the_checks() {
    let world = World::new(|_, _, at| {
        if at.as_secs() == 0 {
            let mut answer = status(429);
            answer.retry_after = Some(Duration::from_secs(100));
            return Ok(answer);
        }
        let mut answer = json(&runners(&[]), "\"r\"");
        answer.poll_interval = Some(Duration::from_secs(40));
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) =
        channels(snapshot(&["app-1-20261112000000000"], true), true);
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(200)).await;
    })
    .await;
    let at: Vec<u64> = world
        .fetch
        .seen()
        .iter()
        .map(|seen| seen.at.as_secs())
        .collect();
    assert_eq!(at, [0, 100, 140, 180]);
}

const ORGANIZATION_RUNNERS: &str = "/orgs/owner/actions/runners?per_page=100";
const ORGANIZATION_TOKEN: &str = "github_pat_fake_organization_status_token";

/// builds wire-shaped pools independently of the producer, so a defaulted
/// repository field cannot conceal an organisation-only decoding failure.
fn organization_snapshot(
    pools: &[(&str, &str, &str)],
    has_token: bool,
    repository: bool,
) -> Arc<Snapshot> {
    let mut value = serde_json::to_value(&*snapshot(
        if repository {
            &["app-1-20261112000000000"]
        } else {
            &[]
        },
        repository,
    ))
    .unwrap();
    let entries: Vec<_> = pools
        .iter()
        .enumerate()
        .map(|(index, (organization, group, runner))| {
            serde_json::json!({
                "name": format!("pool-{index}"),
                "organization": organization,
                "runner_group": group,
                "has_token": has_token,
                "runners": if runner.is_empty() { vec![] } else {
                    vec![serde_json::json!({"workload": runner, "state": "running"})]
                }
            })
        })
        .collect();
    value["pools"].as_array_mut().unwrap().extend(entries);
    value["organizations"] = serde_json::json!([
        {"organization": "owner", "has_token": has_token, "pools": []},
        {"organization": "other", "has_token": has_token, "pools": []}
    ]);
    Arc::new(serde_json::from_value(value).unwrap())
}

/// keeps the repository credential distinct from two organisation credentials,
/// making accidental reuse of a repository scope observable to the fake fetch.
fn organization_world(
    handler: impl FnMut(&str, Option<&str>, Duration) -> Result<Answer, Cause> + 'static,
) -> World {
    let world = World::new(handler);
    for organization in ["owner", "other"] {
        world
            .store
            .set(
                &TokenKey::parse_organization(organization).unwrap(),
                &Secret::new(ORGANIZATION_TOKEN.to_string()),
            )
            .unwrap();
    }
    world
}

/// several pools in one group and another group share one paged scope; an
/// unrelated host's runner and a same-named repository runner are controls.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn organization_pools_share_conditional_pages_but_not_repository_scope_or_jobs() {
    const PAGE2: &str = "/orgs/owner/actions/runners?per_page=100&page=2";
    let world = organization_world(|path, etag, _| {
        if etag.is_some() {
            assert_eq!(etag, Some(if path == PAGE2 { "\"p2\"" } else { "\"p1\"" }));
            return Ok(status(304));
        }
        match path {
            ORGANIZATION_RUNNERS => {
                let mut answer = json(
                    &runners(&[
                        ("alpha-1", "online", true),
                        ("another-host", "online", true),
                        ("app-1-20261112000000000", "online", true),
                    ]),
                    "\"p1\"",
                );
                answer.link = Some(format!("<{BASE}{PAGE2}>; rel=\"next\""));
                Ok(answer)
            }
            PAGE2 => Ok(json(
                &runners(&[("alpha-2", "offline", false), ("beta-1", "online", false)]),
                "\"p2\"",
            )),
            "/orgs/other/actions/runners?per_page=100" => {
                Ok(json(&runners(&[("other-1", "online", false)]), "\"p1\""))
            }
            RUNNERS => Ok(json(
                &runners(&[("app-1-20261112000000000", "online", false)]),
                "\"p1\"",
            )),
            other => panic!("unexpected request, including an organization job lookup: {other}"),
        }
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) = channels(
        organization_snapshot(
            &[
                ("owner", "alpha", "alpha-1"),
                ("owner", "alpha", "alpha-2"),
                ("owner", "beta", "beta-1"),
                ("other", "alpha", "other-1"),
            ],
            true,
            true,
        ),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(16)).await;
    })
    .await;
    let seen = world.fetch.seen();
    for path in [
        ORGANIZATION_RUNNERS,
        PAGE2,
        "/orgs/other/actions/runners?per_page=100",
        RUNNERS,
    ] {
        assert_eq!(
            seen.iter().filter(|request| request.path == path).count(),
            2,
            "{path}"
        );
    }
    assert!(seen.iter().all(|request| request.token
        == if request.path.starts_with("/orgs/") {
            ORGANIZATION_TOKEN
        } else {
            TOKEN
        }));
    let status = status_rx.borrow().clone();
    assert_eq!(status.repositories.len(), 1);
    assert_eq!(status.repositories[0].repository, "owner/app");
    assert_eq!(status.repositories[0].runners[0].github, "idle");
    let groups: BTreeMap<_, _> = status
        .organizations
        .iter()
        .map(|organization| {
            assert_eq!(organization.status, "ok");
            assert!(organization.checked_at.unwrap() >= START_MS + 15_000);
            (
                (
                    organization.organization.as_str(),
                    organization.runner_group.as_str(),
                ),
                organization
                    .runners
                    .iter()
                    .map(|runner| (runner.name.as_str(), runner.github.as_str()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        groups,
        BTreeMap::from([
            (
                ("owner", "alpha"),
                vec![("alpha-1", "busy"), ("alpha-2", "offline")]
            ),
            (("owner", "beta"), vec![("beta-1", "idle")]),
            (("other", "alpha"), vec![("other-1", "idle")]),
        ])
    );
    let value = serde_json::to_value(&*status).unwrap();
    for organization in value["organizations"].as_array().unwrap() {
        let keys: Vec<_> = organization
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "checked_at",
                "organization",
                "runner_group",
                "runners",
                "status"
            ]
        );
        for runner in organization["runners"].as_array().unwrap() {
            let keys: Vec<_> = runner
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["github", "name"]);
        }
    }
}

/// toggling the subscription must not shorten the interval measured from the
/// previous check; removing the final host runner also removes its display row.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn organization_checks_resume_only_at_the_exact_interval_and_stop_without_host_runners() {
    let world = organization_world(|path, _, _| {
        assert_eq!(path, ORGANIZATION_RUNNERS);
        Ok(json(&runners(&[("alpha-1", "online", false)]), "\"r\""))
    });
    let (snapshot_tx, snapshot_rx, active_tx, active_rx, status_tx, status_rx) = channels(
        organization_snapshot(&[("owner", "alpha", "alpha-1")], true, false),
        false,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(5)).await;
        assert!(world.fetch.seen().is_empty());
        active_tx.send_replace(true);
        sleep(Duration::from_secs(5)).await;
        active_tx.send_replace(false);
        sleep(Duration::from_secs(4)).await;
        active_tx.send_replace(true);
        sleep(Duration::from_millis(5999)).await;
        assert_eq!(world.fetch.seen().len(), 1);
        sleep(Duration::from_millis(2)).await;
        assert_eq!(world.fetch.seen().len(), 2);
        sleep(Duration::from_secs(1)).await;
        active_tx.send_replace(false);
        sleep(Duration::from_secs(60)).await;
        active_tx.send_replace(true);
        sleep(Duration::from_secs(1)).await;
        snapshot_tx.send_replace(organization_snapshot(
            &[("owner", "alpha", "")],
            true,
            false,
        ));
        sleep(Duration::from_secs(60)).await;
    })
    .await;
    let at: Vec<_> = world
        .fetch
        .seen()
        .iter()
        .map(|request| request.at)
        .collect();
    assert_eq!(
        at,
        [
            Duration::from_secs(5),
            Duration::from_secs(20),
            Duration::from_millis(81_001)
        ]
    );
    assert!(status_rx.borrow().organizations.is_empty());
}

/// summary-level token presence can arrive before a pool's copy in a snapshot;
/// neither representation licenses requests while both report no token.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_organization_without_a_token_is_not_checked_and_its_summary_can_supply_token_presence()
{
    let world = organization_world(|path, _, _| {
        assert_eq!(path, ORGANIZATION_RUNNERS);
        Ok(json(&runners(&[]), "\"r\""))
    });
    let (snapshot_tx, snapshot_rx, _a, active_rx, status_tx, _status_rx) = channels(
        organization_snapshot(&[("owner", "alpha", "alpha-1")], false, false),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(60)).await;
        assert!(world.fetch.seen().is_empty());
        let mut value = serde_json::to_value(&*organization_snapshot(
            &[("owner", "alpha", "alpha-1")],
            false,
            false,
        ))
        .unwrap();
        value["organizations"][0]["has_token"] = serde_json::json!(true);
        snapshot_tx.send_replace(Arc::new(serde_json::from_value(value).unwrap()));
        sleep(Duration::from_secs(1)).await;
    })
    .await;
    assert_eq!(world.fetch.paths(), [ORGANIZATION_RUNNERS]);
}

/// cached runner lists outlive individual pools, but must not keep deleted
/// configuration visible until the next scheduled GitHub request.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn removed_organization_groups_and_names_do_not_survive_cached_status() {
    let world = organization_world(|_, _, _| {
        Ok(json(
            &runners(&[
                ("alpha-1", "online", false),
                ("alpha-2", "online", false),
                ("beta-1", "online", false),
            ]),
            "\"r\"",
        ))
    });
    let (snapshot_tx, snapshot_rx, _a, active_rx, status_tx, status_rx) = channels(
        organization_snapshot(
            &[
                ("owner", "alpha", "alpha-1"),
                ("owner", "alpha", "alpha-2"),
                ("owner", "beta", "beta-1"),
            ],
            true,
            false,
        ),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(1)).await;
        snapshot_tx.send_replace(organization_snapshot(
            &[("owner", "alpha", "alpha-2")],
            true,
            false,
        ));
        sleep(Duration::from_secs(1)).await;
    })
    .await;
    let status = status_rx.borrow().clone();
    assert_eq!(world.fetch.paths(), [ORGANIZATION_RUNNERS]);
    assert_eq!(status.organizations.len(), 1);
    assert_eq!(status.organizations[0].runner_group, "alpha");
    assert_eq!(status.organizations[0].runners.len(), 1);
    assert_eq!(status.organizations[0].runners[0].name, "alpha-2");
}

/// a repository request is the control for a pause caused by organisation
/// headers, including the boundary where a new hourly budget becomes usable.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_organization_response_updates_and_pauses_the_one_host_budget() {
    let world = organization_world(|path, _, at| {
        let mut answer = json(&runners(&[]), "\"r\"");
        if at < Duration::from_secs(3600) {
            assert_eq!(
                path, ORGANIZATION_RUNNERS,
                "repository bypassed the organization's pause"
            );
            answer.rate = rate(500);
        } else {
            answer.rate = RateHeaders {
                reset: Some(START_MS / 1000 + 7200),
                ..rate(4999)
            };
        }
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) = channels(
        organization_snapshot(&[("owner", "alpha", "alpha-1")], true, true),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(3599)).await;
        assert_eq!(world.fetch.paths(), [ORGANIZATION_RUNNERS]);
        let status = status_rx.borrow().clone();
        let budget = status.budget.as_ref().unwrap();
        assert_eq!(
            (
                budget.limit,
                budget.remaining,
                budget.resets_at,
                budget.level.as_str()
            ),
            (5000, 500, START_MS + 3_600_000, "paused")
        );
        sleep(Duration::from_secs(2)).await;
    })
    .await;
    let seen = world.fetch.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1].at, Duration::from_secs(3600));
    assert!(seen[2].at >= Duration::from_secs(3600));
    assert!(seen.iter().skip(1).any(|request| request.path == RUNNERS));
    assert_eq!(status_rx.borrow().budget.as_ref().unwrap().level, "normal");
}

/// retry-after constrains all scopes, whereas a longer poll interval constrains
/// only the organisation that returned it.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn organization_secondary_limits_hold_repository_requests_and_poll_headers_apply() {
    let world = organization_world(|path, _, at| {
        if at < Duration::from_secs(100) {
            assert_eq!(path, ORGANIZATION_RUNNERS);
            let mut answer = status(429);
            answer.retry_after = Some(Duration::from_secs(100));
            return Ok(answer);
        }
        let mut answer = json(&runners(&[]), "\"r\"");
        if path == ORGANIZATION_RUNNERS {
            answer.poll_interval = Some(Duration::from_secs(40));
        }
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) = channels(
        organization_snapshot(&[("owner", "alpha", "alpha-1")], true, true),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(181)).await;
    })
    .await;
    let seen = world.fetch.seen();
    let org_at: Vec<_> = seen
        .iter()
        .filter(|request| request.path == ORGANIZATION_RUNNERS)
        .map(|request| request.at.as_secs())
        .collect();
    assert_eq!(org_at, [0, 100, 140, 180]);
    assert!(
        seen.iter()
            .filter(|request| request.path == RUNNERS)
            .all(|request| request.at >= Duration::from_secs(100))
    );
}

/// same-origin links can still cross the credential's scope or endpoint; path
/// normalisation must not turn such a link into an authorised next page.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn organization_paging_refuses_other_origins_scopes_and_endpoints() {
    for target in [
        "https://evil.example/orgs/owner/actions/runners?page=2".to_string(),
        format!("{BASE}/orgs/other/actions/runners?page=2"),
        format!("{BASE}/repos/owner/app/actions/runners?page=2"),
        format!("{BASE}/repositories/7/actions/runners?page=2"),
        format!("{BASE}/orgs/owner/actions/runner-groups?page=2"),
        format!("{BASE}/orgs/owner/actions/runners/../runner-groups?page=2"),
    ] {
        let world = organization_world(move |path, _, _| {
            assert_eq!(path, ORGANIZATION_RUNNERS, "followed a refused page");
            let mut answer = json(&runners(&[]), "\"r\"");
            answer.link = Some(format!("<{target}>; rel=\"next\""));
            Ok(answer)
        });
        let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) = channels(
            organization_snapshot(&[("owner", "alpha", "alpha-1")], true, false),
            true,
        );
        drive(&world, snapshot_rx, active_rx, status_tx, async {
            sleep(Duration::from_secs(1)).await;
        })
        .await;
        assert_eq!(world.fetch.paths(), [ORGANIZATION_RUNNERS]);
        assert_eq!(status_rx.borrow().organizations[0].status, "error");
    }
}

/// a successful busy state precedes each failure, distinguishing retained last
/// data from falsely treating an error response as a fresh successful check.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn organization_failures_back_off_without_exposing_tokens_or_response_fields() {
    for (code, cause) in [
        (401, "token_rejected"),
        (403, "token_rejected"),
        (404, "not_found"),
        (302, "moved"),
        (502, "error"),
    ] {
        let world = organization_world(move |path, _, at| {
            assert_eq!(path, ORGANIZATION_RUNNERS);
            if at < Duration::from_secs(15) {
                return Ok(json(&runners(&[("alpha-1", "online", true)]), "\"r\""));
            }
            let mut answer = status(code);
            answer.message = Some(format!("raw response {ORGANIZATION_TOKEN}"));
            answer.body = format!("raw body {ORGANIZATION_TOKEN}").into_bytes();
            Ok(answer)
        });
        let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) = channels(
            organization_snapshot(&[("owner", "alpha", "alpha-1")], true, false),
            true,
        );
        drive(&world, snapshot_rx, active_rx, status_tx, async {
            sleep(Duration::from_secs(62)).await;
        })
        .await;
        let at: Vec<_> = world
            .fetch
            .seen()
            .iter()
            .map(|request| request.at.as_secs())
            .collect();
        assert_eq!(at, [0, 15, 30, 60]);
        let status = status_rx.borrow().clone();
        assert_eq!(status.organizations[0].status, cause);
        assert_eq!(status.organizations[0].runners[0].github, "busy");
        assert_eq!(status.organizations[0].checked_at, Some(START_MS + 100));
        let value = serde_json::to_string(&*status).unwrap();
        assert!(!value.contains(ORGANIZATION_TOKEN));
        assert!(!value.contains("raw"));
        let explanation = match code {
            404 => Some("GitHub cannot find the organisation with its token"),
            302 => Some("the organisation has moved on GitHub"),
            _ => None,
        };
        if let Some(explanation) = explanation {
            assert!(
                world
                    .log
                    .lines()
                    .iter()
                    .any(|line| line.contains(explanation))
            );
            assert!(
                world
                    .log
                    .lines()
                    .iter()
                    .all(|line| !line.contains("repository"))
            );
        }
        assert!(
            world
                .log
                .lines()
                .iter()
                .all(|line| !line.contains(ORGANIZATION_TOKEN))
        );
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn repository_failures_keep_their_repository_explanation() {
    for (code, explanation) in [
        (404, "GitHub cannot find the repository with its token"),
        (302, "the repository has moved on GitHub"),
    ] {
        let world = World::new(move |path, _, _| {
            assert_eq!(path, RUNNERS);
            Ok(status(code))
        });
        let (_s, snapshot_rx, _a, active_rx, status_tx, _status_rx) =
            channels(snapshot(&["app-1-20261112000000000"], true), true);
        drive(&world, snapshot_rx, active_rx, status_tx, async {
            sleep(Duration::from_secs(1)).await;
        })
        .await;
        assert!(
            world
                .log
                .lines()
                .iter()
                .any(|line| line.contains(explanation))
        );
        assert!(
            world
                .log
                .lines()
                .iter()
                .all(|line| !line.contains("organisation"))
        );
    }
}

/// a store error can itself contain a secret; recovering the store must permit
/// the next scheduled attempt without exposing that error text.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_unreadable_organization_token_is_not_requested_or_logged_and_recovers() {
    let world = organization_world(|path, _, _| {
        assert_eq!(path, ORGANIZATION_RUNNERS);
        Ok(json(&runners(&[("alpha-1", "online", false)]), "\"r\""))
    });
    world.store.fail_with(Some(ORGANIZATION_TOKEN));
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) = channels(
        organization_snapshot(&[("owner", "alpha", "alpha-1")], true, false),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(1)).await;
        assert!(world.fetch.seen().is_empty());
        assert_eq!(
            status_rx.borrow().organizations[0].status,
            "token_unreadable"
        );
        world.store.fail_with(None);
        sleep(Duration::from_secs(15)).await;
    })
    .await;
    assert_eq!(world.fetch.seen()[0].at, Duration::from_secs(15));
    assert_eq!(status_rx.borrow().organizations[0].status, "ok");
    assert!(
        world
            .log
            .lines()
            .iter()
            .all(|line| !line.contains(ORGANIZATION_TOKEN))
    );
}

/// omission counts describe group entries, not distinct organisation logins;
/// all runner rows must be exhausted before group summaries are cut.
#[test]
fn organization_status_fits_by_counting_omitted_runners_then_groups() {
    let mut status: GithubStatus = serde_json::from_value(serde_json::json!({
        "organizations": (0..100).map(|index| serde_json::json!({
            "organization": "owner", "runner_group": format!("group-{index}"),
            "status": "ok", "runners": [{"name": "runner".repeat(50), "github": "idle"}]
        })).collect::<Vec<_>>()
    }))
    .unwrap();
    status.fit(2000);
    assert!(serde_json::to_vec(&status).unwrap().len() <= 2000);
    assert_eq!(status.omitted_runners, 100);
    assert!(status.omitted_organizations > 0);
    assert_eq!(
        status.organizations.len() + usize::try_from(status.omitted_organizations).unwrap(),
        100
    );
}

/// an in-flight response may finish after unsubscribe, but must not start its
/// next page; resubscription reauthorises cached pages within the interval rule.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn ending_a_subscription_during_an_organization_page_stops_the_next_page() {
    const PAGE2: &str = "/orgs/owner/actions/runners?per_page=100&page=2";
    let world = organization_world(|path, etag, _| match (path, etag) {
        (ORGANIZATION_RUNNERS, None) => {
            let mut answer = json(&runners(&[]), "\"p1\"");
            answer.link = Some(format!("<{BASE}{PAGE2}>; rel=\"next\""));
            Ok(answer)
        }
        (ORGANIZATION_RUNNERS, Some("\"p1\"")) => Ok(status(304)),
        (PAGE2, None) => Ok(json(&runners(&[("alpha-1", "online", true)]), "\"p2\"")),
        other => panic!("unexpected request {other:?}"),
    });
    let (_s, snapshot_rx, active_tx, active_rx, status_tx, status_rx) = channels(
        organization_snapshot(&[("owner", "alpha", "alpha-1")], true, false),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_millis(50)).await;
        active_tx.send_replace(false);
        sleep(Duration::from_millis(950)).await;
        assert_eq!(world.fetch.paths(), [ORGANIZATION_RUNNERS]);
        active_tx.send_replace(true);
        sleep(Duration::from_secs(15)).await;
    })
    .await;
    let seen = world.fetch.seen();
    assert_eq!(
        world.fetch.paths(),
        [ORGANIZATION_RUNNERS, ORGANIZATION_RUNNERS, PAGE2]
    );
    assert_eq!(seen[1].at, Duration::from_secs(15));
    assert_eq!(seen[2].at, Duration::from_millis(15_100));
    assert_eq!(
        status_rx.borrow().organizations[0].runners[0].github,
        "busy"
    );
}

/// a conditional response still carries budget evidence; a larger remaining
/// count in the same reset window must not shorten the conservative interval.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn organization_304_rate_headers_slow_checks_and_keep_the_lowest_budget() {
    let world = organization_world(|path, etag, at| {
        assert_eq!(path, ORGANIZATION_RUNNERS);
        let mut answer = if etag.is_some() {
            assert_eq!(etag, Some("\"r\""));
            status(304)
        } else {
            json(&runners(&[("alpha-1", "online", false)]), "\"r\"")
        };
        answer.rate = rate(if at < Duration::from_secs(60) {
            1250
        } else {
            4000
        });
        Ok(answer)
    });
    let (_s, snapshot_rx, _a, active_rx, status_tx, status_rx) = channels(
        organization_snapshot(&[("owner", "alpha", "alpha-1")], true, false),
        true,
    );
    drive(&world, snapshot_rx, active_rx, status_tx, async {
        sleep(Duration::from_secs(121)).await;
    })
    .await;
    let at: Vec<_> = world
        .fetch
        .seen()
        .iter()
        .map(|request| request.at)
        .collect();
    assert_eq!(
        at,
        [
            Duration::ZERO,
            Duration::from_secs(60),
            Duration::from_secs(120)
        ]
    );
    let status = status_rx.borrow().clone();
    let budget = status.budget.as_ref().unwrap();
    assert_eq!((budget.remaining, budget.level.as_str()), (1250, "slowed"));
    assert_eq!(status.organizations[0].runners[0].github, "idle");
}
