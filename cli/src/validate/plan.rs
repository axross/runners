//! what a valid configuration plans.

use std::fmt;

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
    pub stop_on_network_change: Option<bool>,
    pub resume_after_stop: Option<bool>,
}

/// a pool's registration destination; an organisation group name is resolved and
/// checked at registration, not authorised by configuration validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolTarget {
    Repository {
        owner: String,
        repository: String,
    },
    Organization {
        organization: String,
        runner_group: String,
    },
}

impl PoolTarget {
    #[must_use]
    pub fn repository(owner: impl Into<String>, repository: impl Into<String>) -> Self {
        Self::Repository {
            owner: owner.into(),
            repository: repository.into(),
        }
    }

    /// whether both targets are repositories with the same owner and name, in any case.
    #[must_use]
    pub fn same_repository(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Repository { owner, repository },
                Self::Repository {
                    owner: other_owner,
                    repository: other_repository,
                },
            ) => {
                owner.eq_ignore_ascii_case(other_owner)
                    && repository.eq_ignore_ascii_case(other_repository)
            }
            _ => false,
        }
    }

    /// the resource owner as configured, before token-key normalisation.
    #[must_use]
    pub fn owner(&self) -> &str {
        match self {
            Self::Repository { owner, .. } => owner,
            Self::Organization { organization, .. } => organization,
        }
    }
}

impl fmt::Display for PoolTarget {
    /// a human-readable destination, not a token-store key or configuration value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository { owner, repository } => write!(f, "{owner}/{repository}"),
            Self::Organization {
                organization,
                runner_group,
            } => write!(f, "{organization} (runner group {runner_group})"),
        }
    }
}

/// one pool after the selected profile's configuration checks have succeeded.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// the entry's position among the file's entries, counting from 0.
    pub index: usize,
    /// where the entry sits in the file, in the reader's path form, such as
    /// `repositories[0]`.
    pub path: String,
    pub target: PoolTarget,
    pub name: String,
    /// the routing and runner identity of an on-demand pool, distinct from its
    /// logical name; absent pools keep the always-on registration labels.
    pub scale_set: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::PoolTarget;

    fn organization(organization: &str) -> PoolTarget {
        PoolTarget::Organization {
            organization: organization.to_owned(),
            runner_group: "group".to_owned(),
        }
    }

    #[test]
    fn repositories_match_by_owner_and_name_in_any_case() {
        let target = PoolTarget::repository("Owner", "Repo");
        assert!(target.same_repository(&PoolTarget::repository("owner", "REPO")));
        assert!(!target.same_repository(&PoolTarget::repository("other", "Repo")));
        assert!(!target.same_repository(&PoolTarget::repository("Owner", "other")));
    }

    #[test]
    fn an_organisation_is_never_the_same_repository() {
        let target = PoolTarget::repository("owner", "repo");
        assert!(!target.same_repository(&organization("owner")));
        assert!(!organization("owner").same_repository(&target));
        assert!(!organization("owner").same_repository(&organization("owner")));
    }
}
