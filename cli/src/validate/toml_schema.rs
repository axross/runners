//! the TOML file's tables and the keys each defines, as schema version 1 states them.

use crate::profile::Profile;

use super::problem::Key;

/// a table of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Table {
    Root,
    Pool,
    Budget,
    Lifecycle,
    Volume,
}

/// what a key holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Child {
    /// a scalar or a list of them.
    Leaf,
    Table(Table),
    /// an array of tables of one kind.
    Tables(Table),
}

/// a key a table defines, for both hosts or for one.
pub(super) struct Member {
    pub key: Key,
    pub child: Child,
    only_on: Option<Profile>,
}

const fn member(key: Key, child: Child) -> Member {
    Member {
        key,
        child,
        only_on: None,
    }
}

const fn member_on(key: Key, child: Child, profile: Profile) -> Member {
    Member {
        key,
        child,
        only_on: Some(profile),
    }
}

const ROOT: [Member; 6] = [
    member(Key::SchemaVersion, Child::Leaf),
    member(Key::RoutingLabel, Child::Leaf),
    member(Key::ImageName, Child::Leaf),
    member_on(Key::Budget, Child::Table(Table::Budget), Profile::Axmc),
    member(Key::Lifecycle, Child::Table(Table::Lifecycle)),
    member(Key::Repositories, Child::Tables(Table::Pool)),
];

const POOL: [Member; 12] = [
    member(Key::Name, Child::Leaf),
    member(Key::Repository, Child::Leaf),
    member(Key::Slots, Child::Leaf),
    member(Key::Labels, Child::Leaf),
    member(Key::Os, Child::Leaf),
    member(Key::Cpus, Child::Leaf),
    member(Key::MemoryGb, Child::Leaf),
    member_on(Key::DiskGb, Child::Leaf, Profile::Axmc),
    member(Key::Diagnostics, Child::Leaf),
    member(Key::DiagnosticDirectory, Child::Leaf),
    member(Key::DiagnosticRawRecords, Child::Leaf),
    member(Key::Volumes, Child::Tables(Table::Volume)),
];

const BUDGET: [Member; 3] = [
    member(Key::Cpus, Child::Leaf),
    member(Key::MemoryGb, Child::Leaf),
    member(Key::DiskGb, Child::Leaf),
];

const LIFECYCLE: [Member; 4] = [
    member(Key::StartAtLogin, Child::Leaf),
    member(Key::StartRunnersOnLaunch, Child::Leaf),
    member(Key::KeepAwake, Child::Leaf),
    member(Key::ResumeAfterStop, Child::Leaf),
];

const VOLUME: [Member; 2] = [
    member(Key::Suffix, Child::Leaf),
    member(Key::MountPath, Child::Leaf),
];

impl Table {
    pub fn members(self) -> &'static [Member] {
        match self {
            Self::Root => &ROOT,
            Self::Pool => &POOL,
            Self::Budget => &BUDGET,
            Self::Lifecycle => &LIFECYCLE,
            Self::Volume => &VOLUME,
        }
    }

    /// the member that `name` spells in a file for `profile`, or `None` when the table
    /// does not define it there.
    pub fn defined(self, name: &str, profile: Profile) -> Option<&'static Member> {
        self.members().iter().find(|member| {
            name == key_name(member.key) && member.only_on.is_none_or(|on| on == profile)
        })
    }

    /// the member that `name` spells whatever the host, which the tree holds so that
    /// the rules can reject it for the host that lacks it.
    pub fn named(self, name: &str) -> Option<&'static Member> {
        self.members()
            .iter()
            .find(|member| name == key_name(member.key))
    }
}

/// the key as the TOML file spells it; an owner is part of `repository`.
pub(crate) fn key_name(key: Key) -> &'static str {
    match key {
        Key::ImageName => "image_name",
        Key::Repositories => "pool",
        Key::Budget => "budget",
        Key::Owner | Key::Repository => "repository",
        Key::Name => "name",
        Key::Slots => "runners",
        Key::TokenPath => "token_path",
        Key::Labels => "labels",
        Key::Volumes => "volumes",
        Key::Cpus => "cpus",
        Key::MemoryGb => "memory_gb",
        Key::Diagnostics => "diagnostics",
        Key::DiagnosticDirectory => "diagnostic_directory",
        Key::DiagnosticRawRecords => "diagnostic_raw_records",
        Key::Os => "os",
        Key::DiskGb => "disk_gb",
        Key::Suffix => "suffix",
        Key::MountPath => "mount_path",
        Key::SchemaVersion => "schema_version",
        Key::RoutingLabel => "routing_label",
        Key::Lifecycle => "lifecycle",
        Key::StartAtLogin => "start_at_login",
        Key::StartRunnersOnLaunch => "start_runners_on_launch",
        Key::KeepAwake => "keep_awake",
        Key::ResumeAfterStop => "resume_after_stop",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_but_the_owner_and_token_path_belongs_to_a_table() {
        let tables = [
            Table::Root,
            Table::Pool,
            Table::Budget,
            Table::Lifecycle,
            Table::Volume,
        ];
        for key in Key::ALL {
            let held = tables
                .iter()
                .any(|table| table.members().iter().any(|member| member.key == key));
            assert_eq!(held, !matches!(key, Key::Owner | Key::TokenPath), "{key:?}");
        }
    }

    #[test]
    fn a_table_defines_each_name_once() {
        for table in [
            Table::Root,
            Table::Pool,
            Table::Budget,
            Table::Lifecycle,
            Table::Volume,
        ] {
            let mut names: Vec<&str> = table.members().iter().map(|m| key_name(m.key)).collect();
            names.sort_unstable();
            let count = names.len();
            names.dedup();
            assert_eq!(names.len(), count, "{table:?}");
        }
    }

    #[test]
    fn the_host_without_a_key_does_not_define_it() {
        assert!(Table::Root.defined("budget", Profile::Axmc).is_some());
        assert!(Table::Root.defined("budget", Profile::Axpc).is_none());
        assert!(Table::Pool.defined("disk_gb", Profile::Axpc).is_none());
        assert!(Table::Pool.defined("token_path", Profile::Axmc).is_none());
        assert!(Table::Pool.defined("Runners", Profile::Axmc).is_none());
    }
}
