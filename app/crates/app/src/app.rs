use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use marshal_cli::agent::{EXECUTABLE_CHANGED, Launch};
use marshal_cli::instance::roots::Roots;
use marshal_modes::{Overrides, is_sign_in};
use marshal_session::{
    AppLog, Ended, Library, Log, MAX_FILE_BYTES, MOVED_FILE, MovedFile, OwnExecutable, Processes,
    RECORD_FILE, RecordFile, Seams, Session, TimeSource, Updater, WallClock, may_control_agent,
};
use marshal_tray::{Look, SIZES, Watch, utc_offset_minutes};
use marshal_view::{GlyphColour, IconState};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, Window, WindowEvent};

use crate::confirmation::CONFIRMATION_WINDOW;
use crate::screen::Screen;
use crate::state::{AppState, Latest};
use crate::{commands, icon, tray};

pub const MAIN_WINDOW: &str = "main";

/// the event that tells the main window's page it is being hidden, so the page clears
/// a token half typed before nobody can see it.
pub const HIDDEN_EVENT: &str = "marshal:hidden";

/// hides the main window, after telling its page, from every place the app hides it. The
/// window is hidden even when the page could not be told.
pub fn hide_main_window<R: Runtime>(window: &Window<R>) -> tauri::Result<()> {
    let told = window.emit_to(MAIN_WINDOW, HIDDEN_EVENT, ());
    window.hide()?;
    in_dock(window.app_handle(), false);
    if let Some(state) = window.app_handle().try_state::<AppState>() {
        state.handle.window_shown(false);
    }
    told
}

/// on macOS, makes Marshal a regular app, with a Dock icon and its menus, while its main
/// window is open, and a menu bar app with neither once it is hidden; `LSUIElement`
/// starts it as the latter. Nothing elsewhere.
#[allow(unused_variables)]
fn in_dock<R: Runtime>(app: &AppHandle<R>, shown: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if shown {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}

/// tells the session whether the main window can be seen, which is when it holds the
/// `github` subscription; the session ignores a value it already has.
pub fn tell_session_shown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<AppState>() {
        state.handle.window_shown(main_window_shown(app));
    }
}

/// what the app does with a window's events: closing the main window hides it and leaves
/// the icon, and closing the confirmation window is a No.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    // a minimised window reports a resize, and one shown or restored gains focus or a size
    if window.label() == MAIN_WINDOW
        && matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_))
    {
        tell_session_shown(window.app_handle());
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        match window.label() {
            MAIN_WINDOW => {
                api.prevent_close();
                let _ = hide_main_window(window);
            }
            CONFIRMATION_WINDOW => {
                // closing the window is a No
                let _ = window.app_handle().state::<AppState>().answer();
            }
            _ => {}
        }
    }
}

/// brings the main window forward, showing it when it is hidden.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        in_dock(app, true);
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    tell_session_shown(app);
}

/// whether the main window can be seen: visible and not minimised, so a question put to
/// it is seen. A minimised window counts as not shown, and a question goes to a window
/// of its own.
pub fn main_window_shown<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(MAIN_WINDOW).is_some_and(|window| {
        window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
    })
}

/// Tauri's opener plugin, which the page cannot reach: its link script is off, and no
/// capability grants its commands. Only the app's own `open_job` opens a page with it.
pub fn opener<R: Runtime>() -> impl tauri::plugin::Plugin<R> {
    tauri_plugin_opener::Builder::new()
        .open_js_links_on_click(false)
        .build()
}

/// the log when the app's own file cannot be made: the app still runs.
struct Discard;

impl Log for Discard {
    fn info(&self, _line: &str) {}
    fn warn(&self, _line: &str) {}
}

/// where the app updates itself, Windows, the feed and the agent's process handles;
/// elsewhere neither.
#[cfg(windows)]
fn update_seams(app: &AppHandle) -> (Option<Arc<dyn Updater>>, Arc<dyn Processes>) {
    (crate::updates::feed(app), Arc::new(crate::updates::Handles))
}

#[cfg(not(windows))]
fn update_seams(_app: &AppHandle) -> (Option<Arc<dyn Updater>>, Arc<dyn Processes>) {
    (None, Arc::new(marshal_session::NoUpdates))
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
    let executable = std::env::current_exe()?;
    let in_place = may_control_agent(&executable);
    if !in_place {
        log.warn(&format!(
            "Marshal runs from {}, outside Applications, so it starts no agent.",
            executable.display()
        ));
    }
    let launcher = OwnExecutable::at(executable, Arc::clone(&log));
    let version = env!("CARGO_PKG_VERSION");
    let time = Arc::new(WallClock::new(utc_offset_minutes));
    // every launch removes the record kept across an update; a launch of the update's
    // version within 30 minutes of it leaves the runners as they were, and any other
    // launch is fresh: the agent starts stopped and is sent `launch`
    let record = Arc::new(RecordFile::new(roots.config_directory().join(RECORD_FILE)));
    let taken = record.take(version, time.now_ms());
    match taken {
        Some(Launch::Stopped) => {
            log.info("This launch follows an update from stopped runners, so they stay stopped.")
        }
        Some(Launch::Running) => {
            log.info("This launch follows an update from running runners, so they start again.")
        }
        None => {}
    }
    let fresh = taken.is_none();
    let launch = taken.unwrap_or(Launch::Stopped);
    // the marker the last copy left if its agent ended because Marshal was moved; only a
    // copy that may control an agent takes it, so one opened elsewhere leaves it for that copy
    let moved = MovedFile::new(roots.config_directory().join(MOVED_FILE));
    let after_moved = in_place && moved.take();
    let (updater, processes) = update_seams(app);
    let watch = Arc::new(Mutex::new(Watch::new()));
    let latest = Arc::new(Latest::default());
    let screen = Screen::new(
        app.clone(),
        Arc::clone(&watch),
        Arc::clone(&latest),
        Arc::clone(&log),
    );
    let (session, handle) = Session::new(Seams {
        dial: Library::new(roots, version),
        launcher: Arc::new(launcher),
        present: Arc::new(screen),
        time,
        log: Arc::clone(&log),
        version: version.to_string(),
        agent_version: marshal_cli::agent::PACKAGE_VERSION.to_string(),
        updater,
        processes,
        record,
        launch,
        fresh,
    });
    let session = if !in_place {
        session.outside_applications()
    } else if after_moved {
        log.info("The last agent ended because Marshal was moved, renamed or replaced.");
        session.after_moved()
    } else {
        session
    };
    app.manage(AppState::new(handle, latest, Arc::clone(&log)));
    // the window starts hidden, and a start from the sign-in item leaves it so
    if is_sign_in(std::env::args_os()) {
        log.info("Marshal started at sign-in, with its icon only.");
        tell_session_shown(app);
        in_dock(app, false);
    } else {
        show_main_window(app);
    }
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
        if ended == Ended::AgentEnded(EXECUTABLE_CHANGED.to_string())
            && let Err(why) = moved.write()
        {
            log.warn(&format!(
                "The next launch cannot be told why the agent ended: {why}"
            ));
        }
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
        .plugin(opener())
        .setup(|app| start(app.handle()));
    // only the app mode runs this, so only it registers the updater
    #[cfg(windows)]
    let built = built.plugin(crate::updates::plugin());
    let built = built
        .on_window_event(on_window_event)
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
