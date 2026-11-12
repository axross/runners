//! fakes for the session's seams: an agent that answers as scripted, a launcher that
//! brings it up, a screen that remembers what it was shown, and a clock the test moves
//! by sleeping on tokio's paused one.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use marshal_cli::agent::Launch;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::wire::UpdateEnding;
use marshal_session::{
    AgentInfo, Connected, Dial, DialError, Ended, Failure, Handle, Later, Launcher, Link, Log,
    Method, Present, Process, Processes, Pushed, Record, Seams, Session, Shown, TimeSource,
    Updater, Verified,
};
use marshal_view::{Clock, View};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// 2026-11-04 03:52 UTC, after the times the fixtures hold.
pub const NOW_MS: u64 = 1_793_764_320_000;

pub fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn fixture(name: &str) -> Snapshot {
    patched(name, |_| {})
}

pub fn patched(name: &str, change: impl FnOnce(&mut serde_json::Value)) -> Snapshot {
    let path = format!(
        "{}/../view/tests/fixtures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    change(&mut value);
    serde_json::from_value(value).unwrap()
}

// ---- the agent

/// what a call to the fake agent was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Called {
    StartAll,
    StopAll,
    Build,
    Quit,
    TokenSet { repository: String, token: String },
    TokenRemove { repository: String },
    Drain { version: String },
}

#[derive(Default)]
struct AgentState {
    calls: Vec<Called>,
    /// the answers to calls, by name; a call not named is answered `Ok`.
    answers: Vec<(&'static str, Result<(), Failure>)>,
    subscribe: Option<Result<(), Failure>>,
    /// a call that is held until the test lets it go.
    held: Vec<(&'static str, Arc<tokio::sync::Notify>)>,
    /// the process ID a drain answers with.
    pid: Option<u32>,
}

/// the test's end of a fake agent.
#[derive(Clone)]
pub struct Agent {
    state: Arc<Mutex<AgentState>>,
    pushes: mpsc::UnboundedSender<Pushed>,
}

pub struct FakeLink {
    state: Arc<Mutex<AgentState>>,
}

fn name(method: &Method) -> &'static str {
    match method {
        Method::StartAll => "start_all",
        Method::StopAll => "stop_all",
        Method::Build => "build",
        Method::Quit => "quit",
        Method::TokenSet { .. } => "token_set",
        Method::TokenRemove { .. } => "token_remove",
    }
}

impl Link for FakeLink {
    async fn subscribe(&self) -> Result<(), Failure> {
        locked(&self.state).subscribe.clone().unwrap_or(Ok(()))
    }

    async fn call(&self, method: Method) -> Result<(), Failure> {
        let key = name(&method);
        let called = match method {
            Method::StartAll => Called::StartAll,
            Method::StopAll => Called::StopAll,
            Method::Build => Called::Build,
            Method::Quit => Called::Quit,
            Method::TokenSet { repository, token } => Called::TokenSet {
                repository,
                token: token.expose().to_string(),
            },
            Method::TokenRemove { repository } => Called::TokenRemove { repository },
        };
        let (answer, held) = {
            let mut state = locked(&self.state);
            state.calls.push(called);
            let answer = state
                .answers
                .iter()
                .find(|(named, _)| *named == key)
                .map_or(Ok(()), |(_, answer)| answer.clone());
            let held = state
                .held
                .iter()
                .find(|(named, _)| *named == key)
                .map(|(_, gate)| Arc::clone(gate));
            (answer, held)
        };
        if let Some(gate) = held {
            gate.notified().await;
        }
        answer
    }

