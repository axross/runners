//! the checks across entries: names, token files and routing labels.

use crate::fold;
use crate::profile::Profile;

use super::fields::Errors;
use super::plan::Entry;
use super::problem::{Key, KeyPath, Kind};

fn names_collide(first: &str, second: &str) -> bool {
    first == second || extends(first, second) || extends(second, first)
}

fn extends(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('-'))
}

/// an entry with the values the pairwise checks compare, prepared once so that a
/// pair without a collision allocates nothing.
struct Keys<'a> {
    entry: &'a Entry,
    repository: String,
    token_path: Option<String>,
    /// the labels that are not platform labels of this entry's own OS, each with its
    /// ASCII lowercase form; two entries' lists compare only when their OS is the
    /// same.
    routing_labels: Vec<(&'a str, String)>,
}

impl<'a> Keys<'a> {
    fn new(entry: &'a Entry, profile: Profile) -> Self {
        let platform = profile.platform_labels(entry.os);
        Self {
            entry,
            repository: format!("{}/{}", entry.owner, entry.repository).to_ascii_lowercase(),
            token_path: entry.token_path.as_deref().map(fold::folded),
            routing_labels: entry
                .labels
                .iter()
                .filter(|label| !platform.iter().any(|known| fold::eq_ascii(known, label)))
                .map(|label| (label.as_str(), label.to_ascii_lowercase()))
                .collect(),
        }
    }
}

/// records every collision between a later entry and an earlier one, in entry order;
/// token files compare under simple case folding, as Windows file names ignore case,
/// and pools whose OS differs can share a non-platform label, as their platform
/// labels differ.
pub fn check_entries(entries: &[Entry], profile: Profile, errors: &mut Errors) {
    let keys: Vec<Keys> = entries
        .iter()
        .map(|entry| Keys::new(entry, profile))
        .collect();
    for (later, b) in keys.iter().enumerate().skip(1) {
        let path = KeyPath::root().key(Key::Repositories).index(b.entry.index);
        for a in keys.iter().take(later) {
            let other = a.entry.index;
            if names_collide(&a.entry.name, &b.entry.name) {
                errors.add(
                    path.key(Key::Name),
                    Kind::NameCollision {
                        name: b.entry.name.clone(),
                        other_name: a.entry.name.clone(),
                        other,
                    },
                );
            }
            if a.repository == b.repository {
                if a.token_path != b.token_path {
                    errors.add(path.key(Key::TokenPath), Kind::TokenFileDiffers { other });
                }
                let overlap: Vec<&str> = if a.entry.os == b.entry.os {
                    b.routing_labels
                        .iter()
                        .filter(|(_, label)| {
                            a.routing_labels.iter().any(|(_, known)| known == label)
                        })
                        .map(|(label, _)| *label)
                        .collect()
                } else {
                    Vec::new()
                };
                if !overlap.is_empty() {
                    errors.add(
                        path.key(Key::Labels),
                        Kind::AmbiguousRouting {
                            labels: overlap.join(", "),
                            other,
                        },
                    );
                }
            } else if let Some(token_path) = b
                .entry
                .token_path
                .as_ref()
                .filter(|_| a.token_path == b.token_path)
            {
                errors.add(
                    path.key(Key::TokenPath),
                    Kind::TokenFileReused {
                        token_path: token_path.clone(),
                        other,
                    },
                );
            }
        }
    }
}
