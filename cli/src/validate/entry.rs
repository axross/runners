//! one repository entry: its fields, then whether it takes part in the cross-entry
//! checks.

use serde::Deserialize;

use crate::json::Value;
use crate::profile::{Os, Profile};

use super::Context;
use super::fields::{self, Errors, Field};
use super::labels::registration_labels;
use super::limits::{self, DEFAULT_CPUS, DEFAULT_MEMORY_GB, MAX_DISK_GB, MAX_MEMORY_GB, MAX_SLOTS};
use super::paths;
use super::patterns;
use super::plan::{Entry, PlannedVolume};
use super::problem::{Key, KeyPath, Kind};
use super::volumes::volume_definitions;

const MAX_NAME_LENGTH: usize = 64;
// GitHub documents no limit for a runner's name; the runner name is the entry's name
// plus a slot number and a 17-digit timestamp, so this caps the entry name on an
// assumed limit.

const POSIX_PATH: &str = "an absolute POSIX path without control characters or empty, '.' or '..' components, such as /path/to/file";

/// the members of an entry, named for both profiles; a profile ignores the ones it
/// does not have, which the read already reported as unknown.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EntryFields {
    #[serde(default)]
    owner: Field,
    #[serde(default)]
    repository: Field,
    #[serde(default)]
    name: Field,
    #[serde(default)]
    slots: Field,
    #[serde(default)]
    token_path: Field,
    #[serde(default)]
    labels: Field,
    #[serde(default)]
    volumes: Field,
    #[serde(default)]
    cpus: Field,
    #[serde(default)]
    memory_gb: Field,
    #[serde(default)]
    diagnostics: Field,
    #[serde(default)]
    diagnostic_directory: Field,
    #[serde(default)]
    diagnostic_raw_records: Field,
    #[serde(default)]
    os: Field,
    #[serde(default)]
    disk_gb: Field,
}

/// returns the entry at `index` in the file, or `None` after recording every problem;
/// an entry with a bad diagnostic field is still returned, so it takes part in the
/// cross-entry checks.
pub fn read_entry(value: &Value, index: usize, cx: &mut Context) -> Option<Entry> {
    let profile = cx.profile;
    let reader = cx.reader;
    let path = KeyPath::root().key(Key::Repositories).index(index);
    let node =
        fields::read_object::<EntryFields>(value, &path, profile.entry_fields(), &mut cx.errors)?;

    let owner = fields::string(
        &node.owner,
        &path.key(Key::Owner),
        &mut cx.errors,
        patterns::is_owner,
        "a GitHub owner name (letters, digits and hyphens)",
    );
    let repository = fields::string(
        &node.repository,
        &path.key(Key::Repository),
        &mut cx.errors,
        patterns::is_repository,
        "a GitHub repository name (letters, digits and . _ -)",
    );
    let token_path = read_token_path(&node.token_path, &path, cx);
    let os = read_os(&node.os, &path, cx);
    let labels = registration_labels(
        &node.labels,
        &path,
        reader.routing_label(),
        profile,
        os,
        &mut cx.errors,
    );
    let volumes = read_volumes(&node, &path, cx);
    let slots_path = path.key(Key::Slots);
    let slots = fields::required(&node.slots, &slots_path, &mut cx.errors).and_then(|value| {
        limits::read_bounded_integer(value, &slots_path, MAX_SLOTS, &mut cx.errors)
    });
    let name = read_name(&node.name, &path, &mut cx.errors);
    let cpus = read_cpus(&node.cpus, &path, &mut cx.errors);
    let memory_gb = read_memory(&node.memory_gb, &path, &mut cx.errors);
    let disk_gb = match profile {
        Profile::Axpc => Some(None),
        Profile::Axmc => read_disk(&node.disk_gb, &path, &mut cx.errors).map(Some),
    };
    let (diagnostics, raw_records) = read_diagnostic_flags(&node, &path, &mut cx.errors);
    let diagnostic_directory =
        read_diagnostic_directory(&node.diagnostic_directory, &path, diagnostics, cx);
    if raw_records && !diagnostics {
        cx.errors.add(
            path.key(Key::DiagnosticRawRecords),
            Kind::RequiresDiagnostics,
        );
    }

    let os_usable = profile == Profile::Axpc || os.is_some();
    let (Some(owner), Some(repository), Some(token_path), Some(labels), Some(volumes), Some(slots)) =
        (owner, repository, token_path, labels, volumes, slots)
    else {
        return None;
    };
    let (Some(name), Some(cpus), Some(memory_gb), Some(disk_gb)) = (name, cpus, memory_gb, disk_gb)
    else {
        return None;
    };
    if !os_usable {
        return None;
    }
    Some(Entry {
        index,
        path: reader.entry_path(index),
        owner: owner.to_string(),
        repository: repository.to_string(),
        name: name.clone(),
        slots,
        token_path: token_path.map(str::to_string),
        labels,
        cpus,
        memory_gb,
        volumes: volumes
            .into_iter()
            .map(|volume| PlannedVolume {
                name: format!("{name}-{}", volume.suffix),
                mount_path: volume.mount_path,
            })
            .collect(),
        diagnostics,
        diagnostic_directory,
        diagnostic_raw_records: raw_records,
        os,
        disk_gb,
    })
}

