//! a window calls only the commands its capability grants, checked against the app's
//! own manifest and capabilities with the mock runtime: no web view runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Arc;

use marshal_app::commands;
use marshal_app::state::{AppState, Asking, Latest};
use marshal_session::{
    Connected, Dial, DialError, Failure, Launcher, Link, Log, Method, Present, Seams, Session,
    Shown, Then, WallClock,
};
use marshal_view::Confirmation;
use serde_json::json;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

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

fn question() -> Confirmation {
    Confirmation {
        title: "Stop all runners?".to_string(),
        body: "They stay stopped until you choose Start All.".to_string(),
        confirm: "Stop".to_string(),
        cancel: "Cancel".to_string(),
    }
}

fn app() -> App<MockRuntime> {
    let (session, handle) = Session::new(Seams {
        dial: NoOne,
        launcher: Arc::new(Refuse),
        present: Arc::new(Quiet),
        time: Arc::new(WallClock::new(|| 0)),
        log: Arc::new(Quiet),
    });
    tauri::async_runtime::spawn(async move {
        session.run().await;
    });
    let state = AppState::new(handle, Arc::new(Latest::default()), Arc::new(Quiet));
    assert!(state.ask(Asking {
        question: question(),
        then: Then::Quit,
    }));
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::read_view,
            commands::start_all,
            commands::stop_all,
            commands::build,
            commands::stop_build,
            commands::try_again,
            commands::set_token,
            commands::remove_token,
            commands::hide_window,
            commands::read_confirmation,
            commands::answer_confirmation,
        ])
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

const MAIN_COMMANDS: [&str; 9] = [
    "read_view",
    "start_all",
    "stop_all",
    "build",
    "stop_build",
    "try_again",
    "set_token",
    "remove_token",
    "hide_window",
];

const CONFIRMATION_COMMANDS: [&str; 2] = ["read_confirmation", "answer_confirmation"];

fn arguments(command: &str) -> serde_json::Value {
    match command {
        "set_token" => json!({ "repository": "axross/runners", "token": "" }),
        "remove_token" => json!({ "repository": "axross/runners" }),
        "read_view" => json!({ "forms": null }),
        "answer_confirmation" => json!({ "accepted": false }),
        _ => json!({}),
    }
}

#[test]
fn the_main_window_calls_its_nine_commands() {
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
