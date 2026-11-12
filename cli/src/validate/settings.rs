//! the settings a file carries beyond the host's rules: the schema version, the
//! routing label and the lifecycle table, for a format whose file holds them.

use serde::Deserialize;

use crate::fold;
use crate::json::Value;
use crate::profile::Profile;

use super::fields::{self, Errors, Field};
use super::patterns;
use super::plan::Lifecycle;
use super::problem::{Key, KeyPath, Kind, LABEL_SHAPE};
use super::reader::Reader;

/// the only schema version this program reads.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LifecycleFields {
    #[serde(default)]
    start_at_login: Field,
    #[serde(default)]
    start_runners_on_launch: Field,
    #[serde(default)]
    keep_awake: Field,
    #[serde(default)]
    resume_after_stop: Field,
}

/// whether the rest of the file is worth reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    /// the version is this program's, or wrong in a way recorded beside the others.
    Known,
    /// a newer program wrote the file, which is then the only problem.
    Newer,
}

/// records a missing schema version, one other than [`SCHEMA_VERSION`], or a newer
/// one, which stops the read.
pub fn check_schema_version(root: &Value, errors: &mut Errors) -> Schema {
    let path = KeyPath::root().key(Key::SchemaVersion);
    let field = match root {
        Value::Object(object) => object
            .iter()
            .find(|(name, _)| *name == super::json_reader::key_name(Key::SchemaVersion))
            .map(|(_, value)| value),
        _ => None,
    };
    let Some(Value::Number(number)) = field else {
        match field {
            None => errors.add(path, Kind::Missing),
            Some(_) => errors.add(path, Kind::MustBe("the integer 1")),
        }
        return Schema::Known;
    };
    match number.as_integer() {
        Some(version) if version == i128::from(SCHEMA_VERSION) => {}
        Some(version) if version > i128::from(SCHEMA_VERSION) => {
            errors.add(path, Kind::NewerSchemaVersion);
            return Schema::Newer;
        }
        _ => errors.add(path, Kind::MustBe("the integer 1")),
    }
    Schema::Known
}

/// reads the routing label: a label that is no platform label of any host.
pub fn read_routing_label(field: &Field, errors: &mut Errors) -> Option<String> {
    let path = KeyPath::root().key(Key::RoutingLabel);
    let value = fields::required(field, &path, errors)?;
    let label = match value {
        Value::String(label) if patterns::is_label(label) => label,
        _ => {
            errors.add(path, Kind::MustBe(LABEL_SHAPE));
            return None;
        }
    };
    if Profile::known_platform_labels()
        .iter()
        .any(|known| fold::eq_ascii(known, label))
    {
        errors.add(
            path,
            Kind::ForeignPlatformLabel {
                label: label.clone(),
                os: None,
            },
        );
        return None;
    }
    Some(label.clone())
}

/// reads the lifecycle table, whose absence leaves every value unset.
pub fn read_lifecycle(
    field: &Field,
    reader: &dyn Reader,
    errors: &mut Errors,
) -> Option<Lifecycle> {
    let path = KeyPath::root().key(Key::Lifecycle);
    let Some(value) = field.get() else {
        return Some(Lifecycle::default());
    };
    let node = fields::read_object::<LifecycleFields>(value, &path, reader, errors)?;
    let mut valid = true;
    let mut read = |field: &Field, key: Key| match field.get() {
        None => None,
        Some(Value::Bool(setting)) => Some(*setting),
        Some(_) => {
            errors.add(path.key(key), Kind::MustBe("a boolean"));
            valid = false;
            None
        }
    };
    let lifecycle = Lifecycle {
        start_at_login: read(&node.start_at_login, Key::StartAtLogin),
        start_runners_on_launch: read(&node.start_runners_on_launch, Key::StartRunnersOnLaunch),
        keep_awake: read(&node.keep_awake, Key::KeepAwake),
        resume_after_stop: read(&node.resume_after_stop, Key::ResumeAfterStop),
    };
    valid.then_some(lifecycle)
}
