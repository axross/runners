//! the commands the web view calls. Each is granted to a window by a capability, so a
//! window calls only the ones its capability names.

use marshal_session::{Act, Forms, Shown, Then};
use marshal_view::Confirmation;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Runtime, State, Window};

use crate::app::hide_main_window;
use crate::confirmation;
use crate::state::AppState;

/// the view now. `forms` says which token forms the page has open, which only the page
/// knows; the view then shows their state.
#[tauri::command]
pub async fn read_view(state: State<'_, AppState>, forms: Option<Forms>) -> Result<Shown, String> {
    state.handle.read_view(forms).await
}

#[tauri::command]
pub async fn start_all(state: State<'_, AppState>) -> Result<(), String> {
    state.handle.act(Act::StartAll).await
}

#[tauri::command]
pub async fn stop_all(state: State<'_, AppState>) -> Result<(), String> {
    state.handle.act(Act::StopAll).await
}

#[tauri::command]
pub async fn build(state: State<'_, AppState>) -> Result<(), String> {
    state.handle.act(Act::Build).await
}

#[tauri::command]
pub async fn stop_build(state: State<'_, AppState>) -> Result<(), String> {
    state.handle.act(Act::StopBuild).await
}

#[tauri::command]
pub async fn try_again(state: State<'_, AppState>) -> Result<(), String> {
    state.handle.try_again().await
}

/// stores the token entered in the window. The token goes to the session as it arrived
/// and nowhere else: this function keeps no copy and writes no line.
#[tauri::command]
pub async fn set_token(
    state: State<'_, AppState>,
    repository: String,
    token: String,
) -> Result<(), String> {
    state.handle.set_token(repository, token).await
}

#[tauri::command]
pub async fn remove_token(state: State<'_, AppState>, repository: String) -> Result<(), String> {
    state.handle.remove_token(repository).await
}

#[tauri::command]
pub fn hide_window<R: Runtime>(window: Window<R>) -> Result<(), String> {
    hide_main_window(&window).map_err(|error| error.to_string())
}

/// the question the confirmation window shows.
#[tauri::command]
pub fn read_confirmation(state: State<'_, AppState>) -> Result<Confirmation, String> {
    state
        .question()
        .ok_or_else(|| "There is nothing to confirm.".to_string())
}

/// the answer to the question: a yes does what was asked, and either answer closes the
/// window.
#[tauri::command]
pub async fn answer_confirmation<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    accepted: bool,
) -> Result<(), String> {
    let asked = state.answer();
    confirmation::close(&app);
    let outcome = match asked {
        Some(asking) if accepted => match asking.then {
            Then::Act(act) => state.handle.act(act).await,
            Then::Quit => {
                state.handle.quit();
                Ok(())
            }
        },
        _ => Ok(()),
    };
    // the window that asked is gone, so nobody is left to show the failure
    if let Err(words) = &outcome {
        state
            .log
            .warn(&format!("The confirmed action did not run: {words}"));
    }
    outcome
}

/// the handler that routes the web view's calls to the commands above, which the app
/// and the tests that run the real capabilities register alike, so a command added here
/// is in both.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        read_view,
        start_all,
        stop_all,
        build,
        stop_build,
        try_again,
        set_token,
        remove_token,
        hide_window,
        read_confirmation,
        answer_confirmation,
    ]
}
