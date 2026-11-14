//! the words that name the operating system Marshal runs on and what runs its jobs:
//! where tokens are kept, where the icon lives, how a stuck process is ended, which
//! document says how to recover, what a runner runs in and what Build makes. The view
//! takes them from its input, which has W1's Windows words unless the app gives the
//! host's, so every view can be drawn and tested in either set on any operating system.

/// one operating system's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostWords {
    /// the store that keeps tokens.
    pub store: &'static str,
    /// the main window's footer note on closing it.
    pub close_note: &'static str,
    /// why another Marshal may hold the lock, and how to find it.
    pub lock_held: &'static str,
    /// how to end an agent that does not answer.
    pub end_stuck_agent: &'static str,
    /// the document with the manual recovery and the token permissions.
    pub host_document: &'static str,
    /// what Build makes and the runners run on: "runner image" or "golden VM".
    pub image: &'static str,
    /// the same, where W1's words say only "image".
    pub image_short: &'static str,
    /// what one runner runs in, and more than one.
    pub workload: &'static str,
    pub workloads: &'static str,
    /// the program that builds the image and whose output goes to the log.
    pub engine: &'static str,
    /// a pool that waits for that program, and what to do about it.
    pub waiting: &'static str,
    pub start_engine: &'static str,
}

pub const WINDOWS: HostWords = HostWords {
    store: "Windows Credential Manager",
    close_note: "Closing this window keeps Marshal running in the notification area.",
    lock_held: "Another Marshal process holds the runner lock, and Marshal cannot tell which: one that is starting or ending, an older version, or another Windows user's. Marshal checks again every 5 s. If it stays, look in Task Manager for marshal.exe or marshal-app.exe, or sign the other user out.",
    end_stuck_agent: "End marshal-app.exe in Task Manager, then open Marshal again; the next start removes the containers it left.",
    host_document: "Windows Runner Host",
    image: "runner image",
    image_short: "image",
    workload: "container",
    workloads: "containers",
    engine: "Docker",
    waiting: "Waiting for Docker",
    start_engine: "Start Docker Desktop. Marshal checks every 5 s.",
};

pub const MACOS: HostWords = HostWords {
    store: "the login keychain",
    close_note: "Closing this window keeps Marshal running in the menu bar.",
    lock_held: "Another Marshal process holds the runner lock, and Marshal cannot tell which: one that is starting or ending, an older version such as the marshal command, or another user's. Marshal checks again every 5 s. If it stays, look in Activity Monitor for marshal or Marshal, or log the other user out.",
    end_stuck_agent: "Force quit Marshal's agent in Activity Monitor, then open Marshal again; the next start deletes the VMs it left.",
    host_document: "macOS Runner Host",
    image: "golden VM",
    image_short: "golden VM",
    workload: "VM",
    workloads: "VMs",
    engine: "Tart",
    waiting: "Waiting for Tart",
    start_engine: "Install Tart and Softnet, and set Softnet's SUID bit, as macOS Runner Host says. Marshal checks every 5 s.",
};

/// the words of the operating system this build is for, which the app gives the view.
pub const HOST: HostWords = if cfg!(target_os = "macos") {
    MACOS
} else {
    WINDOWS
};

#[cfg(test)]
mod tests {
    use super::{HOST, MACOS, WINDOWS};

    #[test]
    fn the_hosts_words_are_this_targets() {
        let expected = if cfg!(target_os = "macos") {
            MACOS
        } else {
            WINDOWS
        };
        assert_eq!(HOST, expected);
    }

    #[test]
    fn the_macs_words_name_nothing_of_windows_or_docker() {
        let all = [
            MACOS.store,
            MACOS.close_note,
            MACOS.lock_held,
            MACOS.end_stuck_agent,
            MACOS.host_document,
            MACOS.image,
            MACOS.image_short,
            MACOS.workload,
            MACOS.workloads,
            MACOS.engine,
            MACOS.waiting,
            MACOS.start_engine,
        ];
        for text in all {
            for windows in [
                "Windows",
                "Credential Manager",
                "Task Manager",
                ".exe",
                "notification area",
                "container",
                "Docker",
                "image",
            ] {
                assert!(!text.contains(windows), "{text:?} names {windows:?}");
            }
        }
    }
}
