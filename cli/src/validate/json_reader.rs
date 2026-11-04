//! the JSON reader's side of validation: today's rules for each host, and the path form
//! and wording that the shared fixtures pin.

use crate::json::Object;
use crate::profile::Profile;

use super::budget::BUDGET_FIELDS;
use super::limits::{CPU_RANGE, format_nanos};
use super::problem::{Key, KeyPath, Kind, Location, Problem, Segment};
use super::reader::Reader;
use super::volumes::VOLUME_FIELDS;

/// reads a host configuration written as JSON for one host profile.
#[derive(Debug, Clone, Copy)]
pub struct JsonReader {
    profile: Profile,
}

impl JsonReader {
    #[must_use]
    pub fn new(profile: Profile) -> Self {
        Self { profile }
    }
}

impl JsonReader {
    /// the keys the profile defines for the object at `path`.
    fn defined_keys(&self, path: &KeyPath) -> &'static [&'static str] {
        match path.segments() {
            [] => self.profile.root_fields(),
            [Segment::Key(Key::Repositories), Segment::Index(_)] => self.profile.entry_fields(),
            [Segment::Key(Key::Budget)] => &BUDGET_FIELDS,
            [
                Segment::Key(Key::Repositories),
                Segment::Index(_),
                Segment::Key(Key::Volumes),
                Segment::Index(_),
            ] => &VOLUME_FIELDS,
            _ => &[],
        }
    }
}

impl Reader for JsonReader {
    fn routing_label(&self) -> Option<&str> {
        Some(self.profile.routing_label())
    }

    fn may_omit_os_and_volumes(&self) -> bool {
        false
    }

    fn windows_pool_has_os(&self) -> bool {
        false
    }

    fn joins_owner_and_repository(&self) -> bool {
        false
    }

    fn applies_token_path_rules(&self) -> bool {
        true
    }

    fn entry_path(&self, index: usize) -> String {
        path_text(&KeyPath::root().key(Key::Repositories).index(index))
    }

    fn undefined_keys(&self, node: &Object, path: &KeyPath) -> Vec<String> {
        let allowed = self.defined_keys(path);
        node.iter()
            .map(|(name, _)| name)
            .filter(|name| !allowed.contains(name))
            .map(str::to_string)
            .collect()
    }

    fn locate(&self, _path: &KeyPath, _kind: &Kind) -> Option<Location> {
        None
    }

    fn render(&self, problem: &Problem) -> String {
        let path = path_text(&problem.path);
        let profile = self.profile;
        let slots = key_name(Key::Slots);
        match &problem.kind {
            Kind::Missing => format!("{path}: required field is missing"),
            Kind::MustBe(what) => format!("{path}: must be {what}"),
            Kind::MustBeIntegerUpTo(max) => format!("{path}: must be an integer from 1 to {max}"),
            Kind::CpuCountOutOfRange => format!("{path}: {CPU_RANGE}"),
            Kind::CpuCountRoundsToZero => format!(
                "{path}: {CPU_RANGE}, and large enough not to be written as 0, which Docker reads as no limit"
            ),
            Kind::UnknownField(name) => format!("{path}.{name}: unknown field"),
            Kind::Unreadable(message) => format!("{path}: {message}"),
            Kind::NoRepositories => format!("{path}: must list at least one repository"),
            Kind::LabelShape => format!(
                "{path}: each label must be a string of letters, digits and . _ : / - starting with a letter or digit"
            ),
            Kind::PlatformLabel(label) => {
                format!("{path}: '{label}' is always added, so list only non-platform labels")
            }
            Kind::ForeignPlatformLabel {
                label,
                os: Some(os),
            } => format!(
                "{path}: '{label}' is a platform label that a {os} pool on {profile} cannot provide, so list only non-platform labels"
            ),
            Kind::ForeignPlatformLabel { label, os: None } => {
                format!("{path}: '{label}' is a platform label, so list only non-platform labels")
            }
            Kind::DuplicateLabel(label) => format!("{path}: duplicate label '{label}'"),
            Kind::VolumesUnsupported => {
                format!("{path}: not supported on {profile}, because VM pools keep no volumes")
            }
            Kind::OsUnsupported(os) => {
                format!("{path}: a {os} pool is not supported on {profile}")
            }
            Kind::NewerSchemaVersion => format!("{path}: written by a newer version"),
            Kind::MountPathHasDotDot => format!("{path}: must not contain '..'"),
            Kind::DuplicateSuffix(suffix) => format!("{path}: duplicate suffix '{suffix}'"),
            Kind::DuplicateMountPath(mount_path) => {
                format!("{path}: duplicate mount path '{mount_path}'")
            }
            Kind::NameTooLong { length, max } => format!(
                "{path}: name is {length} characters, at most {max} are allowed because it starts every runner name"
            ),
            Kind::RequiresDiagnostics => {
                format!("{path}: requires {}", key_name(Key::Diagnostics))
            }
            Kind::DotPathComponents => format!("{path}: must not contain dot path components"),
            Kind::InsideCheckout => format!("{path}: must be outside the checkout"),
            Kind::NameCollision {
                name,
                other_name,
                other,
            } => format!(
                "{path}: name '{name}' collides with '{other_name}' at {}; set a distinct name on one entry",
                self.entry_path(*other)
            ),
            Kind::TokenFileDiffers { other } => format!(
                "{path}: pools for the same repository must use the same token file as {}",
                self.entry_path(*other)
            ),
            Kind::TokenFileReused { token_path, other } => format!(
                "{path}: token file '{token_path}' is already used at {}; each repository needs its own token",
                self.entry_path(*other)
            ),
            Kind::AmbiguousRouting { labels, other } => format!(
                "{path}: ambiguous same-repository routing; labels '{labels}' also appear at {}",
                self.entry_path(*other)
            ),
            Kind::MacosVmsOverLimit { total, limit } => format!(
                "{path}: macOS pools total {total} VMs (the sum of their {slots}), above the limit of {limit}"
            ),
            Kind::CpuBudgetExceeded {
                total_nanos,
                budget_nanos,
            } => format!(
                "{path}: pools total {} CPUs (the sum of {slots} times {}), above the budget of {}",
                format_nanos(*total_nanos),
                key_name(Key::Cpus),
                format_nanos(*budget_nanos)
            ),
            Kind::MemoryBudgetExceeded { total, budget } => format!(
                "{path}: pools total {total} GB of memory (the sum of {slots} times {}), above the budget of {budget} GB",
                key_name(Key::MemoryGb)
            ),
            Kind::DiskBudgetExceeded { total, budget } => format!(
                "{path}: pools total {total} GB of disk (the sum of {slots} times {}), above the budget of {budget} GB",
                key_name(Key::DiskGb)
            ),
        }
    }
}

