//! the boot of a VM the CLI holds: the wait for its guest agent and the text of a
//! boot that fails, shared by a job VM and the build VM of the golden VM.

use std::cell::RefCell;
use std::time::Duration;

use crate::macos::prerequisites::{SIGNED_IN_SECTION, section_of_document};
use crate::runtime::clock::Clock;

use super::client::{Captured, TartClient};
use super::guest::Guest;
use super::hold::HeldVm;

/// what a boot failure calls a job VM.
pub const JOB_VM: &str = "job VM";

/// what a boot failure calls the build VM.
pub const BUILD_VM: &str = "build VM";

/// the time bounds of the wait for a guest agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentWait {
    /// from the start of the wait to the agent's first answer.
    pub deadline: Duration,
    /// the wait between the agent's answers.
    pub poll: Duration,
    /// the bound on one question to the agent.
    pub call: Duration,
}

/// why the wait for a VM's guest agent ended without an answer.
#[derive(Debug)]
pub enum BootEnd {
    /// `tart run` ended first.
    Ended(Captured),
    /// the deadline came, and the last question's error is the text.
    NoAnswer(String),
}

/// waits until the guest agent of `vm` answers, `tart run` ends, or the deadline,
/// counted from now, passes.
pub async fn wait_for_agent<C: Clock>(
    client: &TartClient,
    clock: &C,
    wait: AgentWait,
    vm: &str,
    held: &HeldVm,
) -> Result<(), BootEnd> {
    let guest = Guest::new(client, vm);
    let last = RefCell::new(String::new());
    let asking = async {
        loop {
            match guest.ask(wait.call).await {
                Ok(()) => return,
                Err(text) => *last.borrow_mut() = text,
            }
            clock.sleep(wait.poll).await;
        }
    };
    tokio::select! {
        () = asking => Ok(()),
        ended = held.ended() => Err(BootEnd::Ended(ended)),
        () = tokio::time::sleep(wait.deadline) => Err(BootEnd::NoAnswer(last.borrow().clone())),
    }
}

/// the text of a boot that failed; `kind` is [`JOB_VM`] or [`BUILD_VM`]. A locked
/// login keychain is the documented cause, so it names the host document's section.
#[must_use]
pub fn boot_failure_text(kind: &str, vm: &str, end: &BootEnd) -> String {
    let cause = match end {
        BootEnd::Ended(ended) => {
            let code = ended.code.map_or_else(
                || "no exit code".to_string(),
                |code| format!("exit code {code}"),
            );
            let text = ended.error_text();
            let text = if text.is_empty() { "no output" } else { &text };
            format!("tart run for the {kind} {vm} ended with {code} ({text})")
        }
        BootEnd::NoAnswer(text) => {
            let text = if text.is_empty() { "no error" } else { text };
            format!(
                "the guest agent of the {kind} {vm} did not answer by the boot deadline ({text})"
            )
        }
    };
    format!(
        "boot failure: {cause}; a locked login keychain is the documented cause, see {}",
        section_of_document(SIGNED_IN_SECTION)
    )
}
