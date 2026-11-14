//! a window calls only the commands its capability grants, checked against the app's
//! own manifest and capabilities with the mock runtime: no web view runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use marshal_app::state::{AppState, Asking, Latest};
use marshal_app::{HIDDEN_EVENT, commands, on_window_event, opener};
use marshal_session::{
    Act, AgentInfo, Connected, Dial, DialError, Failure, Launcher, Link, Log, Method, NoUpdates,
    Present, Pushed, Seams, Session, Shown, Then, WallClock,
};
use marshal_view::Confirmation;
use serde_json::json;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, RunEvent, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tokio::sync::mpsc;

/// no agent ever answers, so the session sits in "no agent" and answers every read.
struct NoOne;

struct Nothing;

impl Link for Nothing {
    async fn subscribe(&self) -> Result<(), Failure> {
        Ok(())
    }
    async fn call(&self, _method: Method) -> Result<(), Failure> {
        Ok(())
    }
    async fn drain_for_update(&self, _version: &str) -> Result<Option<u32>, Failure> {
        Ok(None)
    }
    async fn subscribe_github(&self) -> Result<(), Failure> {
        Ok(())
    }
    async fn renew_github(&self) -> Result<(), Failure> {
        Ok(())
    }
}

impl Dial for NoOne {
    type Link = Nothing;

    async fn dial(&self) -> Result<Connected<Nothing>, DialError> {
        Err(DialError::NoAgent)
    }
}

struct Refuse;

impl Launcher for Refuse {
    fn start_agent(&self, _launch: marshal_cli::agent::Launch) -> Result<(), String> {
        Err("this test starts nothing".to_string())
    }
}

struct Quiet;

impl Present for Quiet {
    fn present(&self, _shown: &Shown) {}
}

impl Log for Quiet {
    fn info(&self, _line: &str) {}
    fn warn(&self, _line: &str) {}
}

/// keeps the lines the app logs.
#[derive(Default)]
struct Recorded(Mutex<Vec<String>>);

impl Recorded {
    fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Log for Recorded {
    fn info(&self, _line: &str) {}
    fn warn(&self, line: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_string());
    }
}

fn question() -> Confirmation {
    Confirmation {
        title: "Stop all runners?".to_string(),
        body: "They stay stopped until you choose Start All.".to_string(),
        confirm: "Stop".to_string(),
        cancel: "Cancel".to_string(),
    }
}

fn app() -> App<MockRuntime> {
    app_asking(Then::Quit, Arc::new(Quiet))
}

/// an app with `then` waiting on its question, writing its log to `log`.
fn app_asking(then: Then, log: Arc<dyn Log>) -> App<MockRuntime> {
    let (session, handle) = Session::new(Seams {
        dial: NoOne,
        launcher: Arc::new(Refuse),
        present: Arc::new(Quiet),
        time: Arc::new(WallClock::new(|| 0)),
        log: Arc::new(Quiet),
        version: "test".to_string(),
        agent_version: "test".to_string(),
        updater: None,
        processes: Arc::new(NoUpdates),
        record: Arc::new(NoUpdates),
        launch: marshal_cli::agent::Launch::Running,
        fresh: false,
    });
    tauri::async_runtime::spawn(async move {
        session.run().await;
    });
    let state = AppState::new(handle, Arc::new(Latest::default()), log);
    assert!(state.ask(Asking {
        question: question(),
        then,
    }));
    mock_builder()
        .manage(state)
        .plugin(opener())
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())
        .expect("the app builds")
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .expect("the window opens")
}

/// what a command says when called from `window`: its JSON result, or the error text.
fn call(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: APP_ORIGIN.parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<serde_json::Value>().unwrap())
    .map_err(|error| error.to_string())
}

/// whether the capability let the call through; a call it let through may still fail on
/// its own account.
fn refused(result: &Result<serde_json::Value, String>) -> bool {
    result
        .as_ref()
        .is_err_and(|words| words.contains("not allowed"))
}

/// where the app's own pages are served from, which is what makes a window local.
const APP_ORIGIN: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

const MAIN_COMMANDS: [&str; 10] = [
    "read_view",
    "start_all",
    "stop_all",
    "build",
    "stop_build",
    "try_again",
    "set_token",
    "remove_token",
    "hide_window",
    "open_job",
];

const CONFIRMATION_COMMANDS: [&str; 2] = ["read_confirmation", "answer_confirmation"];

fn arguments(command: &str) -> serde_json::Value {
    match command {
        "set_token" => json!({ "repository": "axross/runners", "token": "" }),
        "remove_token" => json!({ "repository": "axross/runners" }),
        "read_view" => json!({ "forms": null }),
        "answer_confirmation" => json!({ "accepted": false }),
        "open_job" => json!({ "pool": "app", "runner": "app-1-20261104031522114" }),
        _ => json!({}),
    }
}