    async fn drain_for_update(&self, version: &str) -> Result<Option<u32>, Failure> {
        let (answer, held, pid) = {
            let mut state = locked(&self.state);
            state.calls.push(Called::Drain {
                version: version.to_string(),
            });
            let answer = state
                .answers
                .iter()
                .find(|(named, _)| *named == "drain_for_update")
                .map_or(Ok(()), |(_, answer)| answer.clone());
            let held = state
                .held
                .iter()
                .find(|(named, _)| *named == "drain_for_update")
                .map(|(_, gate)| Arc::clone(gate));
            (answer, held, state.pid)
        };
        if let Some(gate) = held {
            gate.notified().await;
        }
        answer.map(|()| pid)
    }
}

impl Agent {
    /// a connection to hand to the session, and the test's end of it.
    pub fn new() -> (Self, Connected<FakeLink>) {
        Self::reporting(AgentInfo {
            version: "1.3.0".to_string(),
            drains: true,
        })
    }

    /// an agent that says this of itself in `welcome`.
    pub fn reporting(agent: AgentInfo) -> (Self, Connected<FakeLink>) {
        let state = Arc::new(Mutex::new(AgentState::default()));
        let (pushes, receiver) = mpsc::unbounded_channel();
        (
            Self {
                state: Arc::clone(&state),
                pushes,
            },
            Connected {
                link: FakeLink { state },
                pushes: receiver,
                agent,
            },
        )
    }

    pub fn push(&self, snapshot: Snapshot) {
        let _ = self.pushes.send(Pushed::Snapshot(Box::new(snapshot)));
    }

    pub fn heartbeat(&self) {
        let _ = self.pushes.send(Pushed::Heartbeat);
    }

    pub fn end(&self, reason: &str) {
        let _ = self.pushes.send(Pushed::Ending {
            reason: reason.to_string(),
            update: None,
        });
    }

    /// says the agent is ending for the update to `version`.
    pub fn end_for_update(&self, version: &str, stopped_by_you: bool) {
        let _ = self.pushes.send(Pushed::Ending {
            reason: "update".to_string(),
            update: Some(UpdateEnding::new(version, stopped_by_you)),
        });
    }

    /// the process ID the agent's drain answers with.
    pub fn answer_pid(&self, pid: u32) {
        locked(&self.state).pid = Some(pid);
    }

    pub fn close(&self) {
        let _ = self.pushes.send(Pushed::Closed);
    }

    pub fn calls(&self) -> Vec<Called> {
        locked(&self.state).calls.clone()
    }

    pub fn answer(&self, method: &'static str, answer: Result<(), Failure>) {
        locked(&self.state).answers.push((method, answer));
    }

    pub fn subscription(&self, answer: Result<(), Failure>) {
        locked(&self.state).subscribe = Some(answer);
    }

    /// holds every call of `method` until the returned gate is notified.
    pub fn hold(&self, method: &'static str) -> Arc<tokio::sync::Notify> {
        let gate = Arc::new(tokio::sync::Notify::new());
        locked(&self.state).held.push((method, Arc::clone(&gate)));
        gate
    }
}

// ---- the world the session dials into and starts the agent in

pub enum Answer {
    NoAgent,
    NotAnswering,
    Busy,
    LockHeld,
    Failed,
    /// the agent answers after a while, as the library's client does.
    After(Duration, Box<Answer>),
    Agent(Connected<FakeLink>),
}

#[derive(Default)]
struct WorldState {
    queue: VecDeque<Answer>,
    /// the answer once the queue is empty.
    steady: Option<Answer>,
    /// the answer from the first start on.
    after_start: Option<Answer>,
    dials: Vec<Instant>,
    started: Vec<(Launch, Instant)>,
    start_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct World(Arc<Mutex<WorldState>>);

impl World {
    /// what the next dial answers.
    pub fn then(&self, answer: Answer) -> &Self {
        locked(&self.0).queue.push_back(answer);
        self
    }

    /// what every dial answers once nothing is queued.
    pub fn always(&self, answer: Answer) -> &Self {
        locked(&self.0).steady = Some(answer);
        self
    }

    /// what every dial answers from the moment the agent is started.
    pub fn up_after_start(&self, answer: Answer) -> &Self {
        locked(&self.0).after_start = Some(answer);
        self
    }

    pub fn refuse_start(&self, why: &str) {
        locked(&self.0).start_error = Some(why.to_string());
    }

