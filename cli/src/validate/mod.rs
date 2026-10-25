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
pub(crate) mod patterns;
mod plan;
mod uniqueness;
mod volumes;

use std::path::Path;

use serde::Deserialize;

use crate::json::{self, Value};
use crate::profile::Profile;

pub use budget::{MAX_MACOS_VMS, Usage, usage};
pub use limits::{format_cpu_count, format_nanos};
pub use paths::is_inside_checkout;
pub use plan::{Budget, Entry, Plan, PlannedVolume};

use fields::{Errors, Field};

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

/// The members of the configuration's root, named for both profiles.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RootFields {
    #[serde(default)]
    image_name: Field,
    #[serde(default)]
    repositories: Field,
    #[serde(default)]
    budget: Field,
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
    let profile = cx.profile;
    let root = fields::read_object::<RootFields>(
        root,
        "configuration",
        profile.root_fields(),
        &mut cx.errors,
    )?;

    let image_name = read_image_name(&root.image_name, cx);
    let budget = match profile {
        Profile::Axpc => None,
        Profile::Axmc => budget::read_budget(&root.budget, &mut cx.errors),
    };
    let entries = read_repositories(&root.repositories, cx);
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

/// Reads the image name: required on axpc, optional on axmc, where its use is
/// undecided. A present value follows the same rule on both.
fn read_image_name(image_name: &Field, cx: &mut Context) -> Option<String> {
    let expectation = "a local Docker image name with a tag, such as name:tag";
    if cx.profile == Profile::Axmc {
        return match image_name.get() {
            None => None,
            Some(Value::String(name)) if patterns::is_image_name(name) => Some(name.clone()),
            Some(_) => {
                cx.errors.add(format!("imageName: must be {expectation}"));
                None
            }
        };
    }
    fields::string(
        image_name,
        "imageName",
        &mut cx.errors,
        patterns::is_image_name,
        expectation,
    )
    .map(str::to_string)
}

fn read_repositories(repositories: &Field, cx: &mut Context) -> Vec<Entry> {
    let mut entries = Vec::new();
    let Some(value) = fields::required(repositories, "repositories", &mut cx.errors) else {
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

#[cfg(test)]
mod tests {
    use serde::de::{self, Visitor};

    use crate::de::{Deserializer, Error};

    use super::*;

    /// Records the field names a derived type asks its deserializer for.
    struct Probe<'a>(&'a mut Vec<String>);

    impl<'de> de::Deserializer<'de> for Probe<'_> {
        type Error = Error;

        fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
            Err(de::Error::custom("only the field names are wanted"))
        }

        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _name: &'static str,
            fields: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Error> {
            self.0
                .extend(fields.iter().map(|field| (*field).to_string()));
            Err(de::Error::custom("only the field names are wanted"))
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map enum identifier ignored_any
        }
    }

    fn field_names<T: for<'de> Deserialize<'de>>() -> Vec<String> {
        let mut names = Vec::new();
        let _ = T::deserialize(Probe(&mut names));
        names.sort();
        names
    }

    fn union(first: &[&str], second: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = first
            .iter()
            .chain(second)
            .map(|n| (*n).to_string())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn each_derived_type_names_the_union_of_both_profiles_fields() {
        assert_eq!(
            field_names::<RootFields>(),
            union(Profile::Axpc.root_fields(), Profile::Axmc.root_fields())
        );
        assert_eq!(
            field_names::<entry::EntryFields>(),
            union(Profile::Axpc.entry_fields(), Profile::Axmc.entry_fields())
        );
        assert_eq!(
            field_names::<budget::BudgetFields>(),
            union(&budget::BUDGET_FIELDS, &[])
        );
        assert_eq!(
            field_names::<volumes::VolumeFields>(),
            union(&volumes::VOLUME_FIELDS, &[])
        );
    }

    fn read<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Error> {
        let tree = json::parse(text.as_bytes()).unwrap();
        T::deserialize(Deserializer::new(&tree).ignoring_field_case())
    }

    /// Reads an object with every member set to `value`, then an empty object.
    fn reads_any_value<T: for<'de> Deserialize<'de>>() {
        for value in ["1", "[]", "{}"] {
            let members: Vec<_> = field_names::<T>()
                .iter()
                .map(|name| format!("\"{name}\": {value}"))
                .collect();
            let text = format!("{{{}}}", members.join(", "));
            assert!(read::<T>(&text).is_ok(), "{text}");
        }
        assert!(read::<T>("{}").is_ok());
    }

    #[test]
    fn every_derived_field_is_a_wrapper_that_reads_any_value() {
        reads_any_value::<RootFields>();
        reads_any_value::<entry::EntryFields>();
        reads_any_value::<budget::BudgetFields>();
        reads_any_value::<volumes::VolumeFields>();
    }

    #[test]
    fn a_wrapper_tells_a_missing_member_from_a_null() {
        let missing = read::<RootFields>("{}").unwrap();
        assert!(missing.image_name.get().is_none());
        let null = read::<RootFields>(r#"{"imageName": null}"#).unwrap();
        assert!(matches!(null.image_name.get(), Some(Value::Null)));
    }
}