#[test]
fn the_main_window_calls_its_ten_commands() {
    let app = app();
    let main = window(&app, "main");
    for command in MAIN_COMMANDS {
        let result = call(&main, command, arguments(command));
        assert!(!refused(&result), "{command}: {result:?}");
    }
}

#[test]
fn the_main_window_is_refused_the_confirmation_windows_commands() {
    let app = app();
    let main = window(&app, "main");
    for command in CONFIRMATION_COMMANDS {
        let result = call(&main, command, arguments(command));
        assert!(refused(&result), "{command}: {result:?}");
    }
}

#[test]
fn the_confirmation_window_reads_its_question_and_answers_it() {
    let app = app();
    let confirmation = window(&app, "confirmation");
    let read = call(&confirmation, "read_confirmation", json!({})).unwrap();
    assert_eq!(read["title"], "Stop all runners?");
    let answered = call(
        &confirmation,
        "answer_confirmation",
        arguments("answer_confirmation"),
    );
    assert!(!refused(&answered), "{answered:?}");
}

#[test]
fn the_confirmation_window_is_refused_every_command_of_the_main_window() {
    let app = app();
    let confirmation = window(&app, "confirmation");
    for command in MAIN_COMMANDS {
        let result = call(&confirmation, command, arguments(command));
        assert!(refused(&result), "{command}: {result:?}");
    }
}

#[test]
fn a_window_with_another_label_is_refused_every_command() {
    let app = app();
    let other = window(&app, "elsewhere");
    for command in MAIN_COMMANDS.iter().chain(&CONFIRMATION_COMMANDS) {
        let result = call(&other, command, arguments(command));
        assert!(refused(&result), "{command}: {result:?}");
    }
}

#[test]
fn a_question_is_read_until_it_is_answered() {
    let app = app();
    let confirmation = window(&app, "confirmation");
    call(
        &confirmation,
        "answer_confirmation",
        arguments("answer_confirmation"),
    )
    .unwrap();
    let after = call(&confirmation, "read_confirmation", json!({}));
    assert!(after.is_err() && !refused(&after), "{after:?}");
}

#[test]
fn a_token_that_is_empty_is_refused_by_the_session_and_not_by_the_capability() {
    let app = app();
    let main = window(&app, "main");
    let result = call(&main, "set_token", arguments("set_token"));
    assert!(!refused(&result), "{result:?}");
}

#[test]
fn a_confirmed_action_that_fails_is_logged_since_its_window_is_gone() {
    let log = Arc::new(Recorded::default());
    let app = app_asking(Then::Act(Act::StopAll), log.clone());
    let confirmation = window(&app, "confirmation");
    // no agent is connected, so the action cannot run
    let result = call(
        &confirmation,
        "answer_confirmation",
        json!({ "accepted": true }),
    );
    assert!(result.is_err() && !refused(&result), "{result:?}");
    let lines = log.lines();
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("The confirmed action did not run: ")),
        "{lines:?}"
    );
}

/// counts the times the page is told the main window hides.
fn count_hidden(app: &App<MockRuntime>) -> Arc<AtomicUsize> {
    let told = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&told);
    app.listen_any(HIDDEN_EVENT, move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    told
}

#[test]
fn the_hide_window_command_tells_the_page_and_hides_the_window() {
    let app = app();
    let main = window(&app, "main");
    let told = count_hidden(&app);
    call(&main, "hide_window", json!({})).unwrap();
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

#[test]
fn closing_the_main_window_tells_the_page_and_hides_it_instead() {
    let (session, handle) = Session::new(Seams {
        dial: NoOne,
        launcher: Arc::new(Refuse),
        present: Arc::new(Quiet),
        time: Arc::new(WallClock::new(|| 0)),
        log: Arc::new(Quiet),
        version: "test".to_string(),
        agent_version: "test".to_string(),
        updater: None,
        processes: Arc::new(NoUpdates),
        record: Arc::new(NoUpdates),
        launch: marshal_cli::agent::Launch::Running,
        fresh: false,
    });
    tauri::async_runtime::spawn(async move {
        session.run().await;
    });
    let app = mock_builder()
        .manage(AppState::new(
            handle,
            Arc::new(Latest::default()),
            Arc::new(Quiet),
        ))
        .on_window_event(on_window_event)
        .build(tauri::generate_context!())
        .expect("the app builds");
    let told = count_hidden(&app);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    // the mock runtime does not call the builder's window handler, so the test hands
    // the first close request it reports to the app's own. The window is kept, so the
    // loop goes quiet; a second request, which the test does not hand over, closes it
    // and ends the loop.
    app.run(move |handle, event| {
        let mut seen = log.lock().unwrap();
        let main = || handle.get_webview_window("main").unwrap();
        match event {
            RunEvent::Ready => {
                seen.push("ready");
                main().close().unwrap();
            }
            RunEvent::WindowEvent { event, .. } => {
                seen.push("close requested");
                if seen.len() == 2 {
                    on_window_event(&main().as_ref().window(), &event);
                }
            }
            RunEvent::MainEventsCleared => {
                seen.push("quiet");
                main().close().unwrap();
            }
            RunEvent::ExitRequested { .. } => seen.push("exit requested"),
            _ => {}
        }
    });
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "ready",
            "close requested",
            "quiet",
            "close requested",
            "exit requested"
        ],
        "the first close request left the window open"
    );
    assert_eq!(told.load(Ordering::SeqCst), 1);
}

