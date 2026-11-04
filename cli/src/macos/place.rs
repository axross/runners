//! the lock under which a slot takes the last place under the macOS VM limit.

use tokio::sync::{Mutex, MutexGuard};

/// one per instance, shared by every macOS slot: a slot holds it from the moment it
/// counts the macOS VMs until its clone is listed, so a second slot counts only
/// after the first one's VM exists, and two slots never take the last place
/// together.
#[derive(Debug, Default)]
pub struct PlaceLock {
    gate: Mutex<()>,
}

/// holds the lock until it is dropped.
pub type PlaceGuard<'a> = MutexGuard<'a, ()>;

impl PlaceLock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// waits for the slot that holds the lock, then holds it.
    pub async fn take(&self) -> PlaceGuard<'_> {
        self.gate.lock().await
    }
}
