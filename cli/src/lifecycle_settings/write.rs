//! the write of one `[lifecycle]` setting to `config.toml` through the configuration's
//! write path, which keeps the rest of the file as it is.

use std::fmt;
use std::path::Path;

use crate::edit::{self, Edit, EditError};
use crate::profile::Profile;
use crate::validate::{Failure, Key, KeyPath};

use super::Setting;

/// why a setting was not written. the file is unchanged in every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// the file changed after the agent read it.
    Changed,
    /// the file has problems, listed as the configuration's rules list them.
    Invalid(Failure),
    /// the file could not be read, or the new one could not be written or put in its
    /// place.
    Io(String),
    /// the file is written in a form the write path does not edit, such as
    /// `lifecycle = { … }` over several lines; only a change in a text editor helps.
    FileForm(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed => f.write_str("config.toml changed since Marshal read it"),
            Self::Invalid(failure) => {
                write!(f, "{}", EditError::Invalid(failure.clone()))
            }
            Self::Io(text) => f.write_str(text),
            Self::FileForm(reason) => write!(
                f,
                "Marshal cannot change config.toml as it is written ({reason}); change the setting in a text editor"
            ),
        }
    }
}

/// the edits that give `setting` the value `value`, or leave it unset.
#[must_use]
pub fn edits(setting: Setting, value: Option<bool>) -> Vec<Edit> {
    let table = KeyPath::root().key(Key::Lifecycle);
    let path = table.key(setting.key());
    match value {
        Some(on) => vec![
            Edit::CreateTable { path: table },
            Edit::Set {
                path,
                value: edit::Value::Bool(on),
            },
        ],
        None => vec![Edit::Remove { path }],
    }
}

/// the refusal a write-path error means for a setting. the edits name only places
/// every valid file has, so a write path that finds them inapplicable refuses the
/// file's form, not the edit.
#[must_use]
pub fn refusal(error: EditError) -> Refusal {
    match error {
        EditError::Changed => Refusal::Changed,
        EditError::Invalid(failure) => Refusal::Invalid(failure),
        error @ EditError::Io { .. } => Refusal::Io(error.to_string()),
        EditError::Inapplicable(reason) => Refusal::FileForm(reason),
    }
}

/// writes `value` to `setting` in the file at `path`, which is valid for `host`.
pub fn write(
    path: &Path,
    host: Profile,
    checkout: Option<&Path>,
    setting: Setting,
    value: Option<bool>,
) -> Result<(), Refusal> {
    edit::edit_config(path, host, checkout, &edits(setting, value)).map_err(refusal)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::edit::rewrite;

    const POOL: &str = "\n[[pool]]\nname = \"alpha\"\nrepository = \"octo/hello\"\nrunners = 1\n";

    fn file(lifecycle: &str) -> String {
        format!(
            "schema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"i:1\"\n{lifecycle}{POOL}"
        )
    }

    fn apply(text: &str, setting: Setting, value: Option<bool>) -> Result<String, Refusal> {
        rewrite(text.as_bytes(), Profile::Axpc, None, &edits(setting, value))
            .map(|bytes| String::from_utf8(bytes).unwrap())
            .map_err(refusal)
    }

    #[test]
    fn a_setting_is_written_into_a_missing_table_and_keeps_the_rest() {
        let before = file("# keep this\n");
        let after = apply(&before, Setting::StopOnNetworkChange, Some(false)).unwrap();
        assert_eq!(
            after,
            before.replace(
                "# keep this\n",
                "\n[lifecycle]\nstop_on_network_change = false\n# keep this\n"
            )
        );
    }

    #[test]
    fn a_set_value_changes_in_place_and_unsetting_removes_only_its_line() {
        let before =
            file("\n[lifecycle]\nkeep_awake = true # stay awake\nresume_after_stop = true\n");
        assert_eq!(
            apply(&before, Setting::KeepAwake, Some(false)).unwrap(),
            before.replace("keep_awake = true #", "keep_awake = false #")
        );
        assert_eq!(
            apply(&before, Setting::ResumeAfterStop, None).unwrap(),
            before.replace("resume_after_stop = true\n", "")
        );
        assert_eq!(apply(&before, Setting::StartAtLogin, None).unwrap(), before);
    }

    #[test]
    fn a_dotted_lifecycle_and_a_one_line_inline_table_are_edited() {
        let dotted = file("lifecycle.keep_awake = true\n");
        let edited = apply(&dotted, Setting::KeepAwake, Some(false)).unwrap();
        assert_eq!(
            edited,
            dotted.replace("keep_awake = true", "keep_awake = false")
        );

        let inline = file("lifecycle = { keep_awake = true }\n");
        let edited = apply(&inline, Setting::StartAtLogin, Some(true)).unwrap();
        assert!(edited.contains("start_at_login = true"));
        assert!(edited.contains("keep_awake = true"));
        assert!(!edited.contains("[lifecycle]"));
    }

    #[test]
    fn a_lifecycle_inline_table_over_several_lines_is_refused_as_the_files_form() {
        let before = file("lifecycle = {\n  keep_awake = true,\n}\n");
        match apply(&before, Setting::KeepAwake, Some(false)) {
            Err(Refusal::FileForm(reason)) => {
                assert!(reason.contains("several lines"), "{reason}");
                let shown = Refusal::FileForm(reason).to_string();
                assert!(shown.contains("text editor"), "{shown}");
            }
            other => panic!("expected a refusal of the file's form, got {other:?}"),
        }
    }

    #[test]
    fn a_file_with_problems_is_refused_as_invalid_not_as_its_form() {
        let before = file("\n[lifecycle]\nkeep_awake = \"yes\"\n");
        assert!(matches!(
            apply(&before, Setting::StartAtLogin, Some(true)),
            Err(Refusal::Invalid(_))
        ));
    }
}