/// the key as the JSON file spells it, which is also the name the shared tree gives it
/// whatever the format; the keys only a file with settings has are spelled the same
/// way, though no JSON file has them.
pub(super) fn key_name(key: Key) -> &'static str {
    match key {
        Key::ImageName => "imageName",
        Key::Repositories => "repositories",
        Key::Budget => "budget",
        Key::Owner => "owner",
        Key::Repository => "repository",
        Key::Name => "name",
        Key::Slots => "slots",
        Key::TokenPath => "tokenPath",
        Key::Labels => "labels",
        Key::Volumes => "volumes",
        Key::Cpus => "cpus",
        Key::MemoryGb => "memoryGb",
        Key::Diagnostics => "diagnostics",
        Key::DiagnosticDirectory => "diagnosticDirectory",
        Key::DiagnosticRawRecords => "diagnosticRawRecords",
        Key::Os => "os",
        Key::DiskGb => "diskGb",
        Key::Suffix => "suffix",
        Key::MountPath => "mountPath",
        Key::SchemaVersion => "schemaVersion",
        Key::RoutingLabel => "routingLabel",
        Key::Lifecycle => "lifecycle",
        Key::StartAtLogin => "startAtLogin",
        Key::StartRunnersOnLaunch => "startRunnersOnLaunch",
        Key::KeepAwake => "keepAwake",
        Key::ResumeAfterStop => "resumeAfterStop",
    }
}

/// the path as `repositories[0].labels`; the root is `configuration`.
fn path_text(path: &KeyPath) -> String {
    if path.segments().is_empty() {
        return "configuration".to_string();
    }
    let mut text = String::new();
    for segment in path.segments() {
        match segment {
            Segment::Key(key) => {
                if !text.is_empty() {
                    text.push('.');
                }
                text.push_str(key_name(*key));
            }
            Segment::Index(index) => text.push_str(&format!("[{index}]")),
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::{budget, volumes};

    #[test]
    fn every_key_is_spelled_as_one_of_the_schemas_json_names() {
        let mut spelled: Vec<&str> = Key::ALL
            .iter()
            .filter(|key| !key.is_file_setting())
            .map(|key| key_name(*key))
            .collect();
        let mut defined: Vec<&str> = [Profile::Axpc, Profile::Axmc]
            .iter()
            .flat_map(|profile| {
                profile
                    .root_fields()
                    .iter()
                    .chain(profile.entry_fields())
                    .copied()
            })
            .chain(budget::BUDGET_FIELDS)
            .chain(volumes::VOLUME_FIELDS)
            .collect();
        spelled.sort_unstable();
        defined.sort_unstable();
        defined.dedup();
        assert_eq!(spelled, defined);
    }

    #[test]
    fn a_path_reads_as_keys_and_list_positions_and_the_root_as_configuration() {
        let pool = KeyPath::root().key(Key::Repositories).index(2);
        assert_eq!(
            path_text(&pool.key(Key::Volumes).index(0).key(Key::MountPath)),
            "repositories[2].volumes[0].mountPath"
        );
        assert_eq!(path_text(&KeyPath::root()), "configuration");
    }
}
