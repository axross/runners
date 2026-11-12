//! the JSON reader's side of validation: today's rules for each host, and the path form
//! and wording that the shared fixtures pin.

use crate::json::Object;
use crate::profile::Profile;

use super::budget::BUDGET_FIELDS;
use super::problem::{Key, KeyPath, Kind, Location, Problem, Segment, Wording, rule_text};
use super::reader::{Reader, Schema};
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
    fn schema(&self) -> Schema<'_> {
        Schema::Json {
            routing_label: self.profile.routing_label(),
        }
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
        let entry_path = |index| self.entry_path(index);
        let wording = Wording {
            host: self.profile.name(),
            key_name,
            entry_path: &entry_path,
        };
        if let Some(text) = rule_text(&problem.kind, &wording) {
            return format!("{path}: {text}");
        }
        match &problem.kind {
            Kind::UnknownField(name) => format!("{path}.{name}: unknown field"),
            Kind::Missing => format!("{path}: required field is missing"),
            Kind::MustBe(what) => format!("{path}: must be {what}"),
            Kind::Unreadable(message) => format!("{path}: {message}"),
            Kind::NoRepositories => format!("{path}: must list at least one repository"),
            Kind::ForeignPlatformLabel { label, .. } => {
                format!("{path}: '{label}' is a platform label, so list only non-platform labels")
            }
            Kind::NewerSchemaVersion => format!("{path}: written by a newer version"),
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
            // rule_text words every other kind
            _ => format!("{path}: {:?}", problem.kind),
        }
    }
}

/// the key as the JSON file spells it, which is also the name the shared tree gives it
/// whatever the format; the keys only a file with settings has are spelled the same
/// way, though no JSON file has them.
pub(crate) fn key_name(key: Key) -> &'static str {
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
    path.text(key_name)
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
