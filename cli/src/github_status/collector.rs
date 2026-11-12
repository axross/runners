//! the collector: while a client holds a renewed `github` subscription, it checks
//! each repository whose pools have a registered runner, one display request at a
//! time across the host, at most once per interval of the budget's level; it reads
//! the runner list in full, looks up the job of a busy runner whose job is not known,
//! and publishes the `github` topic's value after every answer.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::pending;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::watch;
use tokio::time::{Instant, sleep_until};

use crate::agent::snapshot::Snapshot;
use crate::runtime::log::Log;
use crate::runtime::secret::Secret;
use crate::token_store::{RepositoryKey, TokenStore};

use super::answer::{Answer, Cause, Endpoint, Next, Outcome, Wait, classify};
use super::budget::{Level, Tracker};
use super::clean;
use super::http::{Fetch, Get, next_page, repository_id};
use super::{GithubStatus, Job, RepositoryStatus, RunnerStatus};

/// runners per page of the runner list.
pub const RUNNERS_PER_PAGE: u32 = 100;
/// in-progress runs per page.
pub const RUNS_PER_PAGE: u32 = 10;
/// jobs per page of a run's jobs.
pub const JOBS_PER_PAGE: u32 = 30;
/// turns a busy runner's job is looked for before the lookup stops.
pub const LOOKUP_TURNS: u32 = 20;
/// the most pages of one paged read.
pub const MAX_PAGES: usize = 10;
/// the first wait after a failure, which doubles.
pub const FAILURE_BACKOFF: Duration = Duration::from_secs(15);
/// the longest wait after failures.
pub const MAX_FAILURE_BACKOFF: Duration = Duration::from_secs(5 * 60);
/// the first wait after a secondary limit without `retry-after`, which doubles.
pub const SECONDARY_BACKOFF: Duration = Duration::from_secs(60);
/// the longest wait after secondary limits.
pub const MAX_SECONDARY_BACKOFF: Duration = Duration::from_secs(15 * 60);
/// the least time between two tries of hidden job names.
pub const HIDDEN_RETRY: Duration = Duration::from_secs(60);

/// the most bytes of the value's JSON, leaving room for the event around it.
pub const MAX_STATUS_BYTES: usize = crate::agent::snapshot::MAX_SNAPSHOT_BYTES;

#[derive(Deserialize)]
struct RunnerItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    busy: bool,
}

#[derive(Deserialize)]
struct RunnersPage {
    #[serde(default)]
    runners: Vec<RunnerItem>,
}

#[derive(Deserialize)]
struct RunItem {
    id: u64,
}

#[derive(Deserialize)]
struct RunsPage {
    #[serde(default)]
    workflow_runs: Vec<RunItem>,
}

#[derive(Deserialize)]
struct JobItem {
    id: u64,
    #[serde(default)]
    run_id: u64,
    #[serde(default)]
    runner_name: Option<String>,
    #[serde(default)]
    workflow_name: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct JobsPage {
    #[serde(default)]
    jobs: Vec<JobItem>,
}

/// a page as last read: its `ETag`, its body and its `link` header.
#[derive(Clone)]
struct Page {
    etag: Option<String>,
    body: Vec<u8>,
    link: Option<String>,
}

/// what job names show for a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Names {
    Unknown,
    Shown,
    Hidden,
    Unreadable(Cause),
}

/// what the collector keeps for a repository, across subscriptions.
struct Repository {
    key: RepositoryKey,
    pages: BTreeMap<String, Page>,
    /// when its last check started.
    checked: Option<Instant>,
    /// when GitHub last answered its runner list, in milliseconds since the epoch.
    checked_at: Option<u64>,
    failure: Option<Cause>,
    backoff: Duration,
    retry: Option<Instant>,
    retry_at: Option<u64>,
    /// GitHub's latest `x-poll-interval` for the repository.
    poll: Duration,
    runners: Vec<RunnerStatus>,
    names: Names,
    /// how many turns each busy runner's job has been looked for.
    turns: BTreeMap<String, u32>,
    /// the run ids last read, newest first.
    runs: Vec<u64>,
    /// the jobs pages still to read, front first.
    queue: VecDeque<String>,
    hidden_tried: Option<Instant>,
    /// a runner became busy while names were hidden, and the retry waits for its minute.
    hidden_pending: bool,
    /// the wait after a failed lookup, which doubles, and when lookups resume.
    lookup_backoff: Duration,
    lookup_retry: Option<Instant>,
    lookup_retry_at: Option<u64>,
}

