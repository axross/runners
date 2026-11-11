//! what the session shows: the view reaches the main window as an event, and the icon's
//! image, tooltip and menu follow it.

use std::sync::{Arc, Mutex, PoisonError};

use marshal_session::{Log, Present, Shown};
use marshal_tray::{Surface, Watch};
use tauri::{AppHandle, Emitter};

use crate::app::MAIN_WINDOW;
use crate::state::Latest;
use crate::tray;

/// the event that carries each new view to the main window's page.
pub const VIEW_EVENT: &str = "marshal:view";

pub struct Screen {
    app: AppHandle,
    watch: Arc<Mutex<Watch>>,
    surface: Mutex<Surface>,
    latest: Arc<Latest>,
    log: Arc<dyn Log>,
}

impl Screen {
    pub fn new(
        app: AppHandle,
        watch: Arc<Mutex<Watch>>,
        latest: Arc<Latest>,
        log: Arc<dyn Log>,
    ) -> Self {
        Self {
            app,
            watch,
            surface: Mutex::new(Surface::new()),
            latest,
            log,
        }
    }
}

impl Present for Screen {
    fn present(&self, shown: &Shown) {
        if let Err(error) = self.app.emit_to(MAIN_WINDOW, VIEW_EVENT, shown) {
            self.log
                .warn(&format!("The view could not reach the window: {error}"));
        }
        self.latest.set(&shown.view.menu);
        let look = self
            .watch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_state(shown.view.icon);
        let changes = self
            .surface
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .show(&shown.view.menu, &shown.view.tooltip);
        let app = self.app.clone();
        let log = Arc::clone(&self.log);
        // the menu is built on the event loop's thread
        let queued = self.app.run_on_main_thread(move || {
            let results = [
                look.map(|look| tray::set_look(&app, look)),
                changes.tooltip.map(|words| tray::set_tooltip(&app, &words)),
                changes.menu.map(|menu| tray::set_menu(&app, &menu)),
            ];
            for error in results.into_iter().flatten().filter_map(Result::err) {
                log.warn(&format!("The icon could not be updated: {error}"));
            }
        });
        if let Err(error) = queued {
            self.log
                .warn(&format!("The icon could not be updated: {error}"));
        }
    }
}
