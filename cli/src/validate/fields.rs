//! Reading fields from the parsed tree and recording what is wrong with them.
//!
//! A field the PowerShell validator reads as a required value reports `null` as
//! "required field is missing"; a field it reads directly reports `null` as that
//! field's type error. [`required`] is the first kind, and a caller that matches on
//! [`Object::get`] itself is the second.

use crate::json::{Object, Value};

/// The ordered list of problems found so far, one line each.
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

/// Returns the field's value, or records that it is missing. A `null` is missing.
pub fn required<'a>(
    node: &'a Object,
    name: &str,
    path: &str,
    errors: &mut Errors,
) -> Option<&'a Value> {
    match node.get(name) {
        None | Some(Value::Null) => {
            errors.add(format!("{path}: required field is missing"));
            None
        }
        Some(value) => Some(value),
    }
}

/// Returns the string field when `accepts` passes it, or records why not.
pub fn string<'a>(
    node: &'a Object,
    name: &str,
    path: &str,
    errors: &mut Errors,
    accepts: fn(&str) -> bool,
    expectation: &str,
) -> Option<&'a str> {
    let value = required(node, name, path, errors)?;
    match value {
        Value::String(text) if accepts(text) => Some(text),
        _ => {
            errors.add(format!("{path}: must be {expectation}"));
            None
        }
    }
}

/// Records every member of the node that `allowed` does not list. The comparison is
/// case-sensitive, so `Owner` is unknown even though lookups find it.
pub fn unknown_fields(node: &Object, allowed: &[&str], path: &str, errors: &mut Errors) {
    for (name, _) in node.iter() {
        if !allowed.contains(&name) {
            errors.add(format!("{path}.{name}: unknown field"));
        }
    }
}

/// Returns the node as an object, or records that it is not one.
pub fn object<'a>(value: &'a Value, path: &str, errors: &mut Errors) -> Option<&'a Object> {
    if let Value::Object(object) = value {
        Some(object)
    } else {
        errors.add(format!("{path}: must be an object"));
        None
    }
}
