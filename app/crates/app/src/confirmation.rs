//! the question before an action that costs something, when the main window is not shown
//! to ask it: a small window of its own with Cancel focused.

use marshal_session::{Confirmable, Then};
use marshal_view::Confirmation;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::app::MAIN_WINDOW;
use crate::state::{AppState, Asking};

pub const CONFIRMATION_WINDOW: &str = "confirmation";

/// the event that asks the main window's page to show a question.
pub const ASK_EVENT: &str = "marshal:confirm";

const WIDTH: f64 = 440.0;
const HEIGHT: f64 = 240.0;

#[derive(Serialize, Clone)]
struct Ask {
    action: &'static str,
    question: Confirmation,
}

fn action_id(act: Confirmable) -> &'static str {
    match act {
        Confirmable::StopAll => "stop_all",
        Confirmable::StopBuild => "stop_build",
    }
}

/// asks the main window's page, which shows the question itself and calls the command
/// when it is answered.
pub fn ask_in_main_window<R: Runtime>(
    app: &AppHandle<R>,
    question: &Confirmation,
    then: Confirmable,
) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.set_focus();
    }
    app.emit_to(
        MAIN_WINDOW,
        ASK_EVENT,
        Ask {
            action: action_id(then),
            question: question.clone(),
        },
    )
}

/// opens the confirmation window with `question`. One question shows at a time: while
/// one is open, it is brought forward and the new one is dropped.
pub fn ask_in_own_window<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    question: Confirmation,
    then: Then,
) -> tauri::Result<()> {
    if !state.ask(Asking { question, then }) {
        if let Some(window) = app.get_webview_window(CONFIRMATION_WINDOW) {
            let _ = window.set_focus();
        }
        return Ok(());
    }
    let built = WebviewWindowBuilder::new(
        app,
        CONFIRMATION_WINDOW,
        WebviewUrl::App("confirmation.html".into()),
    )
    .title("Marshal")
    .inner_size(WIDTH, HEIGHT)
    .resizable(false)
    .minimizable(false)
    .maximizable(false)
    .always_on_top(true)
    .center()
    .focused(true)
    .build();
    if built.is_err() {
        // no window, so no question is waiting for an answer
        state.answer();
    }
    built.map(|_| ())
}

/// closes the confirmation window.
pub fn close<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(CONFIRMATION_WINDOW) {
        let _ = window.destroy();
    }
}
