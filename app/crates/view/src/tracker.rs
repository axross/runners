//! the view with what changes over time: the settled count of runners up, and what the
//! live region has said.

use crate::announce::{Announced, Announcer};
use crate::input::Input;
use crate::settled::Settled;
use crate::state::Context;
use crate::view::{View, build};

/// the view for one observation, and what to announce for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub view: View,
    pub announced: Announced,
}

#[derive(Debug, Default)]
pub struct Tracker {
    settled: Settled,
    announcer: Announcer,
}

impl Tracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// the view for `input`, with the count settled over the time the observations
    /// cover, and what the live region should say. Call it on every snapshot, every
    /// change of the app's state, and when [`Announced::recheck_at_ms`] comes.
    pub fn update(&mut self, input: Input<'_>) -> Update {
        let now_ms = input.clock.now_ms;
        let context = Context::new(&input);
        let settled = self
            .settled
            .observe(context.agent_runs(), context.up, now_ms);
        let view = build(&input.settled(settled));
        let announced = self.announcer.update(&view, now_ms);
        Update { view, announced }
    }
}
