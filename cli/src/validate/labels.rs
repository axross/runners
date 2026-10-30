//! a pool's registration labels.

use std::collections::HashSet;

use crate::fold;
use crate::json::Value;
use crate::profile::{Os, Profile};

use super::fields::{Errors, Field};
use super::patterns;

/// returns the registration labels: the platform labels followed by the entry's own,
/// or the platform labels and the routing label when the field is absent or empty;
/// `None` after recording why a present list is unusable.
pub fn registration_labels(
    field: &Field,
    path: &str,
    profile: Profile,
    os: Option<Os>,
    errors: &mut Errors,
) -> Option<Vec<String>> {
    let platform = profile.platform_labels(os);
    let defaults = || -> Vec<String> {
        platform
            .iter()
            .copied()
            .chain([profile.routing_label()])
            .map(str::to_string)
            .collect()
    };
    let Some(value) = field.get() else {
        return Some(defaults());
    };
    let Value::Array(items) = value else {
        errors.add(format!("{path}.labels: must be a list of labels"));
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
                    errors.add(format!("{path}.labels: {problem}"));
                    valid = false;
                } else if seen.contains(&fold::lowercase(label)) {
                    errors.add(format!("{path}.labels: duplicate label '{label}'"));
                    valid = false;
                } else {
                    labels.push(label.clone());
                }
            }
            _ => {
                errors.add(format!(
                    "{path}.labels: each label must be a string of letters, digits and . _ : / - starting with a letter or digit"
                ));
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
fn platform_problem(label: &str, profile: Profile, os: Option<Os>) -> Option<String> {
    let is = |candidates: &[&str]| {
        candidates
            .iter()
            .any(|candidate| fold::eq_ascii(candidate, label))
    };
    if is(profile.platform_labels(os)) {
        return Some(format!(
            "'{label}' is always added, so list only non-platform labels"
        ));
    }
    if !is(profile.foreign_platform_labels()) {
        return None;
    }
    Some(match os {
        Some(os) => format!(
            "'{label}' is a platform label that a {os} pool on {profile} cannot provide, so list only non-platform labels"
        ),
        None => format!("'{label}' is a platform label, so list only non-platform labels"),
    })
}
