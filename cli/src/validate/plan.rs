//! What a valid configuration plans.

use crate::profile::{Os, Profile};

/// A valid host configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub profile: Profile,
    pub image_name: Option<String>,
    pub entries: Vec<Entry>,
    /// The host-wide limits of an axmc configuration.
    pub budget: Option<Budget>,
}

/// One target repository's pool.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Where the entry sits in the file, such as `repositories[0]`.
    pub path: String,
    pub owner: String,
    pub repository: String,
    pub name: String,
    pub slots: u32,
    pub token_path: String,
    pub labels: Vec<String>,
    pub cpus: f64,
    pub memory_gb: u32,
    pub volumes: Vec<PlannedVolume>,
    pub diagnostics: bool,
    /// Where a diagnostics-enabled pool's evidence goes, as the configuration names
    /// it; `None` when diagnostics are off.
    pub diagnostic_directory: Option<String>,
    pub diagnostic_raw_records: bool,
    /// The OS of an axmc pool's virtual machines.
    pub os: Option<Os>,
    /// The per-VM disk cap of an axmc pool.
    pub disk_gb: Option<u32>,
}

/// The most an axmc host can give to all pools together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    pub cpus: f64,
    pub memory_gb: u32,
    pub disk_gb: u32,
}

/// A cache volume of an axpc pool: its name starts with the entry's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedVolume {
    pub name: String,
    pub mount_path: String,
}
