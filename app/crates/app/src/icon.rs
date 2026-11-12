//! the icon's background thread: once a second it reads the taskbar's mode and the
//! primary display's scaling, and the icon's image is replaced only when either, or the
//! icon's state, changed.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use marshal_session::Log;
use marshal_tray::{Watch, taskbar_reading};
use tauri::AppHandle;

use crate::tray;

const EVERY: Duration = Duration::from_secs(1);

pub fn spawn(app: AppHandle, watch: Arc<Mutex<Watch>>, log: Arc<dyn Log>) {
    let started = thread::Builder::new()
        .name("marshal-icon".to_string())
        .spawn({
            let log = Arc::clone(&log);
            move || loop {
                let scale = app
                    .primary_monitor()
                    .ok()
                    .flatten()
                    .map(|monitor| monitor.scale_factor());
                let polled = watch
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .poll(taskbar_reading(), scale);
                if let Some(line) = polled.warn {
                    log.warn(&line);
                }
                if polled.replace.is_some() {
                    let setting = app.clone();
                    let watched = Arc::clone(&watch);
                    let queued = app.run_on_main_thread(move || {
                        let _ = tray::set_current_look(&setting, &watched);
                    });
                    if queued.is_err() {
                        // the app is closing
                        return;
                    }
                }
                thread::sleep(EVERY);
            }
        });
    if let Err(error) = started {
        log.warn(&format!("The icon's thread could not start: {error}"));
    }
}