impl Repository {
    fn new(key: RepositoryKey) -> Self {
        Self {
            key,
            pages: BTreeMap::new(),
            checked: None,
            checked_at: None,
            failure: None,
            backoff: FAILURE_BACKOFF,
            retry: None,
            retry_at: None,
            poll: Duration::ZERO,
            runners: Vec::new(),
            names: Names::Unknown,
            turns: BTreeMap::new(),
            runs: Vec::new(),
            queue: VecDeque::new(),
            hidden_tried: None,
            hidden_pending: false,
            lookup_backoff: FAILURE_BACKOFF,
            lookup_retry: None,
            lookup_retry_at: None,
        }
    }

    /// when its next check is due at `level`; `None` while paused.
    fn due(&self, level: Level) -> Option<Instant> {
        let interval = level.interval()?.max(self.poll);
        let next = self.checked.map(|checked| checked + interval);
        Some(match (next, self.retry) {
            (Some(next), Some(retry)) => next.max(retry),
            (Some(next), None) => next,
            (None, Some(retry)) => retry,
            (None, None) => Instant::now(),
        })
    }

    fn status(&self) -> RepositoryStatus {
        let (job_names, job_names_cause) = match self.names {
            Names::Unknown => ("unknown", None),
            Names::Shown => ("shown", None),
            Names::Hidden => ("hidden", None),
            Names::Unreadable(cause) => ("unreadable", Some(cause.word().to_string())),
        };
        RepositoryStatus {
            repository: self.key.as_str().to_string(),
            checked_at: self.checked_at,
            status: self.failure.map_or("ok", Cause::word).to_string(),
            retry_at: self.retry_at,
            job_names: job_names.to_string(),
            job_names_cause,
            job_names_retry_at: matches!(self.names, Names::Unreadable(_))
                .then_some(self.lookup_retry_at)
                .flatten(),
            runners: self.runners.clone(),
        }
    }

    fn busy_without_job(&self) -> Vec<String> {
        self.runners
            .iter()
            .filter(|runner| runner.github == "busy" && runner.job.is_none())
            .filter(|runner| self.turns.get(&runner.name).copied().unwrap_or(0) < LOOKUP_TURNS)
            .map(|runner| runner.name.clone())
            .collect()
    }
}

/// the repositories to check now: each whose pools have a registered runner and
/// whose token the store had at the last look, with those runners' names.
#[must_use]
pub fn wanted(snapshot: &Snapshot) -> BTreeMap<RepositoryKey, BTreeSet<String>> {
    let with_token: BTreeSet<&str> = snapshot
        .repositories
        .iter()
        .filter(|repository| repository.has_token)
        .map(|repository| repository.repository.as_str())
        .collect();
    let mut wanted: BTreeMap<RepositoryKey, BTreeSet<String>> = BTreeMap::new();
    for pool in &snapshot.pools {
        if !pool.has_token && !with_token.contains(pool.repository.as_str()) {
            continue;
        }
        let Ok(key) = RepositoryKey::parse(&pool.repository) else {
            continue;
        };
        let names: BTreeSet<String> = pool
            .runners
            .iter()
            .filter_map(|runner| runner.workload.clone())
            .collect();
        if !names.is_empty() {
            wanted.entry(key).or_default().extend(names);
        }
    }
    wanted
}

/// what one paged read ended with.
enum Read {
    /// every page, as read or kept.
    Pages(Vec<Vec<u8>>),
    /// the turn ends: a rate limit, or collection stopped.
    Stop,
    /// a lookup answered a 403 that is not a rate limit.
    Hidden,
    Failed(Cause),
}

/// the collector and everything it keeps.
pub struct Collector<'a, F: Fetch> {
    fetch: &'a F,
    store: Option<Arc<dyn TokenStore>>,
    log: &'a dyn Log,
    /// the wall time in milliseconds since the epoch.
    now: Box<dyn Fn() -> u64 + 'a>,
    tracker: Tracker,
    level: Level,
    repositories: BTreeMap<RepositoryKey, Repository>,
    /// no display request starts before this, after a rate limit.
    host_wait: Option<Instant>,
    secondary: Duration,
    published: watch::Sender<Arc<GithubStatus>>,
    shown: BTreeSet<RepositoryKey>,
    active: watch::Receiver<bool>,
}

