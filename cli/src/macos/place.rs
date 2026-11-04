//! the lock under which a slot takes the last place under the macOS VM limit.

use std::future::Future;

use tokio::sync::Mutex;

/// one per instance, shared by every macOS slot. A slot gives it the whole "count,
/// then clone" step, so the lock cannot be released between the two: a second slot
/// counts only after the first one's VM is listed, and two slots never take the
/// last place together.
#[derive(Debug, Default)]
pub struct PlaceLock {
    gate: Mutex<()>,
}

impl PlaceLock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// waits for the slot that holds the lock, then runs `step` and returns its
    /// output, releasing the lock when the step ends. The step does nothing before
    /// the lock is held, because a future runs only when polled.
    pub async fn hold<T>(&self, step: impl Future<Output = T>) -> T {
        let _held = self.gate.lock().await;
        step.await
    }
}
