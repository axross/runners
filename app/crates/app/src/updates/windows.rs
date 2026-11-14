//! the agent's process on Windows, through a handle opened with the right to wait on it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use marshal_session::{Later, Process, Processes};
use winsafe::{self as w, co, guard::CloseHandleGuard};

use super::EXIT_POLL;

/// opens processes by ID with the right to wait on them.
pub struct Handles;

impl Processes for Handles {
    fn open(&self, pid: u32) -> Result<Option<Arc<dyn Process>>, String> {
        match w::HPROCESS::OpenProcess(
            co::PROCESS::SYNCHRONIZE | co::PROCESS::QUERY_LIMITED_INFORMATION,
            false,
            pid,
        ) {
            Ok(handle) => Ok(Some(Arc::new(Handle(Mutex::new(handle))))),
            // no such process: it has exited
            Err(co::ERROR::INVALID_PARAMETER) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}

struct Handle(Mutex<CloseHandleGuard<w::HPROCESS>>);

impl Process for Handle {
    fn has_exited(&self) -> bool {
        let handle = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        matches!(handle.WaitForSingleObject(Some(0)), Ok(co::WAIT::OBJECT_0))
    }

    fn wait(self: Arc<Self>, limit: Duration) -> Later<bool> {
        Box::pin(async move {
            let until = tokio::time::Instant::now() + limit;
            loop {
                if self.has_exited() {
                    return true;
                }
                if tokio::time::Instant::now() >= until {
                    return false;
                }
                tokio::time::sleep(EXIT_POLL).await;
            }
        })
    }
}
