use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use marshal_cli::instance::roots::Roots;
use marshal_modes::Overrides;
use marshal_session::{
    AppLog, Library, Log, MAX_FILE_BYTES, OwnExecutable, Seams, Session, WallClock,
};
use marshal_tray::{Look, SIZES, Watch, utc_offset_minutes};
use marshal_view::{GlyphColour, IconState};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use crate::confirmation::CONFIRMATION_WINDOW;
use crate::screen::Screen;
use crate::state::{AppState, Latest};
use crate::{commands, icon, tray};

pub const MAIN_WINDOW: &str = "main";

/// brings the main window forward, showing it when it is hidden.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// whether the main window can be seen: visible and not minimised, so a question put to
/// it is seen. A minimised window counts as not shown, and a question goes to a window
/// of its own.
pub fn main_window_shown(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN_WINDOW).is_some_and(|window| {
        window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
    })
}

/// the log when the app's own file cannot be made: the app still runs.
struct Discard;

impl Log for Discard {
    fn info(&self, _line: &str) {}
    fn warn(&self, _line: &str) {}
}

/// connects the session to the agent and shows it on the icon and in the window; the
/// app ends when the session does.
fn start(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let roots = Roots::resolve(Overrides::from_environment().root.as_deref())?;
    let log: Arc<dyn Log> = match AppLog::open(roots.log_directory(), MAX_FILE_BYTES) {
        Ok(opened) => Arc::new(opened),
        Err(error) => {
            eprintln!("warning: the app's log cannot be written: {error}");
            Arc::new(Discard)
        }
    };
    let launcher = OwnExecutable::current(Arc::clone(&log))?;
    let watch = Arc::new(Mutex::new(Watch::new()));
    let latest = Arc::new(Latest::default());
    let screen = Screen::new(
        app.clone(),
        Arc::clone(&watch),
        Arc::clone(&latest),
        Arc::clone(&log),
    );
    let (session, handle) = Session::new(Seams {
        dial: Library::new(roots, env!("CARGO_PKG_VERSION")),
        launcher: Arc::new(launcher),
        present: Arc::new(screen),
        time: Arc::new(WallClock::new(utc_offset_minutes)),
        log: Arc::clone(&log),
    });
    app.manage(AppState::new(handle, latest, Arc::clone(&log)));
    tray::create(
        app,
        Look {
            state: IconState::NotInControl,
            colour: GlyphColour::White,
            size: SIZES[0],
        },
    )?;
    icon::spawn(app.clone(), watch, Arc::clone(&log));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let ended = session.run().await;
        log.info(&format!("The app ends: {ended:?}."));
        app.exit(0);
    });
    Ok(())
}

/// runs the app until it is quit; closing the main window hides it and leaves the
/// icon.
pub fn run() -> ExitCode {
    let built = tauri::Builder::default()
        // registered first, so a second launch reaches the first before anything else
        .plugin(tauri_plugin_single_instance::init(
            |app, _args, _directory| {
                show_main_window(app);
            },
        ))
        .setup(|app| start(app.handle()))
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    MAIN_WINDOW => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    CONFIRMATION_WINDOW => {
                        // closing the window is a No
                        let _ = window.app_handle().state::<AppState>().answer();
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!());
    match built {
        Ok(app) => {
            app.run(|_handle, event| {
                // only Quit ends the app; an exit request with no code is the last
                // window closing
                if let RunEvent::ExitRequested {
                    api, code: None, ..
                } = event
                {
                    api.prevent_exit();
                }
            });
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: Marshal cannot start: {error}");
            ExitCode::FAILURE
        }
    }
}
