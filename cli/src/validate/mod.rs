//! Host configuration validation.
//!
//! [`validate`] is a pure function from configuration bytes and a profile to a plan
//! or an ordered list of errors. It reads nothing but its arguments, so a token
//! file's existence and permissions are not its concern. Every error is collected,
//! in the order the PowerShell validator reports it, with its message text.

mod budget;
mod entry;
mod fields;
mod labels;
mod limits;
mod paths;
mod patterns;
mod plan;
mod uniqueness;
mod volumes;

use std::path::Path;

use crate::json::{self, Object, Value};
use crate::profile::Profile;

pub use budget::{MAX_MACOS_VMS, Usage, usage};
pub use limits::{format_cpu_count, format_nanos};
pub use plan::{Budget, Entry, Plan, PlannedVolume};

use fields::Errors;

/// Why a configuration was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The file is not a JSON document the validator can read.
    Unreadable(String),
    /// The document breaks rules, listed in the order they were found.
    Invalid(Vec<String>),
}

/// The outcome of validating one configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub outcome: Result<Plan, Failure>,
    /// True when an entry's `diagnosticDirectory` could not be compared with a
    /// checkout because none was given.
    pub checkout_rule_skipped: bool,
}

struct Context<'a> {
    profile: Profile,
    checkout: Option<&'a Path>,
    errors: Errors,
    checkout_rule_skipped: bool,
}

/// Validates a configuration against a profile. `checkout` is the project checkout
/// that private directories must lie outside, when one is known.
#[must_use]
pub fn validate(bytes: &[u8], profile: Profile, checkout: Option<&Path>) -> Validation {
    let root = match json::parse(bytes) {
        Ok(root) => root,
        Err(error) => {
            return Validation {
                outcome: Err(Failure::Unreadable(error.to_string())),
                checkout_rule_skipped: false,
            };
        }
    };
    let mut cx = Context {
        profile,
        checkout,
        errors: Errors::default(),
        checkout_rule_skipped: false,
    };
    let outcome = match read_root(&root, &mut cx) {
        Some(plan) => Ok(plan),
        None => Err(Failure::Invalid(cx.errors.into_vec())),
    };
    Validation {
        outcome,
        checkout_rule_skipped: cx.checkout_rule_skipped,
    }
}

fn read_root(root: &Value, cx: &mut Context) -> Option<Plan> {
    let root = fields::object(root, "configuration", &mut cx.errors)?;
    let profile = cx.profile;
    fields::unknown_fields(root, profile.root_fields(), "configuration", &mut cx.errors);

    let image_name = read_image_name(root, cx);
    let budget = match profile {
        Profile::Axpc => None,
        Profile::Axmc => budget::read_budget(root, &mut cx.errors),
    };
    let entries = read_repositories(root, cx);
    if profile == Profile::Axmc {
        budget::check_budget(&entries, budget.as_ref(), &mut cx.errors);
    }
    if !cx.errors.is_empty() {
        return None;
    }
    Some(Plan {
        profile,
        image_name,
        entries,
        budget,
    })
}

/// Reads the image name: required on axpc, optional on axmc, where Phase 6 decides
/// its use. A present value follows the same rule on both.
fn read_image_name(root: &Object, cx: &mut Context) -> Option<String> {
    let expectation = "a local Docker image name with a tag, such as name:tag";
    if cx.profile == Profile::Axmc {
        return match root.get("imageName") {
            None => None,
            Some(Value::String(name)) if patterns::is_image_name(name) => Some(name.clone()),
            Some(_) => {
                cx.errors.add(format!("imageName: must be {expectation}"));
                None
            }
        };
    }
    fields::string(
        root,
        "imageName",
        "imageName",
        &mut cx.errors,
        patterns::is_image_name,
        expectation,
    )
    .map(str::to_string)
}

fn read_repositories(root: &Object, cx: &mut Context) -> Vec<Entry> {
    let mut entries = Vec::new();
    let Some(value) = fields::required(root, "repositories", "repositories", &mut cx.errors) else {
        return entries;
    };
    let Value::Array(nodes) = value else {
        cx.errors.add("repositories: must be a list");
        return entries;
    };
    if nodes.is_empty() {
        cx.errors
            .add("repositories: must list at least one repository");
    }
    for (index, node) in nodes.iter().enumerate() {
        if let Some(entry) = entry::read_entry(node, &format!("repositories[{index}]"), cx) {
            entries.push(entry);
        }
    }
    if entries.len() > 1 {
        uniqueness::check_entries(&entries, cx.profile, &mut cx.errors);
    }
    entries
}
