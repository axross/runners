//! getting rid of a VM within a budget: a graceful stop, then a forced stop if that
//! failed, then the deletion; the same steps for a job VM at the end of its job, at
//! the stop sequence, and for a leftover at the next `start`.

use std::time::Duration;

use crate::runtime::backend::BackendError;
use crate::runtime::scrub::scrub_error_text;

use super::client::TartClient;
use super::list::VmSource;

/// how the VM is stopped before it is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// the process that held the VM has ended, or never ran it, so nothing is asked
    /// to stop it; a deletion that fails then is tried once more after a forced stop,
    /// for a holder that was killed and left its `tart run` running.
    NotStarted,
    /// ask it to shut down, and stop it forcibly when that fails.
    Graceful,
    /// stop it forcibly at once, as for a leftover whose state nothing knows.
    Forced,
}

/// the share of a teardown budget each call is given, in tenths.
const GRACEFUL_TENTHS: u32 = 5;
const FORCE_TENTHS: u32 = 2;
const DELETE_TENTHS: u32 = 2;

/// the bounds of the calls of a teardown within one budget: five tenths for the
/// graceful stop, two for the forced stop, two for the deletion and one for the
/// listing that tells a deletion that failed because the VM was already gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Steps {
    /// the bound on the graceful stop call.
    pub graceful: Duration,
    /// the wait Tart gives the VM to shut down itself, a second less than
    /// [`graceful`](Self::graceful), whole seconds, so Tart ends its own wait first.
    pub grace: Duration,
    pub force: Duration,
    pub delete: Duration,
    pub verify: Duration,
}

impl Steps {
    #[must_use]
    pub fn of(budget: Duration) -> Self {
        let tenths = |count: u32| budget * count / 10;
        let graceful = tenths(GRACEFUL_TENTHS);
        let force = tenths(FORCE_TENTHS);
        let delete = tenths(DELETE_TENTHS);
        Self {
            graceful,
            grace: Duration::from_secs(graceful.as_secs().saturating_sub(1)),
            force,
            delete,
            verify: budget
                .saturating_sub(graceful)
                .saturating_sub(force)
                .saturating_sub(delete),
        }
    }
}

/// stops `vm` as `stop` says and deletes it, all within `budget`. A deletion that
/// fails is not a failure when a listing shows the VM gone, because another release
/// of the same VM may have deleted it first.
pub async fn tear_down(
    client: &TartClient,
    vm: &str,
    stop: Stop,
    budget: Duration,
) -> Result<(), BackendError> {
    let steps = Steps::of(budget);
    match stop {
        Stop::NotStarted => {}
        Stop::Graceful => {
            if client.stop(vm, steps.grace, steps.graceful).await.is_err() {
                let _ = client.force_stop(vm, steps.force).await;
            }
        }
        Stop::Forced => {
            let _ = client.force_stop(vm, steps.graceful.max(steps.force)).await;
        }
    }
    let mut deleted = client.delete(vm, steps.delete).await;
    if deleted.is_err() && stop == Stop::NotStarted {
        // a holder that ended may have left its `tart run` child running, as when the
        // shim was killed, and Tart refuses to delete a running VM
        let _ = client.force_stop(vm, steps.force).await;
        deleted = client.delete(vm, steps.delete).await;
    }
    let Err(error) = deleted else {
        return Ok(());
    };
    if is_gone(client, vm, steps.verify).await {
        return Ok(());
    }
    Err(BackendError::new(format!(
        "deleting the VM {vm} failed ({})",
        scrub_error_text(&error.to_string(), "")
    )))
}

/// whether a listing within `budget` shows no local VM called `vm`.
async fn is_gone(client: &TartClient, vm: &str, budget: Duration) -> bool {
    let listed = tokio::time::timeout(budget, client.list()).await;
    matches!(listed, Ok(Ok(entries)) if !entries
        .iter()
        .any(|entry| entry.source == VmSource::Local && entry.name == vm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budget_of_ten_seconds_gives_five_two_two_and_one() {
        let steps = Steps::of(Duration::from_secs(10));
        assert_eq!(steps.graceful, Duration::from_secs(5));
        assert_eq!(steps.grace, Duration::from_secs(4));
        assert_eq!(steps.force, Duration::from_secs(2));
        assert_eq!(steps.delete, Duration::from_secs(2));
        assert_eq!(steps.verify, Duration::from_secs(1));
        assert_eq!(
            steps.graceful + steps.force + steps.delete + steps.verify,
            Duration::from_secs(10)
        );
    }

    #[test]
    fn a_budget_too_small_for_a_wait_gives_the_vm_no_grace() {
        let steps = Steps::of(Duration::from_secs(1));
        assert_eq!(steps.grace, Duration::ZERO);
    }
}