    pub fn dials(&self) -> usize {
        locked(&self.0).dials.len()
    }

    /// the times, in seconds from `since`, each dial was made.
    pub fn dial_times(&self, since: Instant) -> Vec<u64> {
        locked(&self.0)
            .dials
            .iter()
            .map(|at| at.duration_since(since).as_secs())
            .collect()
    }

    pub fn started(&self) -> Vec<Launch> {
        locked(&self.0)
            .started
            .iter()
            .map(|(launch, _)| *launch)
            .collect()
    }
}

/// resolves an answer, which may be taken only once, to what a dial gives.
async fn resolve(answer: Answer) -> Result<Connected<FakeLink>, DialError> {
    let mut answer = answer;
    loop {
        match answer {
            Answer::NoAgent => return Err(DialError::NoAgent),
            Answer::NotAnswering => return Err(DialError::NotAnswering),
            Answer::Busy => return Err(DialError::Busy),
            Answer::LockHeld => return Err(DialError::LockHeld),
            Answer::Failed => return Err(DialError::Failed("it failed".to_string())),
            Answer::Agent(connected) => return Ok(connected),
            Answer::After(wait, next) => {
                tokio::time::sleep(wait).await;
                answer = *next;
            }
        }
    }
}

/// an answer for a steady state, which can be given more than once; an agent can only be
/// given once.
fn repeat(answer: &Answer) -> Answer {
    match answer {
        Answer::NoAgent => Answer::NoAgent,
        Answer::NotAnswering => Answer::NotAnswering,
        Answer::Busy => Answer::Busy,
        Answer::LockHeld => Answer::LockHeld,
        Answer::Failed => Answer::Failed,
        Answer::After(wait, next) => Answer::After(*wait, Box::new(repeat(next))),
        Answer::Agent(_) => panic!("an agent is given once"),
    }
}

impl Dial for World {
    type Link = FakeLink;

    async fn dial(&self) -> Result<Connected<FakeLink>, DialError> {
        let answer = {
            let mut world = locked(&self.0);
            world.dials.push(Instant::now());
            match world.queue.pop_front() {
                Some(answer) => answer,
                None => match world.steady.as_ref() {
                    Some(steady) => repeat(steady),
                    None => Answer::NoAgent,
                },
            }
        };
        resolve(answer).await
    }
}

impl Launcher for World {
    fn start_agent(&self, launch: Launch) -> Result<(), String> {
        let mut world = locked(&self.0);
        if let Some(why) = world.start_error.clone() {
            return Err(why);
        }
        world.started.push((launch, Instant::now()));
        if let Some(answer) = world.after_start.take() {
            world.queue.clear();
            world.queue.push_back(answer);
        }
        Ok(())
    }
}

// ---- the screen, the clock and the log

#[derive(Default)]
pub struct Screen(Mutex<Vec<Shown>>);

impl Screen {
    pub fn shown(&self) -> Vec<Shown> {
        locked(&self.0).clone()
    }

    pub fn latest(&self) -> Shown {
        locked(&self.0)
            .last()
            .cloned()
            .expect("something was shown")
    }

    pub fn view(&self) -> View {
        self.latest().view
    }

    pub fn announced(&self) -> Vec<String> {
        locked(&self.0)
            .iter()
            .flat_map(|shown| shown.announce.clone())
            .collect()
    }
}

impl Present for Screen {
    fn present(&self, shown: &Shown) {
        locked(&self.0).push(shown.clone());
    }
}

/// the wall clock, moved by tokio's paused one.
pub struct FakeTime {
    start: Instant,
}

impl TimeSource for FakeTime {
    fn clock(&self) -> Clock {
        let elapsed = u64::try_from(self.start.elapsed().as_millis()).unwrap();
        Clock::new(NOW_MS + elapsed, 0)
    }
}

#[derive(Default)]
pub struct MemoryLog(Mutex<Vec<String>>);

impl MemoryLog {
    pub fn lines(&self) -> Vec<String> {
        locked(&self.0).clone()
    }
}

impl Log for MemoryLog {
    fn info(&self, line: &str) {
        locked(&self.0).push(line.to_string());
    }

