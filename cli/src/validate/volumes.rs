//! A pool's cache volumes, which only the Docker host has.

use std::collections::HashSet;

use crate::json::{Object, Value};

use super::fields::{self, Errors};
use super::patterns;

const VOLUME_FIELDS: [&str; 2] = ["suffix", "mountPath"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub suffix: String,
    pub mount_path: String,
}

/// Returns the volume definitions, or `None` after recording every problem.
pub fn volume_definitions(node: &Object, path: &str, errors: &mut Errors) -> Option<Vec<Volume>> {
    let field = format!("{path}.volumes");
    let value = fields::required(node, "volumes", &field, errors)?;
    let Value::Array(items) = value else {
        errors.add(format!("{field}: must be a list"));
        return None;
    };
    let mut definitions = Vec::new();
    let mut suffixes = HashSet::new();
    let mut mount_paths = HashSet::new();
    let mut valid = true;
    for (index, item) in items.iter().enumerate() {
        let item_path = format!("{field}[{index}]");
        let Some(object) = fields::object(item, &item_path, errors) else {
            valid = false;
            continue;
        };
        fields::unknown_fields(object, &VOLUME_FIELDS, &item_path, errors);
        let suffix = fields::string(
            object,
            "suffix",
            &format!("{item_path}.suffix"),
            errors,
            patterns::is_name,
            "lowercase letters, digits and hyphens, starting with a letter or digit",
        );
        let mut mount_path = fields::string(
            object,
            "mountPath",
            &format!("{item_path}.mountPath"),
            errors,
            patterns::is_mount_path,
            "an absolute container path of letters, digits and . _ / -",
        );
        if mount_path.is_some_and(|text| text.contains("..")) {
            errors.add(format!("{item_path}.mountPath: must not contain '..'"));
            mount_path = None;
        }
        let (Some(suffix), Some(mount_path)) = (suffix, mount_path) else {
            valid = false;
            continue;
        };
        if !suffixes.insert(suffix.to_ascii_lowercase()) {
            errors.add(format!("{item_path}.suffix: duplicate suffix '{suffix}'"));
            valid = false;
        }
        if !mount_paths.insert(mount_path.to_ascii_lowercase()) {
            errors.add(format!(
                "{item_path}.mountPath: duplicate mount path '{mount_path}'"
            ));
            valid = false;
        }
        definitions.push(Volume {
            suffix: suffix.to_string(),
            mount_path: mount_path.to_string(),
        });
    }
    valid.then_some(definitions)
}