impl<'a, F: Fetch> Collector<'a, F> {
    #[must_use]
    pub fn new(
        fetch: &'a F,
        store: Option<Arc<dyn TokenStore>>,
        log: &'a dyn Log,
        now: Box<dyn Fn() -> u64 + 'a>,
        published: watch::Sender<Arc<GithubStatus>>,
        active: watch::Receiver<bool>,
    ) -> Self {
        Self {
            fetch,
            store,
            log,
            now,
            tracker: Tracker::default(),
            level: Level::Normal,
            repositories: BTreeMap::new(),
            host_wait: None,
            secondary: SECONDARY_BACKOFF,
            published,
            shown: BTreeSet::new(),
            active,
        }
    }

    fn is_active(&self) -> bool {
        *self.active.borrow()
    }

    /// checks GitHub while collection is wanted, for as long as it runs.
    pub async fn run(mut self, mut snapshots: watch::Receiver<Arc<Snapshot>>) {
        let mut active = self.active.clone();
        loop {
            let snapshot = Arc::clone(&snapshots.borrow_and_update());
            let wanted = wanted(&snapshot);
            self.show(&wanted);
            let next = if self.is_active() {
                self.next(&wanted)
            } else {
                None
            };
            match next {
                Some((key, at)) if at <= Instant::now() => {
                    if let Some(names) = wanted.get(&key) {
                        self.turn(&key, names).await;
                    }
                    continue;
                }
                _ => {}
            }
            let at = next.map(|(_, at)| at);
            tokio::select! {
                () = async { match at { Some(at) => sleep_until(at).await, None => pending().await } } => {}
                changed = snapshots.changed() => if changed.is_err() { return },
                changed = active.changed() => if changed.is_err() { return },
            }
        }
    }

    /// the repository due first, and when.
    fn next(
        &mut self,
        wanted: &BTreeMap<RepositoryKey, BTreeSet<String>>,
    ) -> Option<(RepositoryKey, Instant)> {
        let now_ms = (self.now)();
        self.note_level(now_ms);
        let level = self.tracker.level(now_ms);
        let floor = match self.tracker.paused_until(now_ms) {
            Some(until) => {
                Some(Instant::now() + Duration::from_millis(until.saturating_sub(now_ms)))
            }
            None => self.host_wait,
        };
        wanted
            .keys()
            .map(|key| {
                let repository = self
                    .repositories
                    .entry(key.clone())
                    .or_insert_with(|| Repository::new(key.clone()));
                let due = repository.due(level).unwrap_or_else(Instant::now);
                (key.clone(), floor.map_or(due, |floor| due.max(floor)))
            })
            .min_by_key(|(_, at)| *at)
    }

    /// logs a change of level.
    fn note_level(&mut self, now_ms: u64) {
        let level = self.tracker.level(now_ms);
        if level != self.level {
            self.log.info(&format!(
                "GitHub status: checks are now at the {} level.",
                level.word()
            ));
            self.level = level;
        }
    }

    /// publishes the value for the repositories wanted now.
    fn show(&mut self, wanted: &BTreeMap<RepositoryKey, BTreeSet<String>>) {
        let shown: BTreeSet<RepositoryKey> = wanted.keys().cloned().collect();
        if shown != self.shown {
            self.shown = shown;
            self.publish();
        }
    }

    fn publish(&mut self) {
        let now_ms = (self.now)();
        self.note_level(now_ms);
        let mut status = GithubStatus {
            budget: self.tracker.budget(now_ms),
            repositories: self
                .shown
                .iter()
                .filter_map(|key| self.repositories.get(key))
                .map(Repository::status)
                .collect(),
            omitted_runners: 0,
        };
        status.fit(MAX_STATUS_BYTES);
        self.published.send_if_modified(|current| {
            if **current == status {
                false
            } else {
                *current = Arc::new(status);
                true
            }
        });
    }

