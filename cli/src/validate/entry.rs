//! One repository entry: its fields, then whether it takes part in the cross-entry
//! checks.

use crate::json::{Object, Value};
use crate::profile::{Os, Profile};

use super::Context;
use super::fields::{self, Errors};
use super::labels::registration_labels;
use super::limits::{self, DEFAULT_CPUS, DEFAULT_MEMORY_GB, MAX_DISK_GB, MAX_MEMORY_GB, MAX_SLOTS};
use super::paths;
use super::patterns;
use super::plan::{Entry, PlannedVolume};
use super::volumes::volume_definitions;

const MAX_NAME_LENGTH: usize = 64;
// GitHub documents no limit for a runner's name. The runner name is the entry's name
// plus a slot number and a 17-digit timestamp, so capping the entry name keeps every
// name short, on an assumed limit rather than a known one.

const POSIX_PATH: &str = "an absolute POSIX path without control characters or empty, '.' or '..' components, such as /path/to/file";

/// Returns the entry, or `None` after recording every problem. An entry whose
/// owner, repository, token path, labels, volumes, slots, name, CPU and memory
/// values (and on axmc its OS and disk) are all usable is returned even when a
/// diagnostic field is not, so it still takes part in the cross-entry checks.
pub fn read_entry(value: &Value, path: &str, cx: &mut Context) -> Option<Entry> {
    let node = fields::object(value, path, &mut cx.errors)?;
    let profile = cx.profile;
    fields::unknown_fields(node, profile.entry_fields(), path, &mut cx.errors);

    let owner = fields::string(
        node,
        "owner",
        &format!("{path}.owner"),
        &mut cx.errors,
        patterns::is_owner,
        "a GitHub owner name (letters, digits and hyphens)",
    );
    let repository = fields::string(
        node,
        "repository",
        &format!("{path}.repository"),
        &mut cx.errors,
        patterns::is_repository,
        "a GitHub repository name (letters, digits and . _ -)",
    );
    let token_path = match profile {
        Profile::Axpc => fields::string(
            node,
            "tokenPath",
            &format!("{path}.tokenPath"),
            &mut cx.errors,
            patterns::is_windows_token_path,
            "an absolute Windows path, such as C:\\path\\to\\file or \\\\server\\share\\file",
        ),
        Profile::Axmc => fields::string(
            node,
            "tokenPath",
            &format!("{path}.tokenPath"),
            &mut cx.errors,
            paths::is_absolute_posix,
            POSIX_PATH,
        ),
    };
    let os = read_os(node, path, profile, &mut cx.errors);
    let usable_os = if profile == Profile::Axmc { os } else { None };
    let labels = registration_labels(node, path, profile, usable_os, &mut cx.errors);
    let volumes = read_volumes(node, path, profile, &mut cx.errors);
    let slots = fields::required(node, "slots", &format!("{path}.slots"), &mut cx.errors).and_then(
        |value| {
            limits::read_bounded_integer(value, &format!("{path}.slots"), MAX_SLOTS, &mut cx.errors)
        },
    );
    let name = read_name(node, path, &mut cx.errors);
    let cpus = read_cpus(node, path, &mut cx.errors);
    let memory_gb = read_memory(node, path, &mut cx.errors);
    let disk_gb = match profile {
        Profile::Axpc => Some(None),
        Profile::Axmc => read_disk(node, path, &mut cx.errors).map(Some),
    };
    let (diagnostics, raw_records) = read_diagnostic_flags(node, path, &mut cx.errors);
    read_diagnostic_directory(node, path, diagnostics, cx);
    if raw_records && !diagnostics {
        cx.errors
            .add(format!("{path}.diagnosticRawRecords: requires diagnostics"));
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
        path: path.to_string(),
        owner: owner.to_string(),
        repository: repository.to_string(),
        name: name.clone(),
        slots,
        token_path: token_path.to_string(),
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
        diagnostic_raw_records: raw_records,
        os,
        disk_gb,
    })
}

fn read_os(node: &Object, path: &str, profile: Profile, errors: &mut Errors) -> Option<Os> {
    if profile != Profile::Axmc {
        return None;
    }
    let field = format!("{path}.os");
    let value = fields::required(node, "os", &field, errors)?;
    let os = match value {
        Value::String(text) => Os::from_name(text),
        _ => None,
    };
    if os.is_none() {
        errors.add(format!("{field}: must be \"linux\" or \"macos\""));
    }
    os
}

