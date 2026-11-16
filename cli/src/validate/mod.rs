//! host configuration validation: [`validate`] is a pure function from configuration
//! bytes and a profile to a plan or an ordered list of errors, in the order and under
//! the rules that `docs/operations/cli.md` states, with message text the shared
//! fixtures pin; it reads nothing but its arguments, so a token file is not its
//! concern.

mod budget;
mod entry;
mod fields;
mod json_reader;
mod labels;
mod limits;
mod macos_pool;
mod paths;
pub(crate) mod patterns;
mod plan;
mod problem;
mod reader;
mod settings;
mod toml_document;
mod toml_reader;
mod toml_schema;
mod uniqueness;
mod volumes;

use std::path::Path;

use serde::Deserialize;

use crate::json::{self, Value};
use crate::profile::Profile;

pub use budget::{MAX_MACOS_VMS, Usage, usage};
pub use json_reader::JsonReader;
pub(crate) use json_reader::key_name as json_key_name;
pub use limits::{format_cpu_count, format_nanos};
pub use paths::is_inside_checkout;
pub use plan::{Budget, Entry, Lifecycle, Plan, PlannedVolume, Settings};
pub use problem::{Key, KeyPath, Kind, Location, Problem, Segment};
pub use reader::{Format, Reader};
pub use settings::SCHEMA_VERSION;
use toml_reader::TomlReader;
pub(crate) use toml_reader::key_line as toml_key_line;
pub(crate) use toml_reader::path_text as toml_path_text;
pub(crate) use toml_schema::key_name as toml_key_name;
pub(crate) use toml_schema::parse_path as parse_toml_path;

use fields::{Errors, Field};

/// why a configuration was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// the file is not a JSON document the validator can read.
    Unreadable(String),
    /// the document breaks rules, listed in the order they were found.
    Invalid(Vec<String>),
}

/// the outcome of validating one configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub outcome: Result<Plan, Failure>,
    /// true when an entry's `diagnosticDirectory` could not be compared with a
    /// checkout because none was given.
    pub checkout_rule_skipped: bool,
}

/// the members of the configuration's root, named for both profiles and for a file
/// with settings of its own.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RootFields {
    #[serde(default)]
    routing_label: Field,
    #[serde(default)]
    lifecycle: Field,
    #[serde(default)]
    image_name: Field,
    #[serde(default)]
    repositories: Field,
    #[serde(default)]
    budget: Field,
}

/// where the checkout that a diagnostic directory must lie outside comes from.
#[derive(Debug, Clone, Copy)]
pub enum CheckoutRule<'a> {
    /// the one given, which is the one above the configuration file; none skips the
    /// rule.
    Above(Option<&'a Path>),
    /// the one given and, for each diagnostic directory, the one found at or above that
    /// directory, for a configuration that lives outside any checkout. The rule is
    /// then never skipped: the directory's own place was examined.
    AboveAndAtSink(Option<&'a Path>),
}

impl Default for CheckoutRule<'_> {
    /// no checkout known, so the rule is skipped.
    fn default() -> Self {
        Self::Above(None)
    }
}

struct Context<'a> {
    profile: Profile,
    reader: &'a dyn Reader,
    checkout: Option<&'a Path>,
    at_sink: bool,
    errors: Errors,
    checkout_rule_skipped: bool,
}

/// validates a configuration against a profile; `checkout` is the project checkout
/// that private directories must lie outside, when one is known.
#[must_use]
pub fn validate(bytes: &[u8], profile: Profile, checkout: Option<&Path>) -> Validation {
    validate_with(bytes, profile, CheckoutRule::Above(checkout))
}

/// validates a JSON configuration against a profile under a [`CheckoutRule`].
#[must_use]
pub fn validate_with(bytes: &[u8], profile: Profile, rule: CheckoutRule<'_>) -> Validation {
    validate_json_as(bytes, profile, &JsonReader::new(profile), rule)
}

/// validates TOML bytes for a host: the file is read into the tree the JSON reader
/// gives, and the same rules check it. the problems are worded in the file's own
/// names, and a syntax error, a file over the size limit or one that is not UTF-8 is
/// the only problem, as `Failure::Unreadable` carries it.
#[must_use]
pub fn validate_toml(bytes: &[u8], profile: Profile, checkout: Option<&Path>) -> Validation {
    validate_toml_with(bytes, profile, CheckoutRule::Above(checkout))
}