    fn warn(&self, line: &str) {
        locked(&self.0).push(format!("WARNING: {line}"));
    }
}

// ---- the update feed, the agent's process and the record

#[derive(Default)]
struct UpdatesState {
    /// what each check finds, in order; once none are left, no update.
    checks: VecDeque<Result<Option<String>, String>>,
    checked: Vec<Instant>,
    installed: Vec<String>,
    install_error: Option<String>,
    /// processes by ID: whether each has exited, and when it does
    processes: Vec<(u32, Arc<FakeProcessState>)>,
    record: Vec<RecordEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordEvent {
    Written(String),
    Removed,
}

#[derive(Default)]
struct FakeProcessState {
    exited: Mutex<bool>,
    notify: tokio::sync::Notify,
}

#[derive(Clone, Default)]
pub struct Updates(Arc<Mutex<UpdatesState>>);

impl Updates {
    /// the next check finds the update to `version`, downloaded and verified.
    pub fn offer(&self, version: &str) -> &Self {
        locked(&self.0)
            .checks
            .push_back(Ok(Some(version.to_string())));
        self
    }

    pub fn fail_check(&self, why: &str) -> &Self {
        locked(&self.0).checks.push_back(Err(why.to_string()));
        self
    }

    pub fn refuse_install(&self, why: &str) {
        locked(&self.0).install_error = Some(why.to_string());
    }

    /// a running process with this ID.
    pub fn process(&self, pid: u32) -> &Self {
        locked(&self.0)
            .processes
            .push((pid, Arc::new(FakeProcessState::default())));
        self
    }

    pub fn exit(&self, pid: u32) {
        let state = locked(&self.0)
            .processes
            .iter()
            .find(|(id, _)| *id == pid)
            .map(|(_, state)| Arc::clone(state));
        if let Some(state) = state {
            *locked(&state.exited) = true;
            state.notify.notify_waiters();
        }
    }

    pub fn checks(&self, since: Instant) -> Vec<u64> {
        locked(&self.0)
            .checked
            .iter()
            .map(|at| at.duration_since(since).as_secs())
            .collect()
    }

    pub fn installed(&self) -> Vec<String> {
        locked(&self.0).installed.clone()
    }

    pub fn record(&self) -> Vec<RecordEvent> {
        locked(&self.0).record.clone()
    }
}

struct FakeVerified {
    version: String,
    updates: Updates,
}

impl Verified for FakeVerified {
    fn version(&self) -> &str {
        &self.version
    }

    fn install(self: Box<Self>) -> Result<(), String> {
        let mut state = locked(&self.updates.0);
        if let Some(why) = state.install_error.clone() {
            return Err(why);
        }
        state.installed.push(self.version.clone());
        Ok(())
    }
}

impl Updater for Updates {
    fn check(&self) -> Later<Result<Option<Box<dyn Verified>>, String>> {
        let found = {
            let mut state = locked(&self.0);
            state.checked.push(Instant::now());
            state.checks.pop_front().unwrap_or(Ok(None))
        };
        let updates = self.clone();
        Box::pin(async move {
            found.map(|found| {
                found
                    .map(|version| Box::new(FakeVerified { version, updates }) as Box<dyn Verified>)
            })
        })
    }
}

struct FakeProcess(Arc<FakeProcessState>);

impl Process for FakeProcess {
    fn has_exited(&self) -> bool {
        *locked(&self.0.exited)
    }

