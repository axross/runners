//! in-memory stand-ins for the instance's seams, for tests of the code above them;
//! they record what they were asked, answer from a script, and no command wires
//! them in.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use tokio::sync::{Notify, watch};

use crate::runtime::fake::FakeClock;
use crate::runtime::pool::NoPoolStartable;
use crate::runtime::worker::wait_for_stop;
use crate::validate::Plan;

use super::lifecycle::{Core, CoreFailure, Ending, Source};

/// what one run of a [`FakeCore`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreStep {
    /// lets `after` pass on the clock, then fails with `message`, reporting the
    /// containers it had started.
    Fail {
        message: String,
        containers: Vec<String>,
        after: Duration,
    },
    /// returns `Ok` at once, though no stop was asked for.
    EndWithoutStop,
    Panic(String),
    /// fails at once because no pool could start.
    NothingToRun,
    /// fails at once because the image cannot run jobs, as `message` says.
    RefuseImage(String),
    /// never ends, even when the stop is asked for.
    IgnoreStop,
    RunUntilStop,
}

/// a runtime core that follows a script; when the script runs out it runs until the
/// stop.
#[derive(Default)]
pub struct FakeCore {
    pub steps: RefCell<VecDeque<CoreStep>>,
    pub runs: Cell<usize>,
    /// the clock a failing step lets time pass on.
    pub clock: RefCell<Option<Rc<FakeClock>>>,
    /// when set, a core that was asked to stop waits for this before it ends, which
    /// holds the instance in its stopping state.
    pub end_gate: RefCell<Option<Rc<Notify>>>,
    /// what the core reports to `status`.
    pub report: RefCell<Vec<(&'static str, String)>>,
    /// the answers to `check_image`; when they run out, every check passes.
    pub image_checks: RefCell<VecDeque<Result<(), String>>>,
    pub image_checked: Cell<usize>,
}

impl FakeCore {
    pub fn push(&self, step: CoreStep) {
        self.steps.borrow_mut().push_back(step);
    }

    /// a step that fails at once with `message`.
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
                    ending: Ending::Ordinary,
                })
            }
            Some(CoreStep::EndWithoutStop) => Ok(()),
            #[allow(clippy::panic)]
            Some(CoreStep::Panic(message)) => panic!("{message}"),
            Some(CoreStep::NothingToRun) => Err(CoreFailure {
                message: NoPoolStartable.to_string(),
                containers: Vec::new(),
                ending: Ending::NothingToRun,
            }),
            Some(CoreStep::RefuseImage(message)) => Err(CoreFailure {
                message,
                containers: Vec::new(),
                ending: Ending::ImageRefused,
            }),
            Some(CoreStep::IgnoreStop) => std::future::pending().await,
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

    async fn check_image(&self, _plan: &Plan) -> Result<(), String> {
        self.image_checked.set(self.image_checked.get() + 1);
        self.image_checks.borrow_mut().pop_front().unwrap_or(Ok(()))
    }

    fn report(&self) -> Vec<(&'static str, String)> {
        self.report.borrow().clone()
    }
}

/// a configuration source that answers from a script and counts its reads; when the
/// script runs out it answers with the plan it was built with.
pub struct FakeSource {
    plan: Plan,
    pub results: RefCell<VecDeque<Result<Plan, Vec<String>>>>,
    pub loads: Cell<usize>,
    /// when set, a read waits for this before it answers, which holds the instance in
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