/// Reads the cache volumes on axpc. An axmc pool keeps none, so the field is
/// rejected there, whatever it holds.
fn read_volumes(
    node: &Object,
    path: &str,
    profile: Profile,
    errors: &mut Errors,
) -> Option<Vec<super::volumes::Volume>> {
    match profile {
        Profile::Axpc => volume_definitions(node, path, errors),
        Profile::Axmc => {
            if node.get("volumes").is_some() {
                errors.add(format!(
                    "{path}.volumes: not supported on axmc, because VM pools keep no volumes"
                ));
            }
            Some(Vec::new())
        }
    }
}

fn read_name(node: &Object, path: &str, errors: &mut Errors) -> Option<String> {
    let field = format!("{path}.name");
    let name = fields::string(
        node,
        "name",
        &field,
        errors,
        patterns::is_name,
        "lowercase letters, digits and hyphens, starting with a letter or digit",
    )?;
    if name.len() > MAX_NAME_LENGTH {
        errors.add(format!(
            "{field}: name is {} characters, at most {MAX_NAME_LENGTH} are allowed because it starts every runner name",
            name.len()
        ));
        return None;
    }
    Some(name.to_string())
}

fn read_cpus(node: &Object, path: &str, errors: &mut Errors) -> Option<f64> {
    match node.get("cpus") {
        None => Some(DEFAULT_CPUS),
        Some(value) => limits::cpu_count(value, &format!("{path}.cpus"), errors),
    }
}

fn read_memory(node: &Object, path: &str, errors: &mut Errors) -> Option<u32> {
    let Some(value) = node.get("memoryGb") else {
        return Some(DEFAULT_MEMORY_GB);
    };
    limits::read_bounded_integer(value, &format!("{path}.memoryGb"), MAX_MEMORY_GB, errors)
}

fn read_disk(node: &Object, path: &str, errors: &mut Errors) -> Option<u32> {
    let field = format!("{path}.diskGb");
    let value = fields::required(node, "diskGb", &field, errors)?;
    limits::read_bounded_integer(value, &field, MAX_DISK_GB, errors)
}

/// Returns the two diagnostic switches, each false when absent or not a boolean.
fn read_diagnostic_flags(node: &Object, path: &str, errors: &mut Errors) -> (bool, bool) {
    let mut flags = [false, false];
    for (flag, setting) in flags
        .iter_mut()
        .zip(["diagnostics", "diagnosticRawRecords"])
    {
        match node.get(setting) {
            None => {}
            Some(Value::Bool(enabled)) => *flag = *enabled,
            Some(_) => errors.add(format!("{path}.{setting}: must be a boolean")),
        }
    }
    (flags[0], flags[1])
}

fn read_diagnostic_directory(node: &Object, path: &str, diagnostics: bool, cx: &mut Context) {
    let field = format!("{path}.diagnosticDirectory");
    if !diagnostics {
        if node.get("diagnosticDirectory").is_some() {
            cx.errors.add(format!("{field}: requires diagnostics"));
        }
        return;
    }
    let (accepts, expectation): (fn(&str) -> bool, &str) = match cx.profile {
        Profile::Axpc => (
            patterns::is_windows_directory,
            "an absolute local Windows directory outside the checkout",
        ),
        Profile::Axmc => (
            paths::is_absolute_posix,
            "an absolute local POSIX directory outside the checkout",
        ),
    };
    let Some(directory) = fields::string(
        node,
        "diagnosticDirectory",
        &field,
        &mut cx.errors,
        accepts,
        expectation,
    ) else {
        return;
    };
    if cx.profile == Profile::Axpc && paths::has_windows_dot_component(directory) {
        cx.errors
            .add(format!("{field}: must not contain dot path components"));
    }
    match cx.checkout {
        Some(checkout) => {
            if paths::is_inside_checkout(directory, checkout, cx.profile) {
                cx.errors
                    .add(format!("{field}: must be outside the checkout"));
            }
        }
        None => cx.checkout_rule_skipped = true,
    }
}
