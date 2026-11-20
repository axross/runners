//! the checks across entries: names, token files and routing labels.

use crate::fold;
use crate::profile::Profile;

use super::fields::Errors;
use super::plan::{Entry, PoolTarget};
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
    owner: String,
    repository: Option<String>,
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
            owner: entry.target.owner().to_ascii_lowercase(),
            repository: match &entry.target {
                PoolTarget::Repository { repository, .. } => Some(repository.to_ascii_lowercase()),
                PoolTarget::Organization { .. } => None,
            },
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

impl Keys<'_> {
    /// whether both entries are for one repository, which takes the same owner and
    /// the same repository name; an organisation pool has no repository.
    fn same_repository(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.repository.is_some()
            && self.repository == other.repository
    }
}

/// records every collision between a later entry and an earlier one, in entry order;
/// token files compare under simple case folding, as Windows file names ignore case,
/// and pools whose OS differs can share a non-platform label, as their platform
/// labels differ.
pub fn check_entries(
    entries: &[Entry],
    profile: Profile,
    routing_label: Option<&str>,
    errors: &mut Errors,
) {
    let keys: Vec<Keys> = entries
        .iter()
        .map(|entry| Keys::new(entry, profile))
        .collect();
    for (later, b) in keys.iter().enumerate().skip(1) {
        let path = KeyPath::root().key(Key::Repositories).index(b.entry.index);
        for a in keys.iter().take(later) {
            let other = a.entry.index;
            let same_repository = a.same_repository(b);
            for (own, peer) in [(b, a), (a, b)] {
                if let Some(scale_set) = &own.entry.scale_set {
                    let name = scale_set.to_ascii_lowercase();
                    let collision =
                        names_collide(&name, &peer.entry.name.to_ascii_lowercase())
                            || peer.entry.scale_set.as_ref().is_some_and(|other| {
                                names_collide(&name, &other.to_ascii_lowercase())
                            })
                            || (same_repository
                                && peer
                                    .entry
                                    .labels
                                    .iter()
                                    .any(|label| fold::eq_ascii(scale_set, label)));
                    if collision {
                        errors.add(
                            KeyPath::root()
                                .key(Key::Repositories)
                                .index(own.entry.index)
                                .key(Key::ScaleSet),
                            Kind::ScaleSetCollision {
                                other: peer.entry.index,
                            },
                        );
                    }
                }
            }
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
            if same_repository {
                if a.token_path != b.token_path {
                    errors.add(path.key(Key::TokenPath), Kind::TokenFileDiffers { other });
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
            let organization_overlap =
                a.owner == b.owner && (a.repository.is_none() || b.repository.is_none());
            // the scale-set collision checks above cover a same-repository pair with an
            // on-demand pool, whose only label is its scale-set name.
            let on_demand = a.entry.scale_set.is_some() || b.entry.scale_set.is_some();
            if a.entry.os == b.entry.os && ((same_repository && !on_demand) || organization_overlap)
            {
                let overlap: Vec<&str> = b
                    .routing_labels
                    .iter()
                    .filter(|(_, label)| a.routing_labels.iter().any(|(_, known)| known == label))
                    .map(|(label, _)| *label)
                    .collect();
                if !overlap.is_empty() {
                    let labels = overlap.join(", ");
                    let kind = if organization_overlap {
                        let routing_label_shared = routing_label.is_some_and(|routing_label| {
                            overlap
                                .iter()
                                .any(|label| fold::eq_ascii(label, routing_label))
                        });
                        Kind::OrganizationRoutingOverlap {
                            labels,
                            other,
                            routing_label_shared,
                        }
                    } else {
                        Kind::AmbiguousRouting { labels, other }
                    };
                    errors.add(path.key(Key::Labels), kind);
                }
            }
        }
    }
}
