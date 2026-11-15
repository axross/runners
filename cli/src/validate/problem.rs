//! what the rule set records when a configuration breaks a rule: the key's path, the
//! kind of problem with its values, and where the reader found the key. the rule set
//! words nothing; a [`Reader`](super::Reader) renders each problem in its format's
//! path form and names.

use crate::profile::Os;

use super::limits::{CPU_RANGE, disk_reserve, format_nanos, linux_disk_too_small};

/// a key of the host configuration, whatever a format calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    ImageName,
    Repositories,
    Budget,
    Owner,
    Repository,
    Name,
    Slots,
    TokenPath,
    Labels,
    Volumes,
    Cpus,
    MemoryGb,
    Diagnostics,
    DiagnosticDirectory,
    DiagnosticRawRecords,
    Os,
    DiskGb,
    Suffix,
    MountPath,
    /// the keys below exist only in a format whose file carries settings of its own.
    SchemaVersion,
    RoutingLabel,
    Lifecycle,
    StartAtLogin,
    StartRunnersOnLaunch,
    KeepAwake,
    StopOnNetworkChange,
    ResumeAfterStop,
}

impl Key {
    pub const ALL: [Self; 27] = [
        Self::ImageName,
        Self::Repositories,
        Self::Budget,
        Self::Owner,
        Self::Repository,
        Self::Name,
        Self::Slots,
        Self::TokenPath,
        Self::Labels,
        Self::Volumes,
        Self::Cpus,
        Self::MemoryGb,
        Self::Diagnostics,
        Self::DiagnosticDirectory,
        Self::DiagnosticRawRecords,
        Self::Os,
        Self::DiskGb,
        Self::Suffix,
        Self::MountPath,
        Self::SchemaVersion,
        Self::RoutingLabel,
        Self::Lifecycle,
        Self::StartAtLogin,
        Self::StartRunnersOnLaunch,
        Self::KeepAwake,
        Self::StopOnNetworkChange,
        Self::ResumeAfterStop,
    ];