    /// one repository's turn: the runner list, then the job lookup.
    async fn turn(&mut self, key: &RepositoryKey, names: &BTreeSet<String>) {
        let token = match self.store.as_ref().map(|store| store.get(key)) {
            Some(Ok(Some(token))) => {
                self.log.mask(&token);
                token
            }
            Some(Ok(None)) | None => {
                if let Some(repository) = self.repositories.get_mut(key) {
                    repository.checked = Some(Instant::now());
                }
                return;
            }
            Some(Err(_)) => {
                self.fail(key, Cause::TokenUnreadable);
                self.publish();
                return;
            }
        };
        if let Some(repository) = self.repositories.get_mut(key) {
            repository.checked = Some(Instant::now());
        }
        let path = format!(
            "/repos/{}/actions/runners?per_page={RUNNERS_PER_PAGE}",
            key.as_str()
        );
        let pages = match self.read(key, &token, &path, Endpoint::Runners, true).await {
            Read::Pages(pages) => pages,
            Read::Failed(cause) => {
                self.fail(key, cause);
                self.publish();
                return;
            }
            Read::Stop | Read::Hidden => {
                self.publish();
                return;
            }
        };
        let parsed: Option<Vec<RunnersPage>> = pages
            .iter()
            .map(|body| serde_json::from_slice(body).ok())
            .collect();
        let Some(parsed) = parsed else {
            // a page GitHub answered with 200 that is not a runner list
            self.fail(key, Cause::Error);
            self.publish();
            return;
        };
        let became_busy = self.take_runners(key, names, parsed);
        self.publish();
        self.lookup(key, &token, became_busy).await;
        self.publish();
    }

    /// records a failure of the runner list and when it is tried again.
    fn fail(&mut self, key: &RepositoryKey, cause: Cause) {
        let level_interval = self
            .tracker
            .level((self.now)())
            .interval()
            .unwrap_or(Duration::ZERO);
        let now_ms = (self.now)();
        let Some(repository) = self.repositories.get_mut(key) else {
            return;
        };
        let wait = repository.backoff.max(level_interval);
        if repository.failure != Some(cause) {
            self.log.warn(&format!(
                "GitHub status: {} failed: {}; trying again in {} s.",
                key.as_str(),
                describe(cause),
                wait.as_secs()
            ));
        }
        repository.failure = Some(cause);
        repository.retry = Some(Instant::now() + wait);
        repository.retry_at = Some(now_ms + u64::try_from(wait.as_millis()).unwrap_or(u64::MAX));
        repository.backoff = (repository.backoff * 2).min(MAX_FAILURE_BACKOFF);
    }

    /// reads every page of a paged read, conditionally, and takes the budget from
    /// each answer; `follow` reads past the first page.
    async fn read(
        &mut self,
        key: &RepositoryKey,
        token: &Secret,
        first: &str,
        endpoint: Endpoint,
        follow: bool,
    ) -> Read {
        let mut bodies = Vec::new();
        let mut path = first.to_string();
        // the `/repositories/<id>` id of the read, held from the first page that names one
        let mut held_id: Option<String> = None;
        for _ in 0..MAX_PAGES {
            if !self.is_active() || self.waiting() {
                return Read::Stop;
            }
            let etag = self
                .repositories
                .get(key)
                .and_then(|repository| repository.pages.get(&path))
                .and_then(|page| page.etag.clone());
            let result = self
                .fetch
                .get(&Get {
                    path: &path,
                    token,
                    etag: etag.as_deref(),
                })
                .await;
            let answer = match result {
                Ok(answer) => answer,
                Err(cause) => {
                    self.log.info(&format!(
                        "GitHub status: GET {path} got no usable answer: {}.",
                        describe(cause)
                    ));
                    return Read::Failed(cause);
                }
            };
            let now_ms = (self.now)();
            self.tracker.observe(&answer.rate, now_ms);
            self.log_answer(&path, endpoint, &answer, token);
            if let Some(poll) = answer.poll_interval
                && let Some(repository) = self.repositories.get_mut(key)
            {
                repository.poll = poll;
            }
            let outcome = classify(endpoint, &answer);
            let page = match outcome {
                Outcome::Fresh => {
                    self.secondary = SECONDARY_BACKOFF;
                    let page = Page {
                        etag: answer.etag.clone(),
                        body: answer.body.clone(),
                        link: answer.link.clone(),
                    };
                    if let Some(repository) = self.repositories.get_mut(key) {
                        repository.pages.insert(path.clone(), page.clone());
                    }
                    page
                }
                Outcome::Unchanged => {
                    self.secondary = SECONDARY_BACKOFF;
                    let kept = self
                        .repositories
                        .get(key)
                        .and_then(|repository| repository.pages.get(&path).cloned());
                    match kept {
                        Some(page) => page,
                        None => return Read::Failed(Cause::Error),
                    }
                }
                Outcome::RateLimited(wait) => {
                    self.limit(wait);
                    return Read::Stop;
                }
                Outcome::Hidden => return Read::Hidden,
                Outcome::Failed(cause) => return Read::Failed(cause),
            };
            bodies.push(page.body);
            if held_id.is_none() {
                held_id = repository_id(&path).map(str::to_string);
            }
            let next = page.link.as_deref().map_or(Next::None, |link| {
                next_page(link, self.fetch.base_url(), first, held_id.as_deref())
            });
            match next {
                Next::Page(next) if follow => path = next,
                Next::Refused => return Read::Failed(Cause::Error),
                Next::Page(_) | Next::None => return Read::Pages(bodies),
            }
        }
        Read::Failed(Cause::Error)
    }

