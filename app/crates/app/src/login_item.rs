//! the login item on macOS: `SMAppService`'s main-app item, the app's own bundle, which
//! macOS opens at login.

use marshal_session::{LoginItemService, LoginStatus};
use smappservice_rs::{AppService, ServiceManagementError, ServiceStatus, ServiceType};

pub struct MainApp;

fn main_app() -> AppService {
    AppService::new(ServiceType::MainApp)
}

fn with_code(error: &ServiceManagementError) -> String {
    format!("{error} (code {})", error.code())
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

    fn register(&self) -> Result<(), String> {
        main_app().register().map_err(|error| with_code(&error))
    }

    fn unregister(&self) -> Result<(), String> {
        main_app().unregister().map_err(|error| with_code(&error))
    }
}
