//! the resume after a sleep or network stop, while `resume_after_stop` is on: a pure
//! state machine, so the rules are tested without a clock or a process. The caller
//! reads the primary network every check, as the network watch does, and starts the
//! runners as Start all does when [`Resume::observe`] says so.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::macos::network::{NetworkReading, PrimaryNetwork};
use crate::macos::watch::CHECKS_IN_A_ROW;

/// how many automatic resumes [`WINDOW`] allows before a signal stop arms no resume.
pub const LIMIT: usize = 5;

/// the rolling window the automatic resumes are counted over.
pub const WINDOW: Duration = Duration::from_secs(60 * 60);

/// what a signal stop did to the resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Armed {
    /// the runners start again once the recorded network has settled.
    Waiting,
    /// the network the runners ran on has no gateway hardware address, as a
    /// full-tunnel VPN's does, so nothing proves the same network later.
    NoGateway,
    /// the runners resumed [`LIMIT`] times within [`WINDOW`].
    TooOften,
}

#[derive(Debug)]
struct Pending {
    network: PrimaryNetwork,
    held: u32,
}

/// the pending resume and the count of automatic resumes, kept in memory.
#[derive(Debug, Default)]
pub struct Resume {
    pending: Option<Pending>,
    resumes: VecDeque<Instant>,
}

impl Resume {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// arms a resume at a signal stop at `now`, on `ran_on`, the primary network the
    /// runners last ran on.
    pub fn arm(&mut self, now: Instant, ran_on: Option<PrimaryNetwork>) -> Armed {
        self.pending = None;
        while self
            .resumes
            .front()
            .is_some_and(|&resumed| now.saturating_duration_since(resumed) >= WINDOW)
        {
            self.resumes.pop_front();
        }
        if self.resumes.len() >= LIMIT {
            return Armed::TooOften;
        }
        match ran_on {
            Some(network) if network.hardware.is_some() => {
                self.pending = Some(Pending { network, held: 0 });
                Armed::Waiting
            }
            _ => Armed::NoGateway,
        }
    }

    /// cancels a pending resume: at Stop all, Quit, Build, a user's Start all, or the
    /// setting turned off.
    pub fn disarm(&mut self) {
        self.pending = None;
    }

    /// whether a resume is pending.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// judges one reading while stopped, at `now`: true when the recorded network,
    /// with its gateway's hardware address read, has held for [`CHECKS_IN_A_ROW`]
    /// checks. The resume is then spent and counted.
    pub fn observe(&mut self, now: Instant, reading: &NetworkReading) -> bool {
        let Some(pending) = &mut self.pending else {
            return false;
        };
        let same = match reading {
            NetworkReading::Primary(read) => {
                read.hardware.is_some() && pending.network.is_same_network_as(read)
            }
            NetworkReading::NoDefaultRoute => false,
        };
        pending.held = if same { pending.held + 1 } else { 0 };
        if pending.held < CHECKS_IN_A_ROW {
            return false;
        }
        self.pending = None;
        self.resumes.push_back(now);
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::macos::network::HardwareAddress;

    fn network(interface: &str, hardware: Option<u8>) -> PrimaryNetwork {
        PrimaryNetwork {
            interface: interface.to_string(),
            router: Some(Ipv4Addr::new(192, 168, 1, 1)),
            hardware: hardware.map(|last| HardwareAddress::new([2, 0, 0, 0, 0, last])),
        }
    }

    fn read(interface: &str, hardware: Option<u8>) -> NetworkReading {
        NetworkReading::Primary(network(interface, hardware))
    }

    #[test]
    fn the_same_network_with_its_hardware_address_on_two_checks_in_a_row_resumes() {
        let start = Instant::now();
        let mut resume = Resume::new();
        assert_eq!(
            resume.arm(start, Some(network("Wi-Fi 12", Some(1)))),
            Armed::Waiting
        );
        assert!(!resume.observe(start, &read("Wi-Fi 12", Some(1))));
        assert!(!resume.observe(start, &read("Ethernet 3", Some(1))));
        assert!(!resume.observe(start, &read("Wi-Fi 12", Some(1))));
        assert!(resume.observe(start, &read("Wi-Fi 12", Some(1))));
        assert!(!resume.is_pending());
        assert!(!resume.observe(start, &read("Wi-Fi 12", Some(1))));
    }

    #[test]
    fn a_reading_without_the_hardware_address_or_with_another_one_never_counts() {
        let start = Instant::now();
        let mut resume = Resume::new();
        resume.arm(start, Some(network("Wi-Fi 12", Some(1))));
        for reading in [
            read("Wi-Fi 12", None),
            read("Wi-Fi 12", None),
            read("Wi-Fi 12", Some(9)),
            read("Wi-Fi 12", Some(9)),
            NetworkReading::NoDefaultRoute,
            NetworkReading::NoDefaultRoute,
        ] {
            assert!(!resume.observe(start, &reading));
        }
        assert!(resume.is_pending());
    }

    #[test]
    fn a_network_without_a_gateway_hardware_address_arms_nothing() {
        let mut resume = Resume::new();
        assert_eq!(
            resume.arm(Instant::now(), Some(network("VPN 7", None))),
            Armed::NoGateway
        );
        assert_eq!(resume.arm(Instant::now(), None), Armed::NoGateway);
        assert!(!resume.is_pending());
    }

    #[test]
    fn disarming_cancels_the_pending_resume() {
        let start = Instant::now();
        let mut resume = Resume::new();
        resume.arm(start, Some(network("Wi-Fi 12", Some(1))));
        resume.observe(start, &read("Wi-Fi 12", Some(1)));
        resume.disarm();
        assert!(!resume.observe(start, &read("Wi-Fi 12", Some(1))));
        assert!(!resume.is_pending());
    }

    #[test]
    fn the_sixth_stop_within_an_hour_of_five_resumes_arms_none_until_the_first_leaves_the_hour() {
        let start = Instant::now();
        let mut resume = Resume::new();
        let wifi = || Some(network("Wi-Fi 12", Some(1)));
        for minute in 0..5 {
            let now = start + Duration::from_secs(minute * 60);
            assert_eq!(resume.arm(now, wifi()), Armed::Waiting);
            resume.observe(now, &read("Wi-Fi 12", Some(1)));
            assert!(resume.observe(now, &read("Wi-Fi 12", Some(1))));
        }
        let just_inside = start + WINDOW - Duration::from_secs(1);
        assert_eq!(resume.arm(just_inside, wifi()), Armed::TooOften);
        assert!(!resume.is_pending());
        assert_eq!(resume.arm(start + WINDOW, wifi()), Armed::Waiting);
    }
}
