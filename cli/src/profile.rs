//! the two host profiles a configuration can be validated against.

use std::fmt;

/// the machine a configuration describes, which decides its registration labels,
/// the path form of its private paths, and its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// the Windows host with Docker Desktop.
    Axpc,
    /// the macOS host that runs VM pools.
    Axmc,
}

/// the operating system of an axmc pool's virtual machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
}

const AXPC_PLATFORM_LABELS: [&str; 3] = ["self-hosted", "linux", "x64"];
const AXMC_LINUX_PLATFORM_LABELS: [&str; 3] = ["self-hosted", "linux", "ARM64"];
const AXMC_MACOS_PLATFORM_LABELS: [&str; 3] = ["self-hosted", "macOS", "ARM64"];
/// every platform label either host registers; a pool cannot carry one it does not
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
    /// parses a profile name as `--profile` spells it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "axpc" => Some(Self::Axpc),
            "axmc" => Some(Self::Axmc),
            _ => None,
        }
    }

    /// the profile of the machine the CLI runs on, when that machine is one of the
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

    /// the label a pool carries when its `labels` is absent or empty.
    #[must_use]
    pub fn routing_label(self) -> &'static str {
        self.name()
    }

    /// the labels every registration carries, only what both OSes share for an axmc
    /// pool whose OS is unusable.
    #[must_use]
    pub fn platform_labels(self, os: Option<Os>) -> &'static [&'static str] {
        match (self, os) {
            (Self::Axpc, _) => &AXPC_PLATFORM_LABELS,
            (Self::Axmc, Some(Os::Macos)) => &AXMC_MACOS_PLATFORM_LABELS,
            (Self::Axmc, Some(Os::Linux)) => &AXMC_LINUX_PLATFORM_LABELS,
            (Self::Axmc, None) => &["self-hosted", "ARM64"],
        }
    }

    /// the platform labels of any host or OS, which an axmc pool rejects when it does
    /// not provide them itself; axpc accepts them, as its schema always has.
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