    /// whether only a format with file settings has the key, which JSON does not.
    #[must_use]
    pub fn is_file_setting(self) -> bool {
        matches!(
            self,
            Self::SchemaVersion
                | Self::RoutingLabel
                | Self::Lifecycle
                | Self::StartAtLogin
                | Self::StartRunnersOnLaunch
                | Self::KeepAwake
                | Self::StopOnNetworkChange
                | Self::ResumeAfterStop
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Key(Key),
    Index(usize),
}

/// the keys and list positions from the root to a value; the root has none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyPath(Vec<Segment>);

impl KeyPath {
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    /// the path as a format writes it, with each key spelled by `name`: keys joined by
    /// dots and positions in brackets; the root is empty.
    #[must_use]
    pub fn text(&self, name: fn(Key) -> &'static str) -> String {
        let mut text = String::new();
        for segment in self.segments() {
            match segment {
                Segment::Key(key) => {
                    if !text.is_empty() {
                        text.push('.');
                    }
                    text.push_str(name(*key));
                }
                Segment::Index(index) => text.push_str(&format!("[{index}]")),
            }
        }
        text
    }

    #[must_use]
    pub fn key(&self, key: Key) -> Self {
        self.with(Segment::Key(key))
    }

    #[must_use]
    pub fn index(&self, index: usize) -> Self {
        self.with(Segment::Index(index))
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }

    fn with(&self, segment: Segment) -> Self {
        let mut segments = Vec::with_capacity(self.0.len() + 1);
        segments.extend_from_slice(&self.0);
        segments.push(segment);
        Self(segments)
    }
}

/// a position in the file a reader read, counting lines from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
}

/// what is wrong, with the values a reader needs to word it; `other` is the position
/// of an earlier pool, counting from 0.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// a required key is absent or `null`.
    Missing,
    /// the value is not what the text describes, such as `a list`.
    MustBe(&'static str),
    MustBeIntegerUpTo(u32),
    CpuCountOutOfRange,
    /// a CPU count that would be written as 0, which Docker reads as no limit.
    CpuCountRoundsToZero,
    /// the path is the object that holds a key the schema does not define, which is
    /// named as written.
    UnknownField(String),
    /// the object could not be read into its keys; the text is the deserializer's.
    Unreadable(String),
    NoRepositories,
    LabelShape,
    /// a label the platform always adds.
    PlatformLabel(String),
    /// a platform label of another host, or of another operating system when `os` is
    /// known.
    ForeignPlatformLabel {
        label: String,
        os: Option<Os>,
    },
    DuplicateLabel(String),
    VolumesUnsupported,
    /// a pool whose operating system the host cannot run.
    OsUnsupported(Os),
    /// the file declares a schema version this program does not know.
    NewerSchemaVersion,
    MountPathHasDotDot,
    DuplicateSuffix(String),
    DuplicateMountPath(String),
    NameTooLong {
        length: usize,
        max: usize,
    },
    /// the path is the key that needs `diagnostics` to be on.
    RequiresDiagnostics,
    DotPathComponents,
    InsideCheckout,
    NameCollision {
        name: String,
        other_name: String,
        other: usize,
    },
    TokenFileDiffers {
        other: usize,
    },
    TokenFileReused {
        token_path: String,
        other: usize,
    },
    AmbiguousRouting {
        labels: String,
        other: usize,
    },
    MacosVmsOverLimit {
        total: u32,
        limit: u32,
    },
    CpuBudgetExceeded {
        total_nanos: u128,
        budget_nanos: u128,
    },
    MemoryBudgetExceeded {
        total: u64,
        budget: u32,
    },
    /// `linux_reserve` is the disk any Linux pool adds for the golden VM and Tart's
    /// image cache, and `macos_reserve` the disk any macOS pool adds for the golden
    /// macOS VM and its cached base; `total` includes both, each 0 without such a pool.
    DiskBudgetExceeded {
        total: u64,
        linux_reserve: u64,
        macos_reserve: u64,
        budget: u32,
    },
    /// a Linux pool's disk below the given minimum, the golden VM's disk size.
    LinuxDiskTooSmall(u32),
    /// a macOS pool's value below the given minimum; `reason`, when not empty, says
    /// why after a comma.
    MacosBelowMinimum {
        min: u32,
        reason: &'static str,
    },
    /// diagnostics turned on for a macOS pool, which has none.
    MacosDiagnosticsUnsupported,
}

/// one rule a configuration breaks.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub path: KeyPath,
    pub kind: Kind,
    /// where the reader found the key, when its format has positions.
    pub location: Option<Location>,
}

/// says what a label must look like.
pub const LABEL_SHAPE: &str =
    "a string of letters, digits and . _ : / - starting with a letter or digit";

/// what a format's wording of a rule depends on: the host's name, the keys' names, and
/// where an earlier pool sits.
pub struct Wording<'a> {
    pub host: &'a str,
    pub key_name: fn(Key) -> &'static str,
    pub entry_path: &'a dyn Fn(usize) -> String,
}

/// a problem as `rule_text` hands it back: its text, after its path, when it reads
/// the same in every format apart from the names a `Wording` gives, or the kind for
/// the format to word itself.
pub enum RuleText<'k> {
    Shared(String),
    Own(OwnKind<'k>),
}

