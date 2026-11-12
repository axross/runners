//! what the rule set records when a configuration breaks a rule: the key's path, the
//! kind of problem with its values, and where the reader found the key. the rule set
//! words nothing; a [`Reader`](super::Reader) renders each problem in its format's
//! path form and names.

use crate::profile::Os;

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
    ResumeAfterStop,
}

impl Key {
    pub const ALL: [Self; 26] = [
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
    /// `reserve` is the disk any Linux pool adds for the golden VM and Tart's image
    /// cache, which `total` includes; 0 when no pool is a Linux one.
    DiskBudgetExceeded {
        total: u64,
        reserve: u64,
        budget: u32,
    },
    /// a Linux pool's disk below the given minimum, the golden VM's disk size.
    LinuxDiskTooSmall(u32),
}

/// one rule a configuration breaks.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub path: KeyPath,
    pub kind: Kind,
    /// where the reader found the key, when its format has positions.
    pub location: Option<Location>,
}
