//! wall time and sleeping, behind a seam so tests do not wait.

use std::future::Future;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

/// the time source of the worker loop and the Docker wait.
pub trait Clock {
    /// the wall time that names a container.
    fn now(&self) -> SystemTime;

    /// completes after `duration`.
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()>;
}

/// the real clock, on the runtime's timer.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        tokio::time::sleep(duration)
    }
}

impl<C: Clock + ?Sized> Clock for Rc<C> {
    fn now(&self) -> SystemTime {
        (**self).now()
    }

    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        (**self).sleep(duration)
    }
}
