//! the TOML reader's side of validation: schema version 1's path form, names and
//! wording, and where the file holds each key. the words are the file's own: its key
//! names, and Windows or macOS rather than a profile's name.

use crate::json::Object;
use crate::profile::Profile;

use super::problem::{
    Key, KeyPath, Kind, Location, OwnKind, Problem, RuleText, Segment, Wording, rule_text,
};
use super::reader::{Format, Reader};
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
        find(self.root, segments)
    }
}

fn find<'a>(root: &'a Spot, segments: &[Segment]) -> (&'a Spot, bool) {
    let mut spot = root;
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

/// the line, counted from 1, where the TOML bytes hold the key the path names; `None`
/// when they are not a TOML document or do not hold it.
pub(crate) fn key_line(bytes: &[u8], path: &KeyPath) -> Option<usize> {
    let layout = super::toml_document::read(bytes).ok()?;
    match find(&layout.root, path.segments()) {
        (spot, true) => Some(spot.line),
        _ => None,
    }
}

impl Reader for TomlReader<'_> {
    fn format(&self) -> Format<'_> {
        Format::Toml
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
        let entry_path = |index| self.entry_path(index);
        let wording = Wording {
            host: self.host(),
            key_name,
            entry_path: &entry_path,
        };
        let own = match rule_text(kind, &wording) {
            RuleText::Shared(text) => return text,
            RuleText::Own(own) => own,
        };
        match own {
            OwnKind::Missing => "required key is missing".to_string(),
            OwnKind::MustBe(what) => format!("must be {}", expectation(what)),
            OwnKind::UnknownField(_) => "unknown key".to_string(),
            OwnKind::Unreadable(_) => "could not be read".to_string(),
            OwnKind::NoRepositories => "must list at least one pool".to_string(),
            OwnKind::ForeignPlatformLabel(label) => {
                format!("'{label}' is a platform label, so use a label that is not one")
            }
            OwnKind::NewerSchemaVersion => "this file was written by a newer Marshal".to_string(),
            OwnKind::NameCollision {
                name,
                other_name,
                other,
            } => format!(
                "name '{name}' collides with '{other_name}' at {}; set a distinct name on one pool",
                self.entry_path(other)
            ),
            OwnKind::TokenFileDiffers { .. } | OwnKind::TokenFileReused { .. } => {
                "pools are tied to a token file, which this format does not have".to_string()
            }
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
pub(crate) fn path_text(path: &KeyPath) -> String {
    path.text(key_name)
}
