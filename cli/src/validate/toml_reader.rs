//! the TOML reader's side of validation: schema version 1's path form, names and
//! wording, and where the file holds each key. the words are the file's own: its key
//! names, and Windows or macOS rather than a profile's name.

use crate::json::Object;
use crate::profile::Profile;

use super::limits::{CPU_RANGE, format_nanos};
use super::problem::{Key, KeyPath, Kind, Location, Problem, Segment};
use super::reader::Reader;
use super::toml_document::{Layout, Spot};
use super::toml_schema::{Table, key_name};

/// reads a host configuration written as TOML for one host.
pub(super) struct TomlReader<'a> {
    profile: Profile,
    root: &'a Spot,
}

impl<'a> TomlReader<'a> {
    pub(super) fn new(profile: Profile, layout: &'a Layout) -> Self {
        Self {
            profile,
            root: &layout.root,
        }
    }

    fn host(&self) -> &'static str {
        match self.profile {
            Profile::Axpc => "Windows",
            Profile::Axmc => "macOS",
        }
    }

    /// the place the segments name, and whether all of them were found; when not, the
    /// deepest place that was.
    fn find(&self, segments: &[Segment]) -> (&'a Spot, bool) {
        let mut spot = self.root;
        for segment in segments {
            let next = match segment {
                Segment::Key(key) => spot.member(key_name(*key)),
                Segment::Index(index) => spot.items.get(*index),
            };
            match next {
                Some(next) => spot = next,
                None => return (spot, false),
            }
        }
        (spot, true)
    }
}

impl Reader for TomlReader<'_> {
    fn routing_label(&self) -> Option<&str> {
        None
    }

    fn may_omit_os_and_volumes(&self) -> bool {
        true
    }

    fn windows_pool_has_os(&self) -> bool {
        true
    }

    fn joins_owner_and_repository(&self) -> bool {
        true
    }

    fn applies_token_path_rules(&self) -> bool {
        false
    }

    fn entry_path(&self, index: usize) -> String {
        path_text(&KeyPath::root().key(Key::Repositories).index(index))
    }

    fn undefined_keys(&self, _node: &Object, path: &KeyPath) -> Vec<String> {
        let Some(table) = table_at(path) else {
            return Vec::new();
        };
        let (spot, found) = self.find(path.segments());
        if !found {
            return Vec::new();
        }
        spot.member_names()
            .filter(|name| table.defined(name, self.profile).is_none())
            .map(str::to_string)
            .collect()
    }

    fn locate(&self, path: &KeyPath, kind: &Kind) -> Option<Location> {
        let segments = path.segments();
        let (target, member) = match kind {
            Kind::Missing => (
                segments.split_last().map_or(segments, |(_, parent)| parent),
                None,
            ),
            // a clash between pools is about the pool, not the key it names
            Kind::NameCollision { .. }
            | Kind::AmbiguousRouting { .. }
            | Kind::TokenFileDiffers { .. }
            | Kind::TokenFileReused { .. } => (segments.get(..2).unwrap_or(segments), None),
            Kind::UnknownField(name) => (segments, Some(name)),
            _ => (segments, None),
        };
        let (spot, _) = self.find(target);
        let spot = member.and_then(|name| spot.member(name)).unwrap_or(spot);
        Some(Location { line: spot.line })
    }

    fn render(&self, problem: &Problem) -> String {
        let line = problem.location.map_or(1, |location| location.line);
        let path = match &problem.kind {
            Kind::UnknownField(name) => {
                let parent = path_text(&problem.path);
                if parent.is_empty() {
                    name.clone()
                } else {
                    format!("{parent}.{name}")
                }
            }
            _ => path_text(&problem.path),
        };
        format!("line {line}: {path}: {}", self.message(&problem.kind))
    }
}