/// validates TOML bytes for a host under a [`CheckoutRule`], as [`validate_toml`] does.
#[must_use]
pub fn validate_toml_with(bytes: &[u8], profile: Profile, rule: CheckoutRule<'_>) -> Validation {
    match toml_document::read(bytes) {
        Ok(document) => validate_tree(
            &document.tree,
            profile,
            &TomlReader::new(profile, &document),
            rule,
        ),
        Err(problem) => Validation {
            outcome: Err(Failure::Unreadable(problem)),
            checkout_rule_skipped: false,
        },
    }
}

/// validates JSON bytes under the rules the reader supplies; the problems are worded
/// by the reader.
fn validate_json_as(
    bytes: &[u8],
    profile: Profile,
    reader: &dyn Reader,
    rule: CheckoutRule<'_>,
) -> Validation {
    match json::parse(bytes) {
        Ok(root) => validate_tree(&root, profile, reader, rule),
        Err(error) => Validation {
            outcome: Err(Failure::Unreadable(error.to_string())),
            checkout_rule_skipped: false,
        },
    }
}

fn validate_tree(
    root: &Value,
    profile: Profile,
    reader: &dyn Reader,
    rule: CheckoutRule<'_>,
) -> Validation {
    let (checkout, at_sink) = match rule {
        CheckoutRule::Above(checkout) => (checkout, false),
        CheckoutRule::AboveAndAtSink(checkout) => (checkout, true),
    };
    let mut cx = Context {
        profile,
        reader,
        checkout,
        at_sink,
        errors: Errors::default(),
        checkout_rule_skipped: false,
    };
    let outcome = match read_root(root, &mut cx) {
        Some(plan) => Ok(plan),
        None => Err(Failure::Invalid(cx.errors.render(reader))),
    };
    Validation {
        outcome,
        checkout_rule_skipped: cx.checkout_rule_skipped,
    }
}

fn read_root(root: &Value, cx: &mut Context) -> Option<Plan> {
    let profile = cx.profile;
    let reader = cx.reader;
    let has_settings = reader.format().carries_settings();
    if has_settings
        && settings::check_schema_version(root, &mut cx.errors) == settings::Schema::Newer
    {
        return None;
    }
    let root = fields::read_object::<RootFields>(root, &KeyPath::root(), reader, &mut cx.errors)?;

    let routing_label = match reader.format().routing_label() {
        Some(label) => Some(label.to_string()),
        None => settings::read_routing_label(&root.routing_label, &mut cx.errors),
    };
    let image_name = read_image_name(&root.image_name, cx);
    let budget = match profile {
        Profile::Axpc => None,
        Profile::Axmc => budget::read_budget(&root.budget, reader, &mut cx.errors),
    };
    let lifecycle = if has_settings {
        settings::read_lifecycle(&root.lifecycle, reader, &mut cx.errors)
    } else {
        None
    };
    let entries = read_repositories(&root.repositories, routing_label.as_deref(), cx);
    if profile == Profile::Axmc {
        budget::check_budget(&entries, budget.as_ref(), &mut cx.errors);
    }
    if !cx.errors.is_empty() {
        return None;
    }
    let settings = match (has_settings, routing_label, lifecycle) {
        (true, Some(routing_label), Some(lifecycle)) => Some(Settings {
            schema_version: SCHEMA_VERSION,
            routing_label,
            lifecycle,
        }),
        _ => None,
    };
    Some(Plan {
        profile,
        image_name,
        entries,
        budget,
        settings,
    })
}

/// reads the image name: required on axpc, optional on axmc, where it defaults to
/// `actions-runner:local`, and checked the same way where present.
fn read_image_name(image_name: &Field, cx: &mut Context) -> Option<String> {
    let path = KeyPath::root().key(Key::ImageName);
    let expectation = "a local Docker image name with a tag, such as name:tag";
    if cx.profile == Profile::Axmc {
        return match image_name.get() {
            None => None,
            Some(Value::String(name)) if patterns::is_image_name(name) => Some(name.clone()),
            Some(_) => {
                cx.errors.add(path, Kind::MustBe(expectation));
                None
            }
        };
    }
    fields::string(
        image_name,
        &path,
        &mut cx.errors,
        patterns::is_image_name,
        expectation,
    )
    .map(str::to_string)
}