    fn waiting(&self) -> bool {
        self.host_wait.is_some_and(|until| until > Instant::now())
            || self.tracker.paused_until((self.now)()).is_some()
    }

    /// holds every display request back after a rate-limit answer.
    fn limit(&mut self, wait: Wait) {
        let now = Instant::now();
        let until = match wait {
            Wait::UntilReset => {
                self.tracker.reached();
                if self.tracker.paused_until((self.now)()).is_some() {
                    self.log.warn("GitHub status: GitHub says the hourly limit is reached; checks pause until the reset.");
                    return;
                }
                // no reset known to pause until, so wait as for a secondary limit
                let duration = self.secondary;
                self.secondary = (self.secondary * 2).min(MAX_SECONDARY_BACKOFF);
                now + duration
            }
            Wait::For(duration) => now + duration,
            Wait::Secondary(retry_after) => {
                let duration = retry_after.unwrap_or(self.secondary);
                self.secondary = (self.secondary * 2).min(MAX_SECONDARY_BACKOFF);
                now + duration
            }
        };
        self.log.warn(&format!(
            "GitHub status: GitHub asked for a pause; checks wait {} s.",
            until.saturating_duration_since(now).as_secs()
        ));
        self.host_wait = Some(self.host_wait.map_or(until, |current| current.max(until)));
    }

    fn log_answer(&self, path: &str, endpoint: Endpoint, answer: &Answer, token: &Secret) {
        let remaining = answer
            .rate
            .remaining
            .map_or_else(|| "unknown".to_string(), |remaining| remaining.to_string());
        let mut line = format!(
            "GitHub status: GET {path} answered {}; {remaining} requests left.",
            answer.status
        );
        if endpoint != Endpoint::Runners && matches!(answer.status, 403 | 404) {
            line.push_str(&format!(
                " GitHub's message: {}. X-Accepted-GitHub-Permissions: {}.",
                answer
                    .message
                    .as_deref()
                    .map_or_else(|| "none".to_string(), |message| scrub(message, token)),
                answer.accepted_permissions.as_deref().unwrap_or("none")
            ));
        }
        self.log.info(&line);
    }

    /// takes a fresh runner list; returns whether a runner became busy.
    fn take_runners(
        &mut self,
        key: &RepositoryKey,
        names: &BTreeSet<String>,
        pages: Vec<RunnersPage>,
    ) -> bool {
        let now_ms = (self.now)();
        let Some(repository) = self.repositories.get_mut(key) else {
            return false;
        };
        let listed: Vec<RunnerItem> = pages
            .into_iter()
            .flat_map(|page| page.runners)
            .filter(|item| names.contains(&item.name))
            .collect();
        let mut became_busy = false;
        let runners: Vec<RunnerStatus> = listed
            .into_iter()
            .map(|item| {
                let github = match (item.status.as_str(), item.busy) {
                    ("online", true) => "busy",
                    ("online", false) => "idle",
                    _ => "offline",
                };
                let before = repository
                    .runners
                    .iter()
                    .find(|runner| runner.name == item.name);
                if before.map(|runner| runner.github.as_str()) != Some(github) {
                    self.log.info(&format!(
                        "GitHub status: runner {} is {github} on GitHub.",
                        item.name
                    ));
                    became_busy |= github == "busy";
                }
                RunnerStatus {
                    job: before.and_then(|runner| runner.job.clone()),
                    name: item.name,
                    github: github.to_string(),
                }
            })
            .collect();
        for gone in repository
            .runners
            .iter()
            .filter(|runner| !runners.iter().any(|now| now.name == runner.name))
        {
            self.log.info(&format!(
                "GitHub status: runner {} is not listed on GitHub.",
                gone.name
            ));
        }
        repository.turns.retain(|name, _| names.contains(name));
        repository.runners = runners;
        repository.checked_at = Some(now_ms);
        repository.failure = None;
        repository.retry = None;
        repository.retry_at = None;
        repository.backoff = FAILURE_BACKOFF;
        became_busy
    }

