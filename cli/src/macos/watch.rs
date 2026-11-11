//! the judgment of the primary network over successive readings: a pure state
//! machine, so the rules are tested without a clock or a process.

use super::network::{NetworkReading, PrimaryNetwork};
use super::reason::{NO_INTERFACE, network_change_reason};

/// how often the instance reads the primary network.
pub const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// how many checks in a row make a change, and how many a network must hold before
/// it is recorded.
pub const CHECKS_IN_A_ROW: u32 = 2;

/// the change that stops every runner: the recorded interface and the interface of
/// the latest differing reading, or [`NO_INTERFACE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkChange {
    pub from: String,
    pub to: String,
}

impl NetworkChange {
    #[must_use]
    pub fn reason(&self) -> String {
        network_change_reason(&self.from, &self.to)
    }
}

/// what one check concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchOutcome {
    /// the reading matches the record.
    Steady,
    /// the reading differs from the record on the first of two checks.
    Differing,
    /// there is no record yet, and no primary network has held for two checks.
    Waiting,
    /// a primary network held for two checks and is now the record.
    Recorded(PrimaryNetwork),
    /// the reading differed on two checks in a row.
    Changed(NetworkChange),
}

#[derive(Debug)]
enum Phase {
    Waiting {
        candidate: Option<PrimaryNetwork>,
        held: u32,
    },
    Recorded {
        network: PrimaryNetwork,
        differing: u32,
    },
    Changed(NetworkChange),
}

/// the record of the primary network and the count of differing checks. A watch
/// that has reported a change keeps reporting it; a resume builds a new one.
#[derive(Debug)]
pub struct NetworkWatch {
    phase: Phase,
}

impl NetworkWatch {
    /// starts from the first reading at start or resume: a primary network is
    /// recorded at once, and no default route waits for one.
    #[must_use]
    pub fn begin(first: NetworkReading) -> Self {
        let phase = match first {
            NetworkReading::Primary(network) => Phase::Recorded {
                network,
                differing: 0,
            },
            NetworkReading::NoDefaultRoute => Phase::Waiting {
                candidate: None,
                held: 0,
            },
        };
        Self { phase }
    }

    /// whether the watch is still waiting for a primary network to record.
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        matches!(self.phase, Phase::Waiting { .. })
    }

    /// the recorded primary network.
    #[must_use]
    pub fn recorded(&self) -> Option<&PrimaryNetwork> {
        match &self.phase {
            Phase::Recorded { network, .. } => Some(network),
            _ => None,
        }
    }

    /// judges one reading.
    pub fn observe(&mut self, reading: NetworkReading) -> WatchOutcome {
        match &mut self.phase {
            Phase::Waiting { candidate, held } => {
                let NetworkReading::Primary(network) = reading else {
                    *candidate = None;
                    *held = 0;
                    return WatchOutcome::Waiting;
                };
                match candidate {
                    Some(earlier) if earlier.is_same_network_as(&network) => *held += 1,
                    _ => *held = 1,
                }
                if *held < CHECKS_IN_A_ROW {
                    *candidate = Some(network);
                    return WatchOutcome::Waiting;
                }
                self.phase = Phase::Recorded {
                    network: network.clone(),
                    differing: 0,
                };
                WatchOutcome::Recorded(network)
            }
            Phase::Recorded { network, differing } => {
                if let NetworkReading::Primary(read) = &reading
                    && network.is_same_network_as(read)
                {
                    network.hardware = network.hardware.or(read.hardware);
                    *differing = 0;
                    return WatchOutcome::Steady;
                }
                *differing += 1;
                if *differing < CHECKS_IN_A_ROW {
                    return WatchOutcome::Differing;
                }
                let change = NetworkChange {
                    from: network.interface.clone(),
                    to: match reading {
                        NetworkReading::Primary(read) => read.interface,
                        NetworkReading::NoDefaultRoute => NO_INTERFACE.to_string(),
                    },
                };
                self.phase = Phase::Changed(change.clone());
                WatchOutcome::Changed(change)
            }
            Phase::Changed(change) => WatchOutcome::Changed(change.clone()),
        }
    }
}