/// the kinds each format words itself, with the values the wording needs.
pub enum OwnKind<'k> {
    Missing,
    MustBe(&'static str),
    UnknownField(&'k str),
    Unreadable(&'k str),
    NoRepositories,
    /// a platform label of another host.
    ForeignPlatformLabel(&'k str),
    NewerSchemaVersion,
    NameCollision {
        name: &'k str,
        other_name: &'k str,
        other: usize,
    },
    TokenFileDiffers {
        other: usize,
    },
    TokenFileReused {
        token_path: &'k str,
        other: usize,
    },
}

/// words `kind` when it reads the same in every format, and hands back the rest.
#[must_use]
pub fn rule_text<'k>(kind: &'k Kind, wording: &Wording) -> RuleText<'k> {
    let Wording {
        host,
        key_name,
        entry_path,
    } = wording;
    let per_pool = key_name(Key::Slots);
    let text = match kind {
        Kind::MustBeIntegerUpTo(max) => format!("must be an integer from 1 to {max}"),
        Kind::CpuCountOutOfRange => CPU_RANGE.to_string(),
        Kind::CpuCountRoundsToZero => format!(
            "{CPU_RANGE}, and large enough not to be written as 0, which Docker reads as no limit"
        ),
        Kind::LabelShape => format!("each label must be {LABEL_SHAPE}"),
        Kind::PlatformLabel(label) => {
            format!("'{label}' is always added, so list only non-platform labels")
        }
        Kind::ForeignPlatformLabel {
            label,
            os: Some(os),
        } => format!(
            "'{label}' is a platform label that a {os} pool on {host} cannot provide, so list only non-platform labels"
        ),
        Kind::DuplicateLabel(label) => format!("duplicate label '{label}'"),
        Kind::VolumesUnsupported => {
            format!("not supported on {host}, because VM pools keep no volumes")
        }
        Kind::OsUnsupported(os) => format!("a {os} pool is not supported on {host}"),
        Kind::MountPathHasDotDot => "must not contain '..'".to_string(),
        Kind::DuplicateSuffix(suffix) => format!("duplicate suffix '{suffix}'"),
        Kind::DuplicateMountPath(mount_path) => format!("duplicate mount path '{mount_path}'"),
        Kind::NameTooLong { length, max } => format!(
            "name is {length} characters, at most {max} are allowed because it starts every runner name"
        ),
        Kind::RequiresDiagnostics => format!("requires {}", key_name(Key::Diagnostics)),
        Kind::DotPathComponents => "must not contain dot path components".to_string(),
        Kind::InsideCheckout => "must be outside the checkout".to_string(),
        Kind::AmbiguousRouting { labels, other } => format!(
            "ambiguous same-repository routing; labels '{labels}' also appear at {}",
            entry_path(*other)
        ),
        Kind::MacosVmsOverLimit { total, limit } => format!(
            "macOS pools total {total} VMs (the sum of their {per_pool}), above the limit of {limit}"
        ),
        Kind::CpuBudgetExceeded {
            total_nanos,
            budget_nanos,
        } => format!(
            "pools total {} CPUs (the sum of {per_pool} times {}), above the budget of {}",
            format_nanos(*total_nanos),
            key_name(Key::Cpus),
            format_nanos(*budget_nanos)
        ),
        Kind::MemoryBudgetExceeded { total, budget } => format!(
            "pools total {total} GB of memory (the sum of {per_pool} times {}), above the budget of {budget} GB",
            key_name(Key::MemoryGb)
        ),
        Kind::DiskBudgetExceeded {
            total,
            linux_reserve,
            macos_reserve,
            budget,
        } => format!(
            "pools total {total} GB of disk (the sum of {per_pool} times {}{}), above the budget of {budget} GB",
            key_name(Key::DiskGb),
            disk_reserve(*linux_reserve, *macos_reserve)
        ),
        Kind::LinuxDiskTooSmall(min) => linux_disk_too_small(*min),
        Kind::MacosBelowMinimum { min, reason } => {
            format!("must be at least {min} on a macOS pool{reason}")
        }
        Kind::MacosDiagnosticsUnsupported => {
            "not supported on a macOS pool, because macOS VM pools have no diagnostics".to_string()
        }
        Kind::Missing => return RuleText::Own(OwnKind::Missing),
        Kind::MustBe(what) => return RuleText::Own(OwnKind::MustBe(what)),
        Kind::UnknownField(name) => return RuleText::Own(OwnKind::UnknownField(name)),
        Kind::Unreadable(message) => return RuleText::Own(OwnKind::Unreadable(message)),
        Kind::NoRepositories => return RuleText::Own(OwnKind::NoRepositories),
        Kind::ForeignPlatformLabel { label, os: None } => {
            return RuleText::Own(OwnKind::ForeignPlatformLabel(label));
        }
        Kind::NewerSchemaVersion => return RuleText::Own(OwnKind::NewerSchemaVersion),
        Kind::NameCollision {
            name,
            other_name,
            other,
        } => {
            return RuleText::Own(OwnKind::NameCollision {
                name,
                other_name,
                other: *other,
            });
        }
        Kind::TokenFileDiffers { other } => {
            return RuleText::Own(OwnKind::TokenFileDiffers { other: *other });
        }
        Kind::TokenFileReused { token_path, other } => {
            return RuleText::Own(OwnKind::TokenFileReused {
                token_path,
                other: *other,
            });
        }
    };
    RuleText::Shared(text)
}