    fn wait(self: Arc<Self>, limit: Duration) -> Later<bool> {
        Box::pin(async move {
            let exited = async {
                loop {
                    let notified = self.0.notify.notified();
                    if self.has_exited() {
                        return;
                    }
                    notified.await;
                }
            };
            tokio::time::timeout(limit, exited).await.is_ok()
        })
    }
}

impl Processes for Updates {
    fn open(&self, pid: u32) -> Result<Option<Arc<dyn Process>>, String> {
        let state = locked(&self.0)
            .processes
            .iter()
            .find(|(id, _)| *id == pid)
            .map(|(_, state)| Arc::clone(state));
        Ok(state
            .filter(|state| !*locked(&state.exited))
            .map(|state| Arc::new(FakeProcess(state)) as Arc<dyn Process>))
    }
}

impl Record for Updates {
    fn write(&self, version: &str, _at_ms: u64) -> Result<(), String> {
        locked(&self.0)
            .record
            .push(RecordEvent::Written(version.to_string()));
        Ok(())
    }

    fn remove(&self) {
        locked(&self.0).record.push(RecordEvent::Removed);
    }
}

// ---- a running session

pub struct Running {
    pub world: World,
    pub updates: Updates,
    pub screen: Arc<Screen>,
    pub log: Arc<MemoryLog>,
    pub handle: Handle,
    pub started_at: Instant,
    task: JoinHandle<Ended>,
}

/// starts a session on the paused clock; the world is scripted before the first sleep.
/// The app is a development build, which never checks the feed.
pub fn start(world: World) -> Running {
    start_as(world, Updates::default(), "0.0.0-dev", Launch::Running)
}

/// starts a session of an app at `version`, with the update feed `updates` and the
/// launch mode a fresh launch has.
pub fn start_as(world: World, updates: Updates, version: &str, launch: Launch) -> Running {
    start_linking(world, updates, version, version, launch)
}

/// starts a session of an app at `version` whose library, and so its own agent, is at
/// `agent_version`.
pub fn start_linking(
    world: World,
    updates: Updates,
    version: &str,
    agent_version: &str,
    launch: Launch,
) -> Running {
    let screen = Arc::new(Screen::default());
    let log = Arc::new(MemoryLog::default());
    let started_at = Instant::now();
    let (session, handle) = Session::new(Seams {
        dial: world.clone(),
        launcher: Arc::new(world.clone()),
        present: screen.clone(),
        time: Arc::new(FakeTime { start: started_at }),
        log: log.clone(),
        version: version.to_string(),
        agent_version: agent_version.to_string(),
        updater: Some(Arc::new(updates.clone())),
        processes: Arc::new(updates.clone()),
        record: Arc::new(updates.clone()),
        launch,
    });
    Running {
        world,
        updates,
        screen,
        log,
        handle,
        started_at,
        task: tokio::spawn(session.run()),
    }
}

impl Running {
    /// lets the session and its tasks run until they are idle, then moves the clock on by
    /// `by`, running whatever is due.
    pub async fn wait(&self, by: Duration) {
        tokio::time::sleep(by).await;
    }

    /// lets everything that is ready run, without moving the clock.
    pub async fn settle(&self) {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    pub fn is_over(&self) -> bool {
        self.task.is_finished()
    }

    /// how the session ended; it must have.
    pub async fn ended(self) -> Ended {
        tokio::time::timeout(secs(120), self.task)
            .await
            .expect("the session ended in time")
            .expect("the session did not panic")
    }
}

pub fn temp_dir(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "marshal-session-test-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// what the view says about the connection: the whole window's heading, the cause the
/// lost-contact banner gives, or "list" when the list shows with nothing to say about it.
pub fn what(view: &View) -> String {
    if let Some(full) = view.full() {
        if full.heading == "Marshal cannot reach its agent" && full.lines.join(" ").contains("busy")
        {
            return "lost contact (busy)".to_string();
        }
        return full.heading.clone();
    }
    match view.banners().first() {
        Some(banner) if banner.kind == marshal_view::BannerKind::LostContact => {
            let line = banner.lines.join(" ");
            let cause = if line.contains("Reconnecting") {
                "dropped"
            } else if line.contains("busy") {
                "busy"
            } else {
                "restart failed"
            };
            format!("lost contact ({cause})")
        }
        Some(banner) if banner.kind == marshal_view::BannerKind::Quitting => {
            "Quitting Marshal".to_string()
        }
        _ => "list".to_string(),
    }
}
