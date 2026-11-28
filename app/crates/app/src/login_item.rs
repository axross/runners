//! the login item on macOS: `SMAppService`'s main-app item, the app's own bundle, which
//! macOS opens at login.

use marshal_modes::{LAUNCH_MARK_PARAMETER, LaunchEvent};
use marshal_session::{LoginItemError, LoginItemService, LoginStatus};
use objc2_foundation::NSAppleEventManager;
use smappservice_rs::{AppService, ServiceManagementError, ServiceStatus, ServiceType};

pub struct MainApp;

fn main_app() -> AppService {
    AppService::new(ServiceType::MainApp)
}

fn with_code(error: &ServiceManagementError) -> LoginItemError {
    LoginItemError {
        code: error.code(),
        message: error.to_string(),
    }
}

impl LoginItemService for MainApp {
    fn status(&self) -> LoginStatus {
        match main_app().status() {
            ServiceStatus::Enabled => LoginStatus::Enabled,
            ServiceStatus::NotRegistered => LoginStatus::NotRegistered,
            ServiceStatus::RequiresApproval => LoginStatus::RequiresApproval,
            ServiceStatus::NotFound => LoginStatus::NotFound,
        }
    }

    fn register(&self) -> Result<(), LoginItemError> {
        main_app().register().map_err(|error| with_code(&error))
    }

    fn unregister(&self) -> Result<(), LoginItemError> {
        main_app().unregister().map_err(|error| with_code(&error))
    }
}

pub fn open_settings() {
    AppService::open_system_settings_login_items();
}

/// the Apple Event macOS is delivering, which at launch is the open-application event, and
/// none outside an event's delivery. Call it before anything that can run the event loop,
/// as `start` does from Tauri's setup.
pub fn launch_event() -> Option<LaunchEvent> {
    let event = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent()?;
    Some(LaunchEvent {
        class: event.eventClass(),
        id: event.eventID(),
        mark: event
            .paramDescriptorForKeyword(LAUNCH_MARK_PARAMETER)
            .map(|parameter| parameter.enumCodeValue()),
    })
}
