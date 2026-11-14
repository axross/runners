//! the login item on macOS: `SMAppService`'s main-app item, the app's own bundle, which
//! macOS opens at login.

use marshal_session::{LoginItemError, LoginItemService, LoginStatus};
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

/// opens Login Items & Extensions in System Settings.
pub fn open_settings() {
    AppService::open_system_settings_login_items();
}