#[test]
fn a_pool_the_agent_does_not_list_is_refused_by_the_session() {
    let app = app();
    let main = window(&app, "main");
    let result = call(&main, "open_job", arguments("open_job"));
    // the mock runtime hands back a command's error as JSON
    assert_eq!(result, Err(json!("Marshal has no such pool.").to_string()));
}

#[test]
fn the_main_window_is_refused_the_opener_plugins_commands() {
    let app = app();
    let main = window(&app, "main");
    for command in [
        "plugin:opener|open_url",
        "plugin:opener|open_path",
        "plugin:opener|reveal_item_in_dir",
    ] {
        let result = call(
            &main,
            command,
            json!({ "url": "https://example.com/", "path": "C:\\", "paths": [] }),
        );
        assert!(refused(&result), "{command}: {result:?}");
    }
}

/// what a fake agent has seen of the `github` subscription.
#[derive(Default)]
struct Probe {
    /// connections made.
    dials: AtomicUsize,
    /// connections subscribed to `github` and still open.
    github_open: AtomicUsize,
    /// the push channels, kept so no connection reads as closed.
    senders: Mutex<Vec<mpsc::UnboundedSender<Pushed>>>,
}

struct Watched {
    probe: Arc<Probe>,
    github: AtomicBool,
}

impl Link for Watched {
    async fn subscribe(&self) -> Result<(), Failure> {
        Ok(())
    }
    async fn call(&self, _method: Method) -> Result<(), Failure> {
        Ok(())
    }
    async fn subscribe_github(&self) -> Result<(), Failure> {
        if !self.github.swap(true, Ordering::SeqCst) {
            self.probe.github_open.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
    async fn renew_github(&self) -> Result<(), Failure> {
        Ok(())
    }
    async fn drain_for_update(&self, _version: &str) -> Result<Option<u32>, Failure> {
        Ok(None)
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        if self.github.load(Ordering::SeqCst) {
            self.probe.github_open.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

struct Answers(Arc<Probe>);

impl Dial for Answers {
    type Link = Watched;

    async fn dial(&self) -> Result<Connected<Watched>, DialError> {
        self.0.dials.fetch_add(1, Ordering::SeqCst);
        let (sender, pushes) = mpsc::unbounded_channel();
        self.0
            .senders
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(sender);
        Ok(Connected {
            link: Watched {
                probe: Arc::clone(&self.0),
                github: AtomicBool::new(false),
            },
            pushes,
            agent: AgentInfo::default(),
        })
    }
}

/// waits up to 5 s for `held` to say whether a `github` subscription is open.
fn eventually(probe: &Probe, held: bool) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if (probe.github_open.load(Ordering::SeqCst) == 1) == held {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn showing_the_main_window_opens_the_github_subscription_and_hiding_it_ends_it() {
    let probe = Arc::new(Probe::default());
    let (session, handle) = Session::new(Seams {
        dial: Answers(Arc::clone(&probe)),
        launcher: Arc::new(Refuse),
        present: Arc::new(Quiet),
        time: Arc::new(WallClock::new(|| 0)),
        log: Arc::new(Quiet),
        version: "test".to_string(),
        agent_version: "test".to_string(),
        updater: None,
        processes: Arc::new(NoUpdates),
        record: Arc::new(NoUpdates),
        launch: marshal_cli::agent::Launch::Running,
        fresh: false,
    });
    tauri::async_runtime::spawn(async move {
        session.run().await;
    });
    let app = mock_builder()
        .manage(AppState::new(
            handle,
            Arc::new(Latest::default()),
            Arc::new(Quiet),
        ))
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())
        .expect("the app builds");
    let main = window(&app, "main");
    // the session reaches the agent first, so a subscription it took regardless of the
    // window would show by now
    let until = Instant::now() + Duration::from_secs(5);
    while probe.dials.load(Ordering::SeqCst) == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        probe.github_open.load(Ordering::SeqCst),
        0,
        "nothing is held before the window is shown"
    );
    // the mock runtime reports the window visible and not minimised
    on_window_event(&main.as_ref().window(), &WindowEvent::Focused(true));
    assert!(eventually(&probe, true), "showing the window subscribes");
    call(&main, "hide_window", json!({})).unwrap();
    assert!(eventually(&probe, false), "hiding the window ends it");
}
