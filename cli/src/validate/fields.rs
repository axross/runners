//! reading fields from the parsed tree and recording what is wrong with them:
//! [`required`] reports `null` as a missing field, as the PowerShell validator does for
//! a required value, and a caller matching on [`Field::get`] gets a type error for it,
//! as for a field read directly; see `docs/operations/cli.md`.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, DeserializeOwned, Visitor};

use crate::de::Deserializer;
use crate::json::{Object, Value};

/// one member of an object as the validator reads it: absent, an explicit `null`,
/// or a value, so a missing member, a `null` and a wrong type never stop the read.
#[derive(Debug, Default)]
pub enum Field {
    #[default]
    Absent,
    Null,
    Value(Value),
}

static NULL: Value = Value::Null;

impl Field {
    /// returns the member's value, `null` included, or `None` when it is absent.
    #[must_use]
    pub fn get(&self) -> Option<&Value> {
        match self {
            Self::Absent => None,
            Self::Null => Some(&NULL),
            Self::Value(value) => Some(value),
        }
    }
}

impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_option(FieldVisitor)
    }
}

struct FieldVisitor;

impl<'de> Visitor<'de> for FieldVisitor {
    type Value = Field;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any value")
    }

    fn visit_none<E: de::Error>(self) -> Result<Field, E> {
        Ok(Field::Null)
    }

    fn visit_some<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Field, D::Error> {
        Value::deserialize(deserializer).map(Field::Value)
    }
}

/// the ordered list of problems found so far, one line each.
#[derive(Debug, Default)]
pub struct Errors {
    items: Vec<String>,
}

impl Errors {
    pub fn add(&mut self, line: impl Into<String>) {
        self.items.push(line.into());
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<String> {
        self.items
    }
}

/// returns the field's value, or records that it is missing; a `null` is missing.
pub fn required<'a>(field: &'a Field, path: &str, errors: &mut Errors) -> Option<&'a Value> {
    match field.get() {
        None | Some(Value::Null) => {
            errors.add(format!("{path}: required field is missing"));
            None
        }
        Some(value) => Some(value),
    }
}

/// returns the string field when `accepts` passes it, or records why not.
pub fn string<'a>(
    field: &'a Field,
    path: &str,
    errors: &mut Errors,
    accepts: fn(&str) -> bool,
    expectation: &str,
) -> Option<&'a str> {
    let value = required(field, path, errors)?;
    match value {
        Value::String(text) if accepts(text) => Some(text),
        _ => {
            errors.add(format!("{path}: must be {expectation}"));
            None
        }
    }
}

/// reads an object's members into `T`, or records that the node is not an object;
/// the members `allowed` does not list are recorded first, in document order.
pub fn read_object<T: DeserializeOwned>(
    value: &Value,
    path: &str,
    allowed: &[&str],
    errors: &mut Errors,
) -> Option<T> {
    let Value::Object(object) = value else {
        errors.add(format!("{path}: must be an object"));
        return None;
    };
    unknown_fields(object, allowed, path, errors);
    match T::deserialize(Deserializer::new(value).ignoring_field_case()) {
        Ok(fields) => Some(fields),
        Err(error) => {
            errors.add(format!("{path}: {error}"));
            None
        }
    }
}

/// records every member of the node that `allowed` does not list; the comparison is
/// case-sensitive, so `Owner` is unknown even though the read finds it.
fn unknown_fields(node: &Object, allowed: &[&str], path: &str, errors: &mut Errors) {
    for (name, _) in node.iter() {
        if !allowed.contains(&name) {
            errors.add(format!("{path}.{name}: unknown field"));
        }
    }
}
