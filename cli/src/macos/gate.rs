//! the gate that keeps a job VM from being cloned while no primary network is
//! recorded. The signal monitor closes it when the instance begins to run and opens it
//! once a primary network is recorded; the Tart backend waits at it between the host
//! prerequisite check and the clone of a slot's job VM.

use std::rc::Rc;

use tokio::sync::watch;

/// shared by the monitor and the backend; a gate nobody closes is open.
#[derive(Debug, Clone)]
pub struct NetworkGate {
    state: Rc<watch::Sender<bool>>,
}

impl Default for NetworkGate {
    fn default() -> Self {
        Self {
            state: Rc::new(watch::Sender::new(true)),
        }
    }
}

impl NetworkGate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&self) {
        self.state.send_replace(true);
    }

    pub fn close(&self) {
        self.state.send_replace(false);
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        *self.state.borrow()
    }

    /// completes once the gate is open, at once when it is.
    pub async fn wait_until_open(&self) {
        let mut receiver = self.state.subscribe();
        let _ = receiver.wait_for(|open| *open).await;
    }
}
