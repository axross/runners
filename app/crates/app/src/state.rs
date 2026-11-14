//! what the commands and the menu share with the session.

use std::sync::Arc;
use std::sync::{Mutex, PoisonError};

use marshal_session::{Handle, Log, Then};
use marshal_view::{Confirmation, Menu};

/// a question the confirmation window is showing, and what a yes does.
#[derive(Debug, Clone)]
pub struct Asking {
    pub question: Confirmation,
    pub then: Then,
}

/// the menu as the icon last had it, so a menu event finds the item it names.
#[derive(Default)]
pub struct Latest {
    menu: Mutex<Option<Menu>>,
}

impl Latest {
    pub fn set(&self, menu: &Menu) {
        *self.menu.lock().unwrap_or_else(PoisonError::into_inner) = Some(menu.clone());
    }

    pub fn menu(&self) -> Option<Menu> {
        self.menu
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub struct AppState {
    pub handle: Handle,
    pub latest: Arc<Latest>,
    pub log: Arc<dyn Log>,
    asking: Mutex<Option<Asking>>,
}

impl AppState {
    pub fn new(handle: Handle, latest: Arc<Latest>, log: Arc<dyn Log>) -> Self {
        Self {
            handle,
            latest,
            log,
            asking: Mutex::new(None),
        }
    }

    /// records the question the confirmation window is to show; `false` when one is
    /// already showing, which stays.
    pub fn ask(&self, asking: Asking) -> bool {
        let mut slot = self.asking.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return false;
        }
        *slot = Some(asking);
        true
    }

    pub fn question(&self) -> Option<Confirmation> {
        self.asking
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|asking| asking.question.clone())
    }

    /// takes the question, which ends it.
    pub fn answer(&self) -> Option<Asking> {
        self.asking
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}
