//! what a configuration format supplies to the shared rule set.

use crate::json::Object;

use super::problem::{KeyPath, Kind, Location, Problem};

/// the rules that differ by format. they change together, so a reader names its format
/// rather than answering each rule on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format<'a> {
    /// the JSON file of earlier releases: the profile fixes the routing label, the
    /// file carries no settings, and a pool names its owner and repository apart and
    /// its token file, and leaves out neither `os` nor `volumes` where its host has
    /// them.
    Json { routing_label: &'a str },
    /// `config.toml`: the file sets its routing label and settings, and a pool may
    /// leave out `os` and `volumes`, names `owner/name` in one key, and has no token
    /// file.
    Toml,
}

impl Format<'_> {
    /// the label a pool without labels carries besides the platform's, when the
    /// format fixes it; `None` when the file sets it and the rules read it.
    pub fn routing_label(&self) -> Option<&str> {
        match self {
            Self::Json { routing_label } => Some(routing_label),
            Self::Toml => None,
        }
    }

    /// whether the file carries its own settings: `schema_version`, `routing_label`,
    /// and `[lifecycle]`, which the rules then read and a plan records.
    pub fn carries_settings(&self) -> bool {
        *self == Self::Toml
    }

    /// whether a pool may leave out `os` and `volumes`, taking Linux and none; a
    /// Windows pool then has an `os`, which only Linux satisfies there.
    pub fn may_omit_os_and_volumes(&self) -> bool {
        *self == Self::Toml
    }

    /// whether a pool names its owner and repository in one `owner/name` key, which
    /// the rules then split and check part by part.
    pub fn joins_owner_and_repository(&self) -> bool {
        *self == Self::Toml
    }

    /// whether a pool has a token file, which the rules then require and compare.
    pub fn has_token_paths(&self) -> bool {
        *self != Self::Toml
    }
}

/// the format-specific half of validation: the rules that differ by format, and how a
/// problem reads in the format's own path form and key names. the rule set reads a
/// pool's owner and repository as two keys unless the format joins them; a format
/// with one key names both under that key when it renders.
pub trait Reader {
    /// the format the reader reads.
    fn format(&self) -> Format<'_>;

    /// where the `index`th pool sits in the file, in the format's path form.
    fn entry_path(&self, index: usize) -> String;

    /// the names of the members of the object at `path` that the format does not
    /// define for its host, in document order.
    fn undefined_keys(&self, node: &Object, path: &KeyPath) -> Vec<String>;

    /// where the file holds the key a problem is about, when the format has positions.
    fn locate(&self, path: &KeyPath, kind: &Kind) -> Option<Location>;

    /// the problem as one line of text, without a leading hyphen.
    fn render(&self, problem: &Problem) -> String;
}
