//! what a valid configuration plans.

use crate::profile::{Os, Profile};

/// a valid host configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub profile: Profile,
    pub image_name: Option<String>,
    pub entries: Vec<Entry>,
    /// the host-wide limits of an axmc configuration.
    pub budget: Option<Budget>,
    /// what a file with settings of its own carries; JSON has none.
    pub settings: Option<Settings>,
}

/// the settings a file carries beyond the host's rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub schema_version: u32,
    /// the label a pool without labels carries besides the platform's.
    pub routing_label: String,
    pub lifecycle: Lifecycle,
}

/// the `[lifecycle]` table: each value is kept as the file gives it, and an absent
/// one stays unset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lifecycle {
    pub start_at_login: Option<bool>,
    pub start_runners_on_launch: Option<bool>,
    pub keep_awake: Option<bool>,
    pub resume_after_stop: Option<bool>,
}

/// one target repository's pool.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// the entry's position among the file's entries, counting from 0.
    pub index: usize,
    /// where the entry sits in the file, in the reader's path form, such as
    /// `repositories[0]`.
    pub path: String,
    pub owner: String,
    pub repository: String,
    pub name: String,
    pub slots: u32,
    /// the token file's path, for a format that has one.
    pub token_path: Option<String>,
    pub labels: Vec<String>,
    pub cpus: f64,
    pub memory_gb: u32,
    pub volumes: Vec<PlannedVolume>,
    pub diagnostics: bool,
    /// where a diagnostics-enabled pool's evidence goes, as the configuration names
    /// it; `None` when diagnostics are off.
    pub diagnostic_directory: Option<String>,
    pub diagnostic_raw_records: bool,
    /// the OS of an axmc pool's virtual machines; a JSON axpc pool has none, and a TOML
    /// one on Windows has Linux, the only one it runs.
    pub os: Option<Os>,
    /// the per-VM disk cap of an axmc pool.
    pub disk_gb: Option<u32>,
}

/// the most an axmc host can give to all pools together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    pub cpus: f64,
    pub memory_gb: u32,
    pub disk_gb: u32,
}

/// a cache volume of an axpc pool: its name starts with the entry's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedVolume {
    pub name: String,
    pub mount_path: String,
}
