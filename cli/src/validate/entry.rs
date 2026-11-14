//! one repository entry: its fields, then whether it takes part in the cross-entry
//! checks.

use std::path::Path;

use serde::Deserialize;

use crate::json::Value;
use crate::profile::{Os, Profile};

use super::Context;
use super::fields::{self, Errors, Field};
use super::labels::registration_labels;
use super::limits::{
    self, DEFAULT_CPUS, DEFAULT_MEMORY_GB, MAX_DISK_GB, MAX_MEMORY_GB, MAX_SLOTS, MIN_LINUX_DISK_GB,
};
use super::macos_pool;
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
pub fn read_entry(
    value: &Value,
    index: usize,
    routing_label: Option<&str>,
    cx: &mut Context,
) -> Option<Entry> {
    let profile = cx.profile;
    let reader = cx.reader;
    let path = KeyPath::root().key(Key::Repositories).index(index);
    let node = fields::read_object::<EntryFields>(value, &path, reader, &mut cx.errors)?;

    let (owner, repository) = read_owner_and_repository(&node, &path, cx);
    let token_path = read_token_path(&node.token_path, &path, cx);
    let os = read_os(&node.os, &path, cx);
    let labels = registration_labels(
        &node.labels,
        &path,
        routing_label,
        profile,
        os,
        &mut cx.errors,
    );
    let volumes = read_volumes(&node, &path, cx);
    let macos = os == Some(Os::Macos);
    let slots = if macos {
        macos_pool::read_slots(&node.slots, &path, &mut cx.errors)
    } else {
        let default = (profile == Profile::Axmc).then_some(AXMC_LINUX_DEFAULT_SLOTS);
        read_slots(&node.slots, &path, default, &mut cx.errors)
    };
    let name = read_name(&node.name, &path, &mut cx.errors);
    let cpus = if macos {
        macos_pool::read_cpus(&node.cpus, &path, &mut cx.errors)
    } else {
        read_cpus(&node.cpus, &path, profile, &mut cx.errors)
    };
    let memory_gb = if macos {
        macos_pool::read_memory(&node.memory_gb, &path, &mut cx.errors)
    } else {
        read_memory(&node.memory_gb, &path, &mut cx.errors)
    };
    let disk_gb = match (profile, macos) {
        (Profile::Axpc, _) => Some(None),
        (Profile::Axmc, true) => {
            macos_pool::read_disk(&node.disk_gb, &path, &mut cx.errors).map(Some)
        }
        (Profile::Axmc, false) => read_disk(&node.disk_gb, &path, os, &mut cx.errors).map(Some),
    };
    let (diagnostics, raw_records) = read_diagnostic_flags(&node, &path, &mut cx.errors);
    if macos && diagnostics {
        cx.errors.add(
            path.key(Key::Diagnostics),
            Kind::MacosDiagnosticsUnsupported,
        );
    }
    let diagnostic_directory =
        read_diagnostic_directory(&node.diagnostic_directory, &path, diagnostics, cx);
    if raw_records && !diagnostics {
        cx.errors.add(
            path.key(Key::DiagnosticRawRecords),
            Kind::RequiresDiagnostics,
        );
    }

    let os_usable = !has_os(cx) || os.is_some();
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
    if !cx.reader.format().has_token_paths() {
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

/// reads the owner and the repository name, from the two keys of a format that has
/// them or from the one `owner/name` key of a format that joins them.
fn read_owner_and_repository<'a>(
    node: &'a EntryFields,
    path: &KeyPath,
    cx: &mut Context,
) -> (Option<&'a str>, Option<&'a str>) {
    let owner_path = path.key(Key::Owner);
    let repository_path = path.key(Key::Repository);
    let (owner_expectation, repository_expectation) = (
        "a GitHub owner name (letters, digits and hyphens)",
        "a GitHub repository name (letters, digits and . _ -)",
    );
    if !cx.reader.format().joins_owner_and_repository() {
        let owner = fields::string(
            &node.owner,
            &owner_path,
            &mut cx.errors,
            patterns::is_owner,
            owner_expectation,
        );
        let repository = fields::string(
            &node.repository,
            &repository_path,
            &mut cx.errors,
            patterns::is_repository,
            repository_expectation,
        );
        return (owner, repository);
    }
    let Some(value) = fields::required(&node.repository, &repository_path, &mut cx.errors) else {
        return (None, None);
    };
    let parts = match value {
        Value::String(text) => text.split_once('/').filter(|(owner, repository)| {
            !owner.is_empty() && !repository.is_empty() && !repository.contains('/')
        }),
        _ => None,
    };
    let Some((owner, repository)) = parts else {
        cx.errors
            .add(repository_path, Kind::MustBe("\"owner/name\""));
        return (None, None);
    };
    (
        fields::accepted_text(
            owner,
            &owner_path,
            &mut cx.errors,
            patterns::is_owner,
            owner_expectation,
        ),
        fields::accepted_text(
            repository,
            &repository_path,
            &mut cx.errors,
            patterns::is_repository,
            repository_expectation,
        ),
    )
}

/// whether the pool has an `os` under this host and format.
fn has_os(cx: &Context) -> bool {
    cx.profile == Profile::Axmc || cx.reader.format().may_omit_os_and_volumes()
}

fn read_os(field: &Field, path: &KeyPath, cx: &mut Context) -> Option<Os> {
    if !has_os(cx) {
        return None;
    }
    if cx.reader.format().may_omit_os_and_volumes() && field.get().is_none() {
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
    } else if cx.profile == Profile::Axpc && os == Some(Os::Macos) {
        cx.errors.add(field_path, Kind::OsUnsupported(Os::Macos));
        return None;
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
        Profile::Axpc => volume_definitions(&node.volumes, path, cx.reader, &mut cx.errors),
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

/// the slots of an axmc Linux pool that sets none.
pub const AXMC_LINUX_DEFAULT_SLOTS: u32 = 2;

/// reads `slots`: absent is `default` where there is one, and a `null` then a type
/// error; without a default, absent or `null` is a missing field.
fn read_slots(
    slots: &Field,
    path: &KeyPath,
    default: Option<u32>,
    errors: &mut Errors,
) -> Option<u32> {
    let field = path.key(Key::Slots);
    let value = match default {
        Some(default) => match slots.get() {
            None => return Some(default),
            Some(value) => value,
        },
        None => fields::required(slots, &field, errors)?,
    };
    limits::read_bounded_integer(value, &field, MAX_SLOTS, errors)
}

fn read_cpus(cpus: &Field, path: &KeyPath, profile: Profile, errors: &mut Errors) -> Option<f64> {
    let Some(value) = cpus.get() else {
        return Some(DEFAULT_CPUS);
    };
    let field = path.key(Key::Cpus);
    match profile {
        Profile::Axpc => limits::cpu_count(value, &field, errors),
        Profile::Axmc => limits::whole_cpu_count(value, &field, errors),
    }
}

fn read_memory(memory_gb: &Field, path: &KeyPath, errors: &mut Errors) -> Option<u32> {
    let Some(value) = memory_gb.get() else {
        return Some(DEFAULT_MEMORY_GB);
    };
    limits::read_bounded_integer(value, &path.key(Key::MemoryGb), MAX_MEMORY_GB, errors)
}

fn read_disk(disk_gb: &Field, path: &KeyPath, os: Option<Os>, errors: &mut Errors) -> Option<u32> {
    let field = path.key(Key::DiskGb);
    let value = fields::required(disk_gb, &field, errors)?;
    let disk = limits::read_bounded_integer(value, &field, MAX_DISK_GB, errors)?;
    if os == Some(Os::Linux) && disk < MIN_LINUX_DISK_GB {
        errors.add(field, Kind::LinuxDiskTooSmall(MIN_LINUX_DISK_GB));
        return None;
    }
    Some(disk)
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
    // the checkout of the sink's own place, when the rule asks for it and the path is
    // one of this host; a Windows path read on another host names no place here
    let at_sink = Some(Path::new(directory))
        .filter(|path| cx.at_sink && path.is_absolute())
        .and_then(crate::checkout::find_at_or_above);
    let checkouts = [cx.checkout, at_sink.as_deref()];
    if checkouts.iter().all(Option::is_none) {
        cx.checkout_rule_skipped = !cx.at_sink;
    }
    if checkouts
        .into_iter()
        .flatten()
        .any(|checkout| paths::is_inside_checkout(directory, checkout, cx.profile))
    {
        cx.errors.add(field, Kind::InsideCheckout);
    }
    Some(directory.to_string())
}
