//! a pool's cache volumes, which only the Docker host has.

use std::collections::HashSet;

use serde::Deserialize;

use crate::json::Value;

use super::fields::{self, Errors, Field};
use super::patterns;
use super::problem::{Key, KeyPath, Kind};
use super::reader::Reader;

pub(super) const VOLUME_FIELDS: [&str; 2] = ["suffix", "mountPath"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VolumeFields {
    #[serde(default)]
    suffix: Field,
    #[serde(default)]
    mount_path: Field,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub suffix: String,
    pub mount_path: String,
}

/// returns the volume definitions, or `None` after recording every problem; a
/// pool that lists none has none when the reader allows it.
pub fn volume_definitions(
    volumes: &Field,
    path: &KeyPath,
    reader: &dyn Reader,
    errors: &mut Errors,
) -> Option<Vec<Volume>> {
    if reader.may_omit_os_and_volumes() && volumes.get().is_none() {
        return Some(Vec::new());
    }
    let field = path.key(Key::Volumes);
    let value = fields::required(volumes, &field, errors)?;
    let Value::Array(items) = value else {
        errors.add(field, Kind::MustBe("a list"));
        return None;
    };
    let mut definitions = Vec::new();
    let mut suffixes = HashSet::new();
    let mut mount_paths = HashSet::new();
    let mut valid = true;
    for (index, item) in items.iter().enumerate() {
        let item_path = field.index(index);
        let Some(volume) = fields::read_object::<VolumeFields>(item, &item_path, reader, errors)
        else {
            valid = false;
            continue;
        };
        let suffix = fields::string(
            &volume.suffix,
            &item_path.key(Key::Suffix),
            errors,
            patterns::is_name,
            "lowercase letters, digits and hyphens, starting with a letter or digit",
        );
        let mut mount_path = fields::string(
            &volume.mount_path,
            &item_path.key(Key::MountPath),
            errors,
            patterns::is_mount_path,
            "an absolute container path of letters, digits and . _ / -",
        );
        if mount_path.is_some_and(|text| text.contains("..")) {
            errors.add(item_path.key(Key::MountPath), Kind::MountPathHasDotDot);
            mount_path = None;
        }
        let (Some(suffix), Some(mount_path)) = (suffix, mount_path) else {
            valid = false;
            continue;
        };
        if !suffixes.insert(suffix.to_ascii_lowercase()) {
            errors.add(
                item_path.key(Key::Suffix),
                Kind::DuplicateSuffix(suffix.to_string()),
            );
            valid = false;
        }
        if !mount_paths.insert(mount_path.to_ascii_lowercase()) {
            errors.add(
                item_path.key(Key::MountPath),
                Kind::DuplicateMountPath(mount_path.to_string()),
            );
            valid = false;
        }
        definitions.push(Volume {
            suffix: suffix.to_string(),
            mount_path: mount_path.to_string(),
        });
    }
    valid.then_some(definitions)
}