fn read_repositories(
    repositories: &Field,
    routing_label: Option<&str>,
    cx: &mut Context,
) -> Vec<Entry> {
    let path = KeyPath::root().key(Key::Repositories);
    let mut entries = Vec::new();
    let Some(value) = fields::required(repositories, &path, &mut cx.errors) else {
        return entries;
    };
    let Value::Array(nodes) = value else {
        cx.errors.add(path, Kind::MustBe("a list"));
        return entries;
    };
    if nodes.is_empty() {
        cx.errors.add(path, Kind::NoRepositories);
    }
    for (index, node) in nodes.iter().enumerate() {
        if let Some(entry) = entry::read_entry(node, index, routing_label, cx) {
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
    use crate::json::Object;

    use super::*;

    /// records the field names a derived type asks its deserializer for.
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
        let mut root = union(Profile::Axpc.root_fields(), Profile::Axmc.root_fields());
        root.extend(["lifecycle", "routingLabel"].map(String::from));
        root.sort();
        assert_eq!(field_names::<RootFields>(), root);
        assert_eq!(
            field_names::<settings::LifecycleFields>(),
            union(
                &["keepAwake", "resumeAfterStop"],
                &[
                    "startAtLogin",
                    "startRunnersOnLaunch",
                    "stopOnNetworkChange"
                ]
            )
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

    /// reads an object with every member set to `value`, then an empty object.
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
        reads_any_value::<settings::LifecycleFields>();
        reads_any_value::<entry::EntryFields>();
        reads_any_value::<budget::BudgetFields>();
        reads_any_value::<volumes::VolumeFields>();
    }

    /// a reader with its own routing label, path form and wording, to show the rule
    /// set takes them from the reader.
    struct OtherFormat;

    impl Reader for OtherFormat {
        fn format(&self) -> Format<'_> {
            Format::Json {
                routing_label: "custom",
            }
        }

        fn entry_path(&self, index: usize) -> String {
            format!("pool[{index}]")
        }

        fn undefined_keys(&self, _node: &Object, _path: &KeyPath) -> Vec<String> {
            Vec::new()
        }

        fn locate(&self, path: &KeyPath, _kind: &Kind) -> Option<Location> {
            Some(Location {
                line: path.segments().len(),
            })
        }

        fn render(&self, problem: &Problem) -> String {
            let line = problem.location.map_or(0, |location| location.line);
            format!("line {line}: {:?}", problem.kind)
        }
    }

    fn validate_other_format(text: &str, profile: Profile) -> Validation {
        validate_json_as(
            text.as_bytes(),
            profile,
            &OtherFormat,
            CheckoutRule::Above(None),
        )
    }

    #[test]
    fn a_reader_decides_the_routing_label_and_the_path_of_a_pool() {
        let found = validate_other_format(
            r#"{ "imageName": "image:tag", "repositories": [
                { "owner": "o", "repository": "r", "name": "a", "slots": 1,
                  "tokenPath": "C:\\t\\r", "labels": [], "volumes": [] }] }"#,
            Profile::Axpc,
        );
        let plan = found.outcome.unwrap();
        let entry = plan.entries.first().unwrap();
        assert_eq!(entry.path, "pool[0]");
        assert_eq!(entry.labels.last().map(String::as_str), Some("custom"));
    }

    #[test]
    fn a_reader_words_and_places_the_problems_the_rule_set_records() {
        let found = validate_other_format(
            r#"{ "imageName": "image:tag", "repositories": [
                { "owner": "o", "repository": "r", "name": "a", "slots": 0,
                  "tokenPath": "C:\\t\\r", "labels": [], "volumes": [] },
                { "owner": "o", "repository": "r", "name": "a", "slots": 1,
                  "tokenPath": "C:\\t\\r", "labels": [], "volumes": [] }] }"#,
            Profile::Axpc,
        );
        let Err(Failure::Invalid(lines)) = found.outcome else {
            panic!("expected a rejected configuration");
        };
        assert_eq!(lines, ["line 3: MustBeIntegerUpTo(16)"]);

        let found = validate_other_format(
            r#"{ "imageName": "image:tag", "repositories": [
                { "owner": "o", "repository": "r", "name": "a", "slots": 1,
                  "tokenPath": "C:\\t\\r", "labels": [], "volumes": [] },
                { "owner": "o", "repository": "s", "name": "a", "slots": 1,
                  "tokenPath": "C:\\t\\s", "labels": [], "volumes": [] }] }"#,
            Profile::Axpc,
        );
        let Err(Failure::Invalid(lines)) = found.outcome else {
            panic!("expected a rejected configuration");
        };
        assert_eq!(
            lines,
            ["line 3: NameCollision { name: \"a\", other_name: \"a\", other: 0 }"]
        );
    }

    #[test]
    fn a_wrapper_tells_a_missing_member_from_a_null() {
        let missing = read::<RootFields>("{}").unwrap();
        assert!(missing.image_name.get().is_none());
        let null = read::<RootFields>(r#"{"imageName": null}"#).unwrap();
        assert!(matches!(null.image_name.get(), Some(Value::Null)));
    }
}
