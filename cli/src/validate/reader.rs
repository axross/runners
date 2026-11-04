//! what a configuration format supplies to the shared rule set.

use super::problem::{KeyPath, Kind, Location, Problem};

/// the format-specific half of validation: the few rules that differ by format, and
/// how a problem reads in the format's own path form and key names. the rule set reads
/// a pool's owner and repository as two keys; a format with one key splits it and
/// names both under that key when it renders.
pub trait Reader {
    /// the label a pool without labels carries besides the platform's.
    fn routing_label(&self) -> &str;

    /// whether a pool may leave out `os` and `volumes`, taking Linux and none.
    fn may_omit_os_and_volumes(&self) -> bool;

    /// whether a pool has a token file, which the rules then require and compare.
    fn applies_token_path_rules(&self) -> bool;

    /// where the `index`th pool sits in the file, in the format's path form.
    fn entry_path(&self, index: usize) -> String;

    /// where the file holds the key a problem is about, when the format has positions.
    fn locate(&self, path: &KeyPath, kind: &Kind) -> Option<Location>;

    /// the problem as one line of text, without a leading hyphen.
    fn render(&self, problem: &Problem) -> String;
}
