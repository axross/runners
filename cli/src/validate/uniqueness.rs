//! The checks across entries: names, token files and routing labels.

use crate::fold;
use crate::profile::Profile;

use super::fields::Errors;
use super::plan::Entry;

fn names_collide(first: &str, second: &str) -> bool {
    first == second
        || first.starts_with(&format!("{second}-"))
        || second.starts_with(&format!("{first}-"))
}

fn same_repository(first: &Entry, second: &Entry) -> bool {
    fold::eq_ascii(
        &format!("{}/{}", first.owner, first.repository),
        &format!("{}/{}", second.owner, second.repository),
    )
}

/// Records every collision between a later entry and an earlier one, in the order
/// the entries appear. Token files compare under simple case folding, as Windows
/// file names ignore case. Platform labels cannot tell two pools apart, so only a
/// shared non-platform label makes same-repository routing ambiguous, and pools
/// whose OS differs can share one because their platform labels differ.
pub fn check_entries(entries: &[Entry], profile: Profile, errors: &mut Errors) {
    for (later, b) in entries.iter().enumerate().skip(1) {
        for a in entries.iter().take(later) {
            if names_collide(&a.name, &b.name) {
                errors.add(format!(
                    "{}.name: name '{}' collides with '{}' at {}; set a distinct name on one entry",
                    b.path, b.name, a.name, a.path
                ));
            }
            if same_repository(a, b) {
                if !fold::eq_folded(&a.token_path, &b.token_path) {
                    errors.add(format!(
                        "{}.tokenPath: pools for the same repository must use the same token file as {}",
                        b.path, a.path
                    ));
                }
                let platform = profile.platform_labels(b.os);
                let overlap: Vec<&str> = if a.os == b.os {
                    b.labels
                        .iter()
                        .filter(|label| !platform.iter().any(|known| fold::eq_ascii(known, label)))
                        .filter(|label| a.labels.iter().any(|known| fold::eq_ascii(known, label)))
                        .map(String::as_str)
                        .collect()
                } else {
                    Vec::new()
                };
                if !overlap.is_empty() {
                    errors.add(format!(
                        "{}.labels: ambiguous same-repository routing; labels '{}' also appear at {}",
                        b.path,
                        overlap.join(", "),
                        a.path
                    ));
                }
            } else if fold::eq_folded(&a.token_path, &b.token_path) {
                errors.add(format!(
                    "{}.tokenPath: token file '{}' is already used at {}; each repository needs its own token",
                    b.path, b.token_path, a.path
                ));
            }
        }
    }
}
