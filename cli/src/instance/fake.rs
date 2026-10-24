//! In-memory stand-ins for the instance's seams, for tests of the code above them.
//!
//! They record what they were asked and answer from a script. They are test-only and
//! never wired into a command.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use tokio::sync::{Notify, watch};

use crate::runtime::fake::FakeClock;
use crate::runtime::worker::wait_for_stop;
use crate::validate::Plan;

use super::lifecycle::{Core, CoreFailure, Source};

/// What one run of a [`FakeCore`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreStep {
    /// Lets `after` pass on the clock, then fails with `message`, reporting the
    /// containers it had started.
    Fail {
        message: String,
        containers: Vec<String>,
        after: Duration,
    },
    /// Returns `Ok` at once, though no stop was asked for.
    EndWithoutStop,
    /// Runs until the stop is asked for.
    RunUntilStop,
}

/// A runtime core that follows a script. When the script runs out it runs until the
/// stop.
#[derive(Default)]
pub struct FakeCore {
    pub steps: RefCell<VecDeque<CoreStep>>,
    /// How many times the core was started.
    pub runs: Cell<usize>,
    /// The clock a failing step lets time pass on.
    pub clock: RefCell<Option<Rc<FakeClock>>>,
    /// When set, a core that was asked to stop waits for this before it ends, which
    /// holds the instance in its stopping state.
    pub end_gate: RefCell<Option<Rc<Notify>>>,
}

impl FakeCore {
    pub fn push(&self, step: CoreStep) {
        self.steps.borrow_mut().push_back(step);
    }

    /// A step that fails at once with `message`.
    #[must_use]
    pub fn failure(message: &str) -> CoreStep {
        CoreStep::Fail {
            message: message.to_string(),
            containers: Vec::new(),
            after: Duration::ZERO,
        }
    }
}

impl Core for FakeCore {
    async fn run(&self, _plan: &Plan, mut stop: watch::Receiver<bool>) -> Result<(), CoreFailure> {
        self.runs.set(self.runs.get() + 1);
        let step = self.steps.borrow_mut().pop_front();
        match step {
            Some(CoreStep::Fail {
                message,
                containers,
                after,
            }) => {
                if let Some(clock) = self.clock.borrow().as_ref() {
                    clock.advance(after);
                }
                Err(CoreFailure {
                    message,
                    containers,
                })
            }
            Some(CoreStep::EndWithoutStop) => Ok(()),
            Some(CoreStep::RunUntilStop) | None => {
                wait_for_stop(&mut stop).await;
                let gate = self.end_gate.borrow().clone();
                if let Some(gate) = gate {
                    gate.notified().await;
                }
                Ok(())
            }
        }
    }
}

/// A configuration source that answers from a script and counts its reads. When the
/// script runs out it answers with the plan it was built with.
pub struct FakeSource {
    plan: Plan,
    pub results: RefCell<VecDeque<Result<Plan, Vec<String>>>>,
    /// How many times the configuration was read.
    pub loads: Cell<usize>,
    /// When set, a read waits for this before it answers, which holds the instance in
    /// its resuming state.
    pub gate: RefCell<Option<Rc<Notify>>>,
}

impl FakeSource {
    #[must_use]
    pub fn new(plan: Plan) -> Self {
        Self {
            plan,
            results: RefCell::new(VecDeque::new()),
            loads: Cell::new(0),
            gate: RefCell::new(None),
        }
    }
}

impl Source for FakeSource {
    async fn load(&self) -> Result<Plan, Vec<String>> {
        self.loads.set(self.loads.get() + 1);
        let gate = self.gate.borrow().clone();
        if let Some(gate) = gate {
            gate.notified().await;
        }
        self.results
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(self.plan.clone()))
    }
}