impl TomlReader<'_> {
    fn message(&self, kind: &Kind) -> String {
        let host = self.host();
        let runners = key_name(Key::Slots);
        match kind {
            Kind::Missing => "required key is missing".to_string(),
            Kind::MustBe(what) => format!("must be {}", expectation(what)),
            Kind::MustBeIntegerUpTo(max) => format!("must be an integer from 1 to {max}"),
            Kind::CpuCountOutOfRange => CPU_RANGE.to_string(),
            Kind::CpuCountRoundsToZero => format!(
                "{CPU_RANGE}, and large enough not to be written as 0, which Docker reads as no limit"
            ),
            Kind::UnknownField(_) => "unknown key".to_string(),
            Kind::Unreadable(_) => "could not be read".to_string(),
            Kind::NoRepositories => "must list at least one pool".to_string(),
            Kind::LabelShape => "each label must be a string of letters, digits and . _ : / - starting with a letter or digit".to_string(),
            Kind::PlatformLabel(label) => {
                format!("'{label}' is always added, so list only non-platform labels")
            }
            Kind::ForeignPlatformLabel {
                label,
                os: Some(os),
            } => format!(
                "'{label}' is a platform label that a {os} pool on {host} cannot provide, so list only non-platform labels"
            ),
            Kind::ForeignPlatformLabel { label, os: None } => {
                format!("'{label}' is a platform label, so use a label that is not one")
            }
            Kind::DuplicateLabel(label) => format!("duplicate label '{label}'"),
            Kind::VolumesUnsupported => {
                format!("not supported on {host}, because VM pools keep no volumes")
            }
            Kind::OsUnsupported(os) => format!("a {os} pool is not supported on {host}"),
            Kind::NewerSchemaVersion => "this file was written by a newer Marshal".to_string(),
            Kind::MountPathHasDotDot => "must not contain '..'".to_string(),
            Kind::DuplicateSuffix(suffix) => format!("duplicate suffix '{suffix}'"),
            Kind::DuplicateMountPath(mount_path) => {
                format!("duplicate mount path '{mount_path}'")
            }
            Kind::NameTooLong { length, max } => format!(
                "name is {length} characters, at most {max} are allowed because it starts every runner name"
            ),
            Kind::RequiresDiagnostics => format!("requires {}", key_name(Key::Diagnostics)),
            Kind::DotPathComponents => "must not contain dot path components".to_string(),
            Kind::InsideCheckout => "must be outside the checkout".to_string(),
            Kind::NameCollision {
                name,
                other_name,
                other,
            } => format!(
                "name '{name}' collides with '{other_name}' at {}; set a distinct name on one pool",
                self.entry_path(*other)
            ),
            Kind::TokenFileDiffers { .. } | Kind::TokenFileReused { .. } => {
                "pools are tied to a token file, which this format does not have".to_string()
            }
            Kind::AmbiguousRouting { labels, other } => format!(
                "ambiguous same-repository routing; labels '{labels}' also appear at {}",
                self.entry_path(*other)
            ),
            Kind::MacosVmsOverLimit { total, limit } => format!(
                "macOS pools total {total} VMs (the sum of their {runners}), above the limit of {limit}"
            ),
            Kind::CpuBudgetExceeded {
                total_nanos,
                budget_nanos,
            } => format!(
                "pools total {} CPUs (the sum of {runners} times {}), above the budget of {}",
                format_nanos(*total_nanos),
                key_name(Key::Cpus),
                format_nanos(*budget_nanos)
            ),
            Kind::MemoryBudgetExceeded { total, budget } => format!(
                "pools total {total} GB of memory (the sum of {runners} times {}), above the budget of {budget} GB",
                key_name(Key::MemoryGb)
            ),
            Kind::DiskBudgetExceeded { total, budget } => format!(
                "pools total {total} GB of disk (the sum of {runners} times {}), above the budget of {budget} GB",
                key_name(Key::DiskGb)
            ),
        }
    }
}

/// says what the rules' expectation is in TOML's words: a list is an array, and an
/// object is a table.
fn expectation(what: &str) -> &str {
    match what {
        "a list" => "an array of tables",
        "a list of labels" => "an array of labels",
        "an object" => "a table",
        other => other,
    }
}

/// the table the object at `path` is.
fn table_at(path: &KeyPath) -> Option<Table> {
    match path.segments() {
        [] => Some(Table::Root),
        [Segment::Key(Key::Repositories), Segment::Index(_)] => Some(Table::Pool),
        [Segment::Key(Key::Budget)] => Some(Table::Budget),
        [Segment::Key(Key::Lifecycle)] => Some(Table::Lifecycle),
        [
            Segment::Key(Key::Repositories),
            Segment::Index(_),
            Segment::Key(Key::Volumes),
            Segment::Index(_),
        ] => Some(Table::Volume),
        _ => None,
    }
}

/// the path as `pool[0].labels`; the root has none.
fn path_text(path: &KeyPath) -> String {
    let mut text = String::new();
    for segment in path.segments() {
        match segment {
            Segment::Key(key) => {
                if !text.is_empty() {
                    text.push('.');
                }
                text.push_str(key_name(*key));
            }
            Segment::Index(index) => text.push_str(&format!("[{index}]")),
        }
    }
    text
}