/// reads the token path where the reader's format has one; the outer `None` is a
/// problem recorded, the inner one a format without token paths.
fn read_token_path<'a>(
    field: &'a Field,
    path: &KeyPath,
    cx: &mut Context,
) -> Option<Option<&'a str>> {
    if !cx.reader.applies_token_path_rules() {
        return Some(None);
    }
    let field_path = path.key(Key::TokenPath);
    match cx.profile {
        Profile::Axpc => fields::string(
            field,
            &field_path,
            &mut cx.errors,
            patterns::is_windows_token_path,
            "an absolute Windows path, such as C:\\path\\to\\file or \\\\server\\share\\file",
        ),
        Profile::Axmc => fields::string(
            field,
            &field_path,
            &mut cx.errors,
            paths::is_absolute_posix,
            POSIX_PATH,
        ),
    }
    .map(Some)
}

fn read_os(field: &Field, path: &KeyPath, cx: &mut Context) -> Option<Os> {
    if cx.profile != Profile::Axmc {
        return None;
    }
    if cx.reader.may_omit_os_and_volumes() && field.get().is_none() {
        return Some(Os::Linux);
    }
    let field_path = path.key(Key::Os);
    let value = fields::required(field, &field_path, &mut cx.errors)?;
    let os = match value {
        Value::String(text) => Os::from_name(text),
        _ => None,
    };
    if os.is_none() {
        cx.errors
            .add(field_path, Kind::MustBe("\"linux\" or \"macos\""));
    }
    os
}

/// reads the cache volumes on axpc; an axmc pool keeps none, so the field is
/// rejected there, whatever it holds.
fn read_volumes(
    node: &EntryFields,
    path: &KeyPath,
    cx: &mut Context,
) -> Option<Vec<super::volumes::Volume>> {
    match cx.profile {
        Profile::Axpc => volume_definitions(
            &node.volumes,
            path,
            cx.reader.may_omit_os_and_volumes(),
            &mut cx.errors,
        ),
        Profile::Axmc => {
            if node.volumes.get().is_some() {
                cx.errors
                    .add(path.key(Key::Volumes), Kind::VolumesUnsupported);
            }
            Some(Vec::new())
        }
    }
}

fn read_name(name: &Field, path: &KeyPath, errors: &mut Errors) -> Option<String> {
    let field = path.key(Key::Name);
    let name = fields::string(
        name,
        &field,
        errors,
        patterns::is_name,
        "lowercase letters, digits and hyphens, starting with a letter or digit",
    )?;
    if name.len() > MAX_NAME_LENGTH {
        errors.add(
            field,
            Kind::NameTooLong {
                length: name.len(),
                max: MAX_NAME_LENGTH,
            },
        );
        return None;
    }
    Some(name.to_string())
}

fn read_cpus(cpus: &Field, path: &KeyPath, errors: &mut Errors) -> Option<f64> {
    match cpus.get() {
        None => Some(DEFAULT_CPUS),
        Some(value) => limits::cpu_count(value, &path.key(Key::Cpus), errors),
    }
}

fn read_memory(memory_gb: &Field, path: &KeyPath, errors: &mut Errors) -> Option<u32> {
    let Some(value) = memory_gb.get() else {
        return Some(DEFAULT_MEMORY_GB);
    };
    limits::read_bounded_integer(value, &path.key(Key::MemoryGb), MAX_MEMORY_GB, errors)
}

fn read_disk(disk_gb: &Field, path: &KeyPath, errors: &mut Errors) -> Option<u32> {
    let field = path.key(Key::DiskGb);
    let value = fields::required(disk_gb, &field, errors)?;
    limits::read_bounded_integer(value, &field, MAX_DISK_GB, errors)
}

/// returns the two diagnostic switches, each false when absent or not a boolean.
fn read_diagnostic_flags(node: &EntryFields, path: &KeyPath, errors: &mut Errors) -> (bool, bool) {
    let mut flags = [false, false];
    for (flag, (field, setting)) in flags.iter_mut().zip([
        (&node.diagnostics, Key::Diagnostics),
        (&node.diagnostic_raw_records, Key::DiagnosticRawRecords),
    ]) {
        match field.get() {
            None => {}
            Some(Value::Bool(enabled)) => *flag = *enabled,
            Some(_) => errors.add(path.key(setting), Kind::MustBe("a boolean")),
        }
    }
    (flags[0], flags[1])
}

/// returns the directory of a diagnostics-enabled entry, when it is valid.
fn read_diagnostic_directory(
    directory: &Field,
    path: &KeyPath,
    diagnostics: bool,
    cx: &mut Context,
) -> Option<String> {
    let field = path.key(Key::DiagnosticDirectory);
    if !diagnostics {
        if directory.get().is_some() {
            cx.errors.add(field, Kind::RequiresDiagnostics);
        }
        return None;
    }
    let (accepts, expectation): (fn(&str) -> bool, &'static str) = match cx.profile {
        Profile::Axpc => (
            patterns::is_windows_directory,
            "an absolute local Windows directory outside the checkout",
        ),
        Profile::Axmc => (
            paths::is_absolute_posix,
            "an absolute local POSIX directory outside the checkout",
        ),
    };
    let directory = fields::string(directory, &field, &mut cx.errors, accepts, expectation)?;
    if cx.profile == Profile::Axpc && paths::has_windows_dot_component(directory) {
        cx.errors.add(field.clone(), Kind::DotPathComponents);
    }
    match cx.checkout {
        Some(checkout) => {
            if paths::is_inside_checkout(directory, checkout, cx.profile) {
                cx.errors.add(field, Kind::InsideCheckout);
            }
        }
        None => cx.checkout_rule_skipped = true,
    }
    Some(directory.to_string())
}
