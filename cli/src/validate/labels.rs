//! a pool's registration labels.

use std::collections::HashSet;

use crate::fold;
use crate::json::Value;
use crate::profile::{Os, Profile};

use super::fields::{Errors, Field};
use super::patterns;
use super::problem::{Key, KeyPath, Kind};

/// returns the registration labels: the platform labels followed by the entry's own,
/// or the platform labels and the routing label when the field is absent or empty;
/// `None` after recording why a present list is unusable. a file whose routing label
/// is unusable gives none, so its pools carry only the platform labels.
pub fn registration_labels(
    field: &Field,
    entry_path: &KeyPath,
    routing_label: Option<&str>,
    profile: Profile,
    os: Option<Os>,
    errors: &mut Errors,
) -> Option<Vec<String>> {
    let path = entry_path.key(Key::Labels);
    let platform = profile.platform_labels(os);
    let defaults = || -> Vec<String> {
        platform
            .iter()
            .copied()
            .chain(routing_label)
            .map(str::to_string)
            .collect()
    };
    let Some(value) = field.get() else {
        return Some(defaults());
    };
    let Value::Array(items) = value else {
        errors.add(path, Kind::MustBe("a list of labels"));
        return None;
    };
    if items.is_empty() {
        return Some(defaults());
    }

    let mut valid = true;
    let mut seen = HashSet::new();
    let mut labels = Vec::new();
    for item in items {
        match item {
            Value::String(label) if patterns::is_label(label) => {
                if let Some(problem) = platform_problem(label, profile, os) {
                    errors.add(path.clone(), problem);
                    valid = false;
                } else if seen.contains(&fold::lowercase(label)) {
                    errors.add(path.clone(), Kind::DuplicateLabel(label.clone()));
                    valid = false;
                } else {
                    labels.push(label.clone());
                }
            }
            _ => {
                errors.add(path.clone(), Kind::LabelShape);
                valid = false;
            }
        }
        // a string joins the duplicate check whether or not it was accepted
        if let Value::String(label) = item {
            seen.insert(fold::lowercase(label));
        }
    }
    valid.then(|| {
        platform
            .iter()
            .map(|label| (*label).to_string())
            .chain(labels)
            .collect()
    })
}

/// says why a label that matches the label pattern is a platform label the pool
/// must not list, or `None` when it is an ordinary label.
fn platform_problem(label: &str, profile: Profile, os: Option<Os>) -> Option<Kind> {
    let is = |candidates: &[&str]| {
        candidates
            .iter()
            .any(|candidate| fold::eq_ascii(candidate, label))
    };
    if is(profile.platform_labels(os)) {
        return Some(Kind::PlatformLabel(label.to_string()));
    }
    if !is(profile.foreign_platform_labels()) {
        return None;
    }
    Some(Kind::ForeignPlatformLabel {
        label: label.to_string(),
        os,
    })
}