    /// the job lookup of a repository's turn: at most two requests while a busy
    /// runner's job is not known, or one to try hidden names again.
    async fn lookup(&mut self, key: &RepositoryKey, token: &Secret, became_busy: bool) {
        let Some(repository) = self.repositories.get(key) else {
            return;
        };
        let waiting = repository.busy_without_job();
        if waiting.is_empty() {
            return;
        }
        if repository.names == Names::Hidden {
            let due = repository
                .hidden_tried
                .is_none_or(|tried| tried + HIDDEN_RETRY <= Instant::now());
            let pending = repository.hidden_pending || became_busy;
            if let Some(repository) = self.repositories.get_mut(key) {
                repository.hidden_pending = pending && !due;
                if pending && due {
                    repository.hidden_tried = Some(Instant::now());
                }
            }
            if pending && due {
                self.read_runs(key, token).await;
            }
            return;
        }
        if repository
            .lookup_retry
            .is_some_and(|retry| retry > Instant::now())
        {
            return;
        }
        if let Some(repository) = self.repositories.get_mut(key) {
            for name in &waiting {
                *repository.turns.entry(name.clone()).or_insert(0) += 1;
            }
        }
        if !self.read_runs(key, token).await {
            return;
        }
        let page = {
            let Some(repository) = self.repositories.get_mut(key) else {
                return;
            };
            if repository.queue.is_empty() {
                let runs = repository.runs.clone();
                repository.queue.extend(runs.iter().map(|run| {
                    format!(
                        "/repos/{}/actions/runs/{run}/jobs?filter=latest&per_page={JOBS_PER_PAGE}",
                        key.as_str()
                    )
                }));
            }
            repository.queue.pop_front()
        };
        let Some(page) = page else {
            return;
        };
        match self.read(key, token, &page, Endpoint::Jobs, false).await {
            Read::Pages(bodies) => {
                let parsed: Option<Vec<JobsPage>> = bodies
                    .iter()
                    .map(|body| serde_json::from_slice(body).ok())
                    .collect();
                let Some(parsed) = parsed else {
                    self.lookup_failed(key, Cause::Error);
                    return;
                };
                self.lookup_answered(key);
                self.take_jobs(key, &waiting, parsed);
                let next = self
                    .repositories
                    .get(key)
                    .and_then(|repository| repository.pages.get(&page))
                    .and_then(|kept| kept.link.clone())
                    .map(|link| next_page(&link, self.fetch.base_url(), &page, None));
                if let (Some(Next::Page(next)), Some(repository)) =
                    (next, self.repositories.get_mut(key))
                {
                    repository.queue.push_front(next);
                }
            }
            Read::Hidden => self.names(key, Names::Hidden),
            Read::Failed(cause) => self.lookup_failed(key, cause),
            Read::Stop => {}
        }
    }

    /// records a failed lookup: job names are unreadable, and lookups wait 15 s,
    /// doubling to 5 minutes, never less than the level's interval.
    fn lookup_failed(&mut self, key: &RepositoryKey, cause: Cause) {
        let now_ms = (self.now)();
        let level_interval = self
            .tracker
            .level(now_ms)
            .interval()
            .unwrap_or(Duration::ZERO);
        self.names(key, Names::Unreadable(cause));
        if let Some(repository) = self.repositories.get_mut(key) {
            let wait = repository.lookup_backoff.max(level_interval);
            repository.lookup_retry = Some(Instant::now() + wait);
            repository.lookup_retry_at =
                Some(now_ms + u64::try_from(wait.as_millis()).unwrap_or(u64::MAX));
            repository.lookup_backoff = (repository.lookup_backoff * 2).min(MAX_FAILURE_BACKOFF);
        }
    }

