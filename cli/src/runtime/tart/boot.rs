//! the boot of a VM the CLI holds: the wait for its guest agent and the text of a
//! boot that fails, shared by a job VM and the build VM of the golden VM.

use std::cell::RefCell;
use std::time::Duration;

use crate::macos::prerequisites::{SIGNED_IN_SECTION, SOFTNET_SECTION, section_of_document};
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
    /// `tart run` could not be started at all, the text saying why.
    NotStarted(String),
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

/// Tart's texts for a locked login keychain, as its FAQ lists them.
const KEYCHAIN_TEXTS: [&str; 3] = [
    "SecKeyCreateRandomKey_ios failed",
    "Failed to generate keypair",
    "Interaction is not allowed with the Security Server",
];

/// Tart's Softnet errors that a `tart run` the runtime starts can end with.
const SOFTNET_TEXTS: [&str; 2] = [
    "Softnet process terminated prematurely",
    "softnet not found in PATH",
];

/// the text of a boot that failed; `kind` is [`JOB_VM`] or [`BUILD_VM`]. A `tart run`
/// that ended names the host document's section for a locked login keychain, for
/// Softnet, or for both, when its whole output carries one of Tart's texts for that
/// cause; any other failure names no cause.
#[must_use]
pub fn boot_failure_text(kind: &str, vm: &str, end: &BootEnd) -> String {
    let (cause, hint) = match end {
        BootEnd::NotStarted(text) => {
            return format!(
                "boot failure: tart run for the {kind} {vm} could not be started ({text})"
            );
        }
        BootEnd::Ended(ended) => {
            let code = ended.code.map_or_else(
                || "no exit code".to_string(),
                |code| format!("exit code {code}"),
            );
            let text = ended.error_text();
            let text = if text.is_empty() { "no output" } else { &text };
            (
                format!("tart run for the {kind} {vm} ended with {code} ({text})"),
                tart_hint(ended),
            )
        }
        BootEnd::NoAnswer(text) => {
            let text = if text.is_empty() { "no error" } else { text };
            (
                format!(
                    "the guest agent of the {kind} {vm} did not answer by the boot deadline ({text})"
                ),
                String::new(),
            )
        }
    };
    format!("boot failure: {cause}{hint}")
}

/// the hint a boot failure appends for the cause Tart's output points to, empty when
/// it points to none; read from the whole output, not the cut error text.
fn tart_hint(ended: &Captured) -> String {
    let output = format!("{}\n{}", ended.stdout, ended.stderr);
    let carries = |texts: &[&str]| texts.iter().any(|text| output.contains(text));
    let keychain = format!(
        "a locked login keychain, see {}",
        section_of_document(SIGNED_IN_SECTION)
    );
    let softnet = format!("Softnet, see {}", section_of_document(SOFTNET_SECTION));
    match (carries(&KEYCHAIN_TEXTS), carries(&SOFTNET_TEXTS)) {
        (true, true) => format!("; Tart's error points to {keychain}, and to {softnet}"),
        (true, false) => format!("; Tart's error points to {keychain}"),
        (false, true) => format!("; Tart's error points to {softnet}"),
        (false, false) => String::new(),
    }
}
