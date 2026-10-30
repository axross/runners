//! The two host profiles a configuration can be validated against.

use std::fmt;

/// The machine a configuration describes. It decides the registration labels, the
/// path form of private paths, and which fields exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The Windows host with Docker Desktop.
    Axpc,
    /// The macOS host that runs VM pools.
    Axmc,
}

/// The operating system of an axmc pool's virtual machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
}

const AXPC_PLATFORM_LABELS: [&str; 3] = ["self-hosted", "linux", "x64"];
const AXMC_LINUX_PLATFORM_LABELS: [&str; 3] = ["self-hosted", "linux", "ARM64"];
const AXMC_MACOS_PLATFORM_LABELS: [&str; 3] = ["self-hosted", "macOS", "ARM64"];
/// Every platform label either host registers. A pool cannot carry one it does not
/// provide itself.
const ALL_PLATFORM_LABELS: [&str; 5] = ["self-hosted", "linux", "x64", "macOS", "ARM64"];

const AXPC_ROOT_FIELDS: [&str; 2] = ["imageName", "repositories"];
const AXMC_ROOT_FIELDS: [&str; 3] = ["imageName", "repositories", "budget"];
const AXPC_ENTRY_FIELDS: [&str; 12] = [
    "owner",
    "repository",
    "name",
    "slots",
    "tokenPath",
    "labels",
    "volumes",
    "cpus",
    "memoryGb",
    "diagnostics",
    "diagnosticDirectory",
    "diagnosticRawRecords",
];
const AXMC_ENTRY_FIELDS: [&str; 14] = [
    "owner",
    "repository",
    "name",
    "slots",
    "tokenPath",
    "labels",
    "volumes",
    "cpus",
    "memoryGb",
    "diagnostics",
    "diagnosticDirectory",
    "diagnosticRawRecords",
    "os",
    "diskGb",
];

impl Profile {
    /// Parses a profile name as `--profile` spells it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "axpc" => Some(Self::Axpc),
            "axmc" => Some(Self::Axmc),
            _ => None,
        }
    }

    /// The profile of the machine the CLI runs on, when that machine is one of the
    /// two hosts.
    #[must_use]
    pub fn of_this_machine() -> Option<Self> {
        if cfg!(target_os = "windows") {
            Some(Self::Axpc)
        } else if cfg!(target_os = "macos") {
            Some(Self::Axmc)
        } else {
            None
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Axpc => "axpc",
            Self::Axmc => "axmc",
        }
    }

    /// The label a pool carries when its `labels` is absent or empty.
    #[must_use]
    pub fn routing_label(self) -> &'static str {
        self.name()
    }

    /// The labels every registration carries. An axmc pool whose OS is unusable
    /// carries only what both OSes share.
    #[must_use]
    pub fn platform_labels(self, os: Option<Os>) -> &'static [&'static str] {
        match (self, os) {
            (Self::Axpc, _) => &AXPC_PLATFORM_LABELS,
            (Self::Axmc, Some(Os::Macos)) => &AXMC_MACOS_PLATFORM_LABELS,
            (Self::Axmc, Some(Os::Linux)) => &AXMC_LINUX_PLATFORM_LABELS,
            (Self::Axmc, None) => &["self-hosted", "ARM64"],
        }
    }

    /// The platform labels of any host or OS. An axmc pool rejects one that it does
    /// not provide itself; axpc keeps accepting them, as its schema always has.
    #[must_use]
    pub fn foreign_platform_labels(self) -> &'static [&'static str] {
        match self {
            Self::Axpc => &[],
            Self::Axmc => &ALL_PLATFORM_LABELS,
        }
    }

    #[must_use]
    pub fn root_fields(self) -> &'static [&'static str] {
        match self {
            Self::Axpc => &AXPC_ROOT_FIELDS,
            Self::Axmc => &AXMC_ROOT_FIELDS,
        }
    }

    #[must_use]
    pub fn entry_fields(self) -> &'static [&'static str] {
        match self {
            Self::Axpc => &AXPC_ENTRY_FIELDS,
            Self::Axmc => &AXMC_ENTRY_FIELDS,
        }
    }
}

impl Os {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::Macos),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