    /// records a lookup GitHub answered.
    fn lookup_answered(&mut self, key: &RepositoryKey) {
        if let Some(repository) = self.repositories.get_mut(key) {
            repository.lookup_backoff = FAILURE_BACKOFF;
            repository.lookup_retry = None;
            repository.lookup_retry_at = None;
        }
    }

    /// reads the in-progress runs; true when they were read.
    async fn read_runs(&mut self, key: &RepositoryKey, token: &Secret) -> bool {
        let path = format!(
            "/repos/{}/actions/runs?status=in_progress&per_page={RUNS_PER_PAGE}",
            key.as_str()
        );
        match self.read(key, token, &path, Endpoint::Runs, false).await {
            Read::Pages(bodies) => {
                let Some(page) = bodies
                    .first()
                    .and_then(|body| serde_json::from_slice::<RunsPage>(body).ok())
                else {
                    self.lookup_failed(key, Cause::Error);
                    return false;
                };
                let runs: Vec<u64> = page.workflow_runs.iter().map(|run| run.id).collect();
                self.lookup_answered(key);
                self.names(key, Names::Shown);
                if let Some(repository) = self.repositories.get_mut(key) {
                    if repository.runs != runs {
                        repository.queue.clear();
                        // a run no longer in progress needs its jobs pages no more
                        repository.pages.retain(|path, _| {
                            !path.contains("/jobs?")
                                || runs
                                    .iter()
                                    .any(|run| path.contains(&format!("/actions/runs/{run}/jobs?")))
                        });
                    }
                    repository.runs = runs;
                }
                true
            }
            Read::Hidden => {
                self.names(key, Names::Hidden);
                false
            }
            Read::Failed(cause) => {
                self.lookup_failed(key, cause);
                false
            }
            Read::Stop => false,
        }
    }

    fn names(&mut self, key: &RepositoryKey, names: Names) {
        if let Some(repository) = self.repositories.get_mut(key)
            && repository.names != names
        {
            let word = match names {
                Names::Unknown => "unknown".to_string(),
                Names::Shown => "shown".to_string(),
                Names::Hidden => "hidden: the token has no Actions: read".to_string(),
                Names::Unreadable(cause) => format!("unreadable: {}", describe(cause)),
            };
            self.log.info(&format!(
                "GitHub status: job names for {} are {word}.",
                key.as_str()
            ));
            if names == Names::Hidden {
                repository.hidden_tried = Some(Instant::now());
            }
            repository.names = names;
        }
    }

    fn take_jobs(&mut self, key: &RepositoryKey, waiting: &[String], pages: Vec<JobsPage>) {
        let Some(repository) = self.repositories.get_mut(key) else {
            return;
        };
        for job in pages
            .into_iter()
            .flat_map(|page| page.jobs)
            .filter(|job| job.status != "completed")
        {
            let Some(runner_name) = job.runner_name.as_deref() else {
                continue;
            };
            if !waiting.iter().any(|name| name == runner_name) {
                continue;
            }
            if let Some(runner) = repository
                .runners
                .iter_mut()
                .find(|runner| runner.name == runner_name)
            {
                runner.job = Some(Job {
                    workflow: clean::name(job.workflow_name.as_deref().unwrap_or("")),
                    name: clean::name(&job.name),
                    run_id: job.run_id,
                    job_id: job.id,
                });
                self.log.info(&format!(
                    "GitHub status: runner {runner_name} is running job {} of run {}.",
                    job.id, job.run_id
                ));
            }
        }
    }
}

/// GitHub's text with the token replaced, should GitHub ever repeat it.
fn scrub(text: &str, token: &Secret) -> String {
    if token.expose().is_empty() {
        text.to_string()
    } else {
        text.replace(token.expose(), "<token>")
    }
}

/// a cause in words, for the log.
fn describe(cause: Cause) -> &'static str {
    match cause {
        Cause::TokenRejected => "GitHub rejected the token",
        Cause::NotFound => "GitHub cannot find the repository with its token",
        Cause::Moved => "the repository has moved on GitHub",
        Cause::Unreachable => "GitHub could not be reached",
        Cause::Error => "GitHub answered with an error",
        Cause::TokenUnreadable => "the token cannot be read",
    }
}
