//! The checks across entries: names, token files and routing labels.

use crate::fold;
use crate::profile::Profile;

use super::fields::Errors;
use super::plan::Entry;

fn names_collide(first: &str, second: &str) -> bool {
    first == second || extends(first, second) || extends(second, first)
}

/// Reports whether `name` is `prefix`, a `-`, and more.
fn extends(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('-'))
}

/// An entry with the values the pairwise checks compare, prepared once so that no
/// pair allocates.
struct Keys<'a> {
    entry: &'a Entry,
    repository: String,
    token_path: String,
    /// The labels that are not platform labels, each with its ASCII lowercase form.
    routing_labels: Vec<(&'a str, String)>,
}

impl<'a> Keys<'a> {
    fn new(entry: &'a Entry, profile: Profile) -> Self {
        let platform = profile.platform_labels(entry.os);
        Self {
            entry,
            repository: format!("{}/{}", entry.owner, entry.repository).to_ascii_lowercase(),
            token_path: fold::folded(&entry.token_path),
            routing_labels: entry
                .labels
                .iter()
                .filter(|label| !platform.iter().any(|known| fold::eq_ascii(known, label)))
                .map(|label| (label.as_str(), label.to_ascii_lowercase()))
                .collect(),
        }
    }
}

/// Records every collision between a later entry and an earlier one, in the order
/// the entries appear. Token files compare under simple case folding, as Windows
/// file names ignore case. Platform labels cannot tell two pools apart, so only a
/// shared non-platform label makes same-repository routing ambiguous, and pools
/// whose OS differs can share one because their platform labels differ.
pub fn check_entries(entries: &[Entry], profile: Profile, errors: &mut Errors) {
    let keys: Vec<Keys> = entries
        .iter()
        .map(|entry| Keys::new(entry, profile))
        .collect();
    for (later, b) in keys.iter().enumerate().skip(1) {
        for a in keys.iter().take(later) {
            if names_collide(&a.entry.name, &b.entry.name) {
                errors.add(format!(
                    "{}.name: name '{}' collides with '{}' at {}; set a distinct name on one entry",
                    b.entry.path, b.entry.name, a.entry.name, a.entry.path
                ));
            }
            if a.repository == b.repository {
                if a.token_path != b.token_path {
                    errors.add(format!(
                        "{}.tokenPath: pools for the same repository must use the same token file as {}",
                        b.entry.path, a.entry.path
                    ));
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
                    errors.add(format!(
                        "{}.labels: ambiguous same-repository routing; labels '{}' also appear at {}",
                        b.entry.path,
                        overlap.join(", "),
                        a.entry.path
                    ));
                }
            } else if a.token_path == b.token_path {
                errors.add(format!(
                    "{}.tokenPath: token file '{}' is already used at {}; each repository needs its own token",
                    b.entry.path, b.entry.token_path, a.entry.path
                ));
            }
        }
    }
}
