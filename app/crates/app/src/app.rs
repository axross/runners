use std::process::ExitCode;

use tauri::{Manager, RunEvent, WindowEvent};

use crate::{commands, tray};

pub const MAIN_WINDOW: &str = "main";

/// brings the main window forward, showing it when it is hidden.
pub fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
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
        .setup(|app| {
            tray::create(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::read_view,
            commands::hide_window
        ])
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
