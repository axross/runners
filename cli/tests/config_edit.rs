//! changes to a `config.toml` that keep what they leave alone: comments, blank lines,
//! key order, the line ending, the byte-order mark and the comment on a changed
//! value's line; and the refusals that leave the file as it was.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use marshal_cli::edit::{
    ConfigFile, Edit, EditError, Operation, Value, edit_config, remove_leftover_temporary_files,
    rewrite,
};
use marshal_cli::profile::Profile;
use marshal_cli::validate::{Failure, Key, KeyPath, validate_toml};
use support::TempDir;

const SAMPLE: &str = "\
# Marshal configuration for this host.
schema_version = 1
routing_label = \"axpc\" # the label the old profile implied
image_name = \"ghcr.io/octo/runner:1\"

# the first pool
[[pool]]
name = \"alpha\"
repository = \"octo/hello\"
runners = 2 # two at a time
memory_gb = 12 # the host has plenty
labels = [\"gpu\", \"fast\"] # picked by the workflows

# the cache volume
[[pool.volumes]]
suffix = \"cache\"
mount_path = \"/var/cache\"

[[pool.volumes]]
suffix = \"work\"
mount_path = \"/work\"

# the second pool, which is small
[[pool]]
name = \"beta\"
repository = \"octo/world\"
# the cpus it may use
cpus = 1.5
runners = 1

# end of the file
";

const LF: &str = "\n";
const CRLF: &str = "\r\n";
const MARK: &str = "\u{feff}";

fn with_ending(text: &str, ending: &str) -> String {
    text.replace('\n', ending)
}

fn path(parts: &[Part]) -> KeyPath {
    parts.iter().fold(KeyPath::root(), |path, part| match part {
        Part::K(key) => path.key(*key),
        Part::I(index) => path.index(*index),
    })
}

enum Part {
    K(Key),
    I(usize),
}

use Part::{I, K};

fn pool(index: usize, key: Key) -> KeyPath {
    path(&[K(Key::Repositories), I(index), K(key)])
}

fn set(path: KeyPath, value: Value) -> Edit {
    Edit::Set { path, value }
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

struct Config {
    dir: TempDir,
    path: PathBuf,
}

impl Config {
    fn new(contents: &str) -> Self {
        let dir = TempDir::new();
        let path = dir.write("config.toml", contents.as_bytes());
        Self { dir, path }
    }

    fn edit(&self, edits: &[Edit]) -> Result<(), EditError> {
        edit_config(&self.path, Profile::Axpc, None, edits)
    }

    fn read(&self) -> String {
        String::from_utf8(fs::read(&self.path).unwrap()).unwrap()
    }

    fn assert_nothing_left_beside_the_file(&self) {
        assert_eq!(names(self.dir.path()), ["config.toml"]);
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// the lines that `before` has and `after` lacks, and the lines that `after` has and
/// `before` lacks, once their common first and last lines are set aside.
fn changed_lines(before: &str, after: &str) -> (Vec<String>, Vec<String>) {
    let before: Vec<&str> = before.split_inclusive('\n').collect();
    let after: Vec<&str> = after.split_inclusive('\n').collect();
    let head = before
        .iter()
        .zip(&after)
        .take_while(|(left, right)| left == right)
        .count();
    let tail = before[head..]
        .iter()
        .rev()
        .zip(after[head..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let own = |lines: &[&str], tail: usize| {
        lines[head..lines.len() - tail]
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    (own(&before, tail), own(&after, tail))
}

fn lines(ending: &str, lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("{line}{ending}")).collect()
}

/// `SAMPLE` with `edit` applied to its text, in the file's line ending.
fn sample_where(ending: &str, edit: impl FnOnce(&str) -> String) -> String {
    with_ending(&edit(SAMPLE), ending)
}

fn volume(suffix: &str, mount_path: &str) -> Vec<(Key, Value)> {
    vec![
        (Key::Suffix, text(suffix)),
        (Key::MountPath, text(mount_path)),
    ]
}

fn gamma() -> Vec<(Key, Value)> {
    vec![
        (Key::Name, text("gamma")),
        (Key::Repository, text("octo/gamma")),
        (Key::Slots, Value::Integer(1)),
    ]
}

fn volumes_of(pool_index: usize) -> KeyPath {
    path(&[K(Key::Repositories), I(pool_index), K(Key::Volumes)])
}

#[test]
fn the_sample_is_a_valid_file() {
    validate_toml(SAMPLE.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
}

#[test]
fn setting_a_value_changes_only_its_line_and_keeps_the_comment_on_it() {
    for ending in [LF, CRLF] {
        let before = with_ending(SAMPLE, ending);
        let config = Config::new(&before);
        config
            .edit(&[set(pool(0, Key::MemoryGb), Value::Integer(16))])
            .unwrap();
        assert_eq!(
            changed_lines(&before, &config.read()),
            (
                lines(ending, &["memory_gb = 12 # the host has plenty"]),
                lines(ending, &["memory_gb = 16 # the host has plenty"])
            )
        );
        config.assert_nothing_left_beside_the_file();
    }
}

#[test]
fn setting_an_array_changes_only_its_line_and_keeps_the_comment_on_it() {
    for ending in [LF, CRLF] {
        let before = with_ending(SAMPLE, ending);
        let config = Config::new(&before);
        let labels = Value::Strings(vec!["gpu".into(), "big".into(), "arm".into()]);
        config.edit(&[set(pool(0, Key::Labels), labels)]).unwrap();
        assert_eq!(
            changed_lines(&before, &config.read()),
            (
                lines(
                    ending,
                    &["labels = [\"gpu\", \"fast\"] # picked by the workflows"]
                ),
                lines(
                    ending,
                    &["labels = [\"gpu\", \"big\", \"arm\"] # picked by the workflows"]
                )
            )
        );
    }
}

#[test]
fn every_kind_of_value_can_be_set() {
    let config = Config::new(SAMPLE);
    config
        .edit(&[
            set(pool(0, Key::Labels), Value::Strings(Vec::new())),
            set(pool(0, Key::Name), text("alpha-two")),
            set(pool(1, Key::Cpus), Value::Float(2.5)),
            set(pool(1, Key::MemoryGb), Value::Integer(12)),
            set(pool(1, Key::Diagnostics), Value::Bool(false)),
        ])
        .unwrap();
    let after = config.read();
    assert!(after.contains("labels = [] # picked by the workflows\n"));
    assert!(after.contains("name = \"alpha-two\"\n"));
    assert!(after.contains(
        "# the cpus it may use\ncpus = 2.5\nrunners = 1\nmemory_gb = 12\ndiagnostics = false\n"
    ));
}

#[test]
fn setting_a_value_a_table_lacks_adds_one_line_after_its_last_key() {
    for ending in [LF, CRLF] {
        let before = with_ending(SAMPLE, ending);
        let config = Config::new(&before);
        let labels = Value::Strings(vec!["gpu".into()]);
        config.edit(&[set(pool(1, Key::Labels), labels)]).unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| sample
                .replace("runners = 1\n", "runners = 1\nlabels = [\"gpu\"]\n"))
        );
    }
}

#[test]
fn removing_a_value_changes_only_its_line_and_the_comment_on_it() {
    for ending in [LF, CRLF] {
        let before = with_ending(SAMPLE, ending);
        let config = Config::new(&before);
        config
            .edit(&[Edit::Remove {
                path: pool(0, Key::MemoryGb),
            }])
            .unwrap();
        assert_eq!(
            changed_lines(&before, &config.read()),
            (
                lines(ending, &["memory_gb = 12 # the host has plenty"]),
                Vec::new()
            )
        );
        config.assert_nothing_left_beside_the_file();
    }
}

#[test]
fn removing_a_value_leaves_the_comment_lines_above_it() {
    for ending in [LF, CRLF] {
        let before = with_ending(SAMPLE, ending);
        let config = Config::new(&before);
        config
            .edit(&[Edit::Remove {
                path: pool(1, Key::Cpus),
            }])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| sample.replace("cpus = 1.5\n", ""))
        );
    }
}

#[test]
fn removing_a_value_that_is_not_there_writes_nothing() {
    let config = Config::new(SAMPLE);
    let identity = file_identity(&config.path);
    config
        .edit(&[Edit::Remove {
            path: pool(1, Key::Labels),
        }])
        .unwrap();
    assert_eq!(config.read(), SAMPLE);
    assert_eq!(file_identity(&config.path), identity);
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn creating_lifecycle_adds_the_table_before_the_first_pool() {
    for ending in [LF, CRLF] {
        let config = Config::new(&with_ending(SAMPLE, ending));
        config
            .edit(&[
                Edit::CreateTable {
                    path: path(&[K(Key::Lifecycle)]),
                },
                set(
                    path(&[K(Key::Lifecycle), K(Key::KeepAwake)]),
                    Value::Bool(true),
                ),
            ])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| sample.replace(
                "\n# the first pool\n",
                "\n[lifecycle]\nkeep_awake = true\n\n# the first pool\n"
            ))
        );
    }
}

#[test]
fn creating_an_empty_table_and_one_that_exists() {
    let config = Config::new(SAMPLE);
    let create = Edit::CreateTable {
        path: path(&[K(Key::Lifecycle)]),
    };
    config.edit(std::slice::from_ref(&create)).unwrap();
    assert_eq!(
        config.read(),
        SAMPLE.replace(
            "\n# the first pool\n",
            "\n[lifecycle]\n\n# the first pool\n"
        )
    );
    let created = config.read();
    config.edit(&[create]).unwrap();
    assert_eq!(config.read(), created);
}

#[test]
fn a_table_is_created_before_the_pools_when_a_budget_comes_first() {
    let before = "\
schema_version = 1
routing_label = \"axmc\"

[budget]
cpus = 4
memory_gb = 16
disk_gb = 100

[[pool]]
name = \"alpha\"
repository = \"octo/hello\"
runners = 1
disk_gb = 40
";
    let after = rewrite(
        before.as_bytes(),
        Profile::Axmc,
        None,
        &[Edit::CreateTable {
            path: path(&[K(Key::Lifecycle)]),
        }],
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(after).unwrap(),
        before.replace("\n[[pool]]\n", "\n[lifecycle]\n\n[[pool]]\n")
    );
}

#[test]
fn adding_a_pool_keeps_everything_else_and_uses_the_files_line_ending() {
    for ending in [LF, CRLF] {
        let config = Config::new(&with_ending(SAMPLE, ending));
        config
            .edit(&[Edit::AddEntry {
                array: path(&[K(Key::Repositories)]),
                values: gamma(),
            }])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| {
                sample.replace(
                "runners = 1\n\n# end",
                "runners = 1\n\n[[pool]]\nname = \"gamma\"\nrepository = \"octo/gamma\"\nrunners = 1\n\n# end"
            )
            })
        );
        config.assert_nothing_left_beside_the_file();
    }
}

#[test]
fn adding_a_volume_keeps_everything_else_and_uses_the_files_line_ending() {
    for ending in [LF, CRLF] {
        let config = Config::new(&with_ending(SAMPLE, ending));
        config
            .edit(&[
                Edit::AddEntry {
                    array: volumes_of(0),
                    values: volume("tools", "/opt/tools"),
                },
                Edit::AddEntry {
                    array: volumes_of(1),
                    values: volume("scratch", "/scratch"),
                },
            ])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| {
                sample
                .replace(
                    "mount_path = \"/work\"\n",
                    "mount_path = \"/work\"\n\n[[pool.volumes]]\nsuffix = \"tools\"\nmount_path = \"/opt/tools\"\n"
                )
                .replace(
                    "runners = 1\n",
                    "runners = 1\n\n[[pool.volumes]]\nsuffix = \"scratch\"\nmount_path = \"/scratch\"\n"
                )
            })
        );
    }
}

#[test]
fn removing_a_pool_takes_its_volumes_and_the_comments_directly_above_it() {
    for ending in [LF, CRLF] {
        let config = Config::new(&with_ending(SAMPLE, ending));
        config
            .edit(&[Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(0)]),
            }])
            .unwrap();
        let first_pool = SAMPLE.split("# the second pool").next().unwrap();
        let first_pool = first_pool.split("# the first pool\n").nth(1).unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| {
                sample.replace(&format!("# the first pool\n{first_pool}"), "")
            })
        );
        config.assert_nothing_left_beside_the_file();
    }
}

#[test]
fn removing_the_last_pool_takes_the_comment_above_it_and_keeps_the_end_of_the_file() {
    for ending in [LF, CRLF] {
        let config = Config::new(&with_ending(SAMPLE, ending));
        config
            .edit(&[Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(1)]),
            }])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| {
                sample.replace(
                "\n# the second pool, which is small\n[[pool]]\nname = \"beta\"\nrepository = \"octo/world\"\n# the cpus it may use\ncpus = 1.5\nrunners = 1\n",
                ""
            )
            })
        );
    }
}

#[test]
fn removing_a_volume_takes_the_comment_directly_above_it() {
    for ending in [LF, CRLF] {
        let config = Config::new(&with_ending(SAMPLE, ending));
        config
            .edit(&[Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(0), K(Key::Volumes), I(0)]),
            }])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| {
                sample.replace(
                "\n# the cache volume\n[[pool.volumes]]\nsuffix = \"cache\"\nmount_path = \"/var/cache\"\n",
                ""
            )
            })
        );
        let config = Config::new(&with_ending(SAMPLE, ending));
        config
            .edit(&[Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(0), K(Key::Volumes), I(1)]),
            }])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| sample.replace(
                "\n[[pool.volumes]]\nsuffix = \"work\"\nmount_path = \"/work\"\n",
                ""
            ))
        );
    }
}

#[test]
fn a_comment_set_apart_from_a_removed_entry_by_a_blank_line_stays() {
    let before = SAMPLE.replace(
        "# the second pool, which is small\n",
        "# pools that are small\n\n# the second pool\n",
    );
    let config = Config::new(&before);
    config
        .edit(&[Edit::RemoveEntry {
            path: path(&[K(Key::Repositories), I(1)]),
        }])
        .unwrap();
    assert_eq!(
        config.read(),
        before.replace(
            "# the second pool\n[[pool]]\nname = \"beta\"\nrepository = \"octo/world\"\n# the cpus it may use\ncpus = 1.5\nrunners = 1\n\n",
            ""
        )
    );
}

fn bytes_of(config: &Config) -> Vec<u8> {
    fs::read(&config.path).unwrap()
}

fn problems(error: EditError) -> Vec<String> {
    match error {
        EditError::Invalid(Failure::Invalid(problems)) => problems,
        other => panic!("not an invalid document: {other:?}"),
    }
}

#[test]
fn a_result_that_breaks_the_rules_is_refused_with_its_problems_and_the_file_stays() {
    for ending in [LF, CRLF] {
        let before = with_ending(SAMPLE, ending);
        let config = Config::new(&before);
        let error = config
            .edit(&[
                set(pool(0, Key::Slots), Value::Integer(0)),
                set(pool(1, Key::TokenPath), text("C:\\token")),
            ])
            .unwrap_err();
        assert_eq!(
            problems(error),
            [
                "line 10: pool[0].runners: must be an integer from 1 to 16",
                "line 30: pool[1].token_path: unknown key",
            ]
        );
        assert_eq!(bytes_of(&config), before.as_bytes());
        config.assert_nothing_left_beside_the_file();
    }
}

#[test]
fn removing_every_pool_is_refused() {
    let config = Config::new(SAMPLE);
    let error = config
        .edit(&[
            Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(0)]),
            },
            Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(0)]),
            },
        ])
        .unwrap_err();
    assert_eq!(problems(error), ["line 1: pool: required key is missing"]);
    assert_eq!(config.read(), SAMPLE);
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn the_host_decides_which_result_is_valid() {
    let config = Config::new(SAMPLE);
    let error = edit_config(
        &config.path,
        Profile::Axmc,
        None,
        &[set(pool(0, Key::Slots), Value::Integer(1))],
    )
    .unwrap_err();
    assert!(matches!(error, EditError::Invalid(_)), "{error:?}");
    assert_eq!(config.read(), SAMPLE);
}

#[test]
fn a_file_that_is_not_a_toml_document_is_refused_without_quoting_it() {
    const TOKEN: &str = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
    let before = format!("schema_version = 1\nrouting_label = {TOKEN}\n");
    let config = Config::new(&before);
    let error = config
        .edit(&[set(pool(0, Key::Slots), Value::Integer(1))])
        .unwrap_err();
    let EditError::Invalid(Failure::Unreadable(problem)) = &error else {
        panic!("{error:?}");
    };
    assert!(problem.starts_with("line 2, column "), "{problem}");
    assert!(!error.to_string().contains(TOKEN));
    assert_eq!(config.read(), before);
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn a_file_that_is_not_utf8_or_is_over_the_size_limit_is_refused() {
    let config = Config::new("schema_version = 1\n");
    fs::write(&config.path, b"routing_label = \"\xff\"\n").unwrap();
    let error = config
        .edit(&[set(pool(0, Key::Slots), Value::Integer(1))])
        .unwrap_err();
    assert!(error.to_string().contains("not valid UTF-8"), "{error}");

    let big = format!("# {}\n", "x".repeat(1024 * 1024));
    fs::write(&config.path, &big).unwrap();
    let error = config
        .edit(&[set(pool(0, Key::Slots), Value::Integer(1))])
        .unwrap_err();
    assert!(error.to_string().contains("larger than"), "{error}");
    assert_eq!(config.read(), big);
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn a_file_that_changed_after_it_was_read_is_refused_and_left_as_it_is() {
    let config = Config::new(SAMPLE);
    let read = ConfigFile::read(&config.path).unwrap();
    assert_eq!(read.bytes(), SAMPLE.as_bytes());
    let theirs = SAMPLE.replace("octo/hello", "octo/theirs");
    fs::write(&config.path, &theirs).unwrap();

    let error = read
        .edit(
            Profile::Axpc,
            None,
            &[set(pool(0, Key::MemoryGb), Value::Integer(16))],
        )
        .unwrap_err();

    assert_eq!(error, EditError::Changed);
    assert_eq!(config.read(), theirs);
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn a_file_that_is_gone_after_it_was_read_is_refused_as_changed() {
    let config = Config::new(SAMPLE);
    let read = ConfigFile::read(&config.path).unwrap();
    fs::remove_file(&config.path).unwrap();
    let error = read
        .edit(
            Profile::Axpc,
            None,
            &[set(pool(0, Key::MemoryGb), Value::Integer(16))],
        )
        .unwrap_err();
    assert_eq!(error, EditError::Changed);
    assert!(names(config.dir.path()).is_empty());
}

#[test]
fn a_missing_file_and_a_folder_that_vanished_are_file_errors() {
    let dir = TempDir::new();
    let missing = dir.path().join("config.toml");
    let error = edit_config(&missing, Profile::Axpc, None, &[]).unwrap_err();
    assert_eq!(
        error,
        EditError::Io {
            path: missing,
            operation: Operation::Read,
            kind: std::io::ErrorKind::NotFound,
        }
    );

    let config = Config::new(SAMPLE);
    let read = ConfigFile::read(&config.path).unwrap();
    fs::remove_dir_all(config.dir.path()).unwrap();
    let error = read
        .edit(
            Profile::Axpc,
            None,
            &[set(pool(0, Key::MemoryGb), Value::Integer(16))],
        )
        .unwrap_err();
    assert_eq!(
        error,
        EditError::Io {
            path: config.path.clone(),
            operation: Operation::Write,
            kind: std::io::ErrorKind::NotFound,
        }
    );
}

#[cfg(windows)]
#[test]
fn a_file_an_editor_holds_open_is_not_replaced_and_no_temporary_file_stays() {
    use std::os::windows::fs::OpenOptionsExt;
    const SHARE_READ_AND_WRITE: u32 = 3;
    let config = Config::new(SAMPLE);
    let _held = fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_AND_WRITE)
        .open(&config.path)
        .unwrap();
    let error = config
        .edit(&[set(pool(0, Key::MemoryGb), Value::Integer(16))])
        .unwrap_err();
    assert!(
        matches!(
            error,
            EditError::Io {
                operation: Operation::Replace,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(config.read(), SAMPLE);
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn an_edit_that_names_a_place_the_document_lacks_is_refused_and_the_file_stays() {
    let config = Config::new(SAMPLE);
    for edit in [
        set(pool(5, Key::Slots), Value::Integer(1)),
        set(
            path(&[K(Key::Lifecycle), K(Key::KeepAwake)]),
            Value::Bool(true),
        ),
        set(path(&[K(Key::Repositories)]), Value::Integer(1)),
        set(path(&[K(Key::Repositories), I(0)]), Value::Integer(1)),
        set(path(&[I(0), K(Key::Name)]), text("x")),
        Edit::Remove {
            path: path(&[K(Key::Repositories)]),
        },
        Edit::CreateTable {
            path: path(&[K(Key::ImageName)]),
        },
        Edit::AddEntry {
            array: path(&[K(Key::ImageName)]),
            values: gamma(),
        },
        Edit::RemoveEntry {
            path: path(&[K(Key::Repositories), I(9)]),
        },
        Edit::RemoveEntry {
            path: pool(0, Key::Name),
        },
    ] {
        let error = config.edit(std::slice::from_ref(&edit)).unwrap_err();
        assert!(
            matches!(error, EditError::Inapplicable(_)),
            "{edit:?}: {error:?}"
        );
        assert_eq!(config.read(), SAMPLE);
    }
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn an_edit_that_cannot_apply_names_itself_without_the_files_text() {
    let config = Config::new(SAMPLE);
    let error = config
        .edit(&[set(pool(5, Key::Slots), Value::Integer(1))])
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "cannot set pool[5].runners: an entry on the way is missing"
    );
}

#[test]
fn a_crlf_file_with_a_byte_order_mark_keeps_both_and_the_comment_on_the_changed_line() {
    let before = format!(
        "{MARK}{}",
        "# host\r\nschema_version = 1\r\nrouting_label = \"axpc\"\r\nimage_name = \"i:1\"\r\n\r\n[[pool]]\r\nname = \"a\"\r\nrepository = \"o/r\"\r\nrunners = 2 # two at a time\r\n"
    );
    let config = Config::new(&before);
    config
        .edit(&[set(pool(0, Key::Slots), Value::Integer(3))])
        .unwrap();
    let after = bytes_of(&config);
    assert_eq!(
        after,
        before
            .replace("runners = 2 #", "runners = 3 #")
            .into_bytes()
    );
    assert!(after.starts_with(&[0xef, 0xbb, 0xbf]));
    config.assert_nothing_left_beside_the_file();
}

/// the sample with its first `minority_lines` lines ended by `minority` and the rest by
/// `majority`.
fn mixed(majority: &str, minority: &str, minority_lines: usize) -> String {
    SAMPLE
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| {
            let ending = if index < minority_lines {
                minority
            } else {
                majority
            };
            format!("{}{ending}", line.trim_end_matches('\n'))
        })
        .collect()
}

#[test]
fn a_file_with_mixed_endings_is_written_with_its_most_common_one() {
    for (before, ending) in [(mixed(CRLF, LF, 3), CRLF), (mixed(LF, CRLF, 3), LF)] {
        let config = Config::new(&before);
        config
            .edit(&[
                set(pool(0, Key::MemoryGb), Value::Integer(16)),
                Edit::AddEntry {
                    array: path(&[K(Key::Repositories)]),
                    values: gamma(),
                },
            ])
            .unwrap();
        assert_eq!(
            config.read(),
            sample_where(ending, |sample| {
                sample
                .replace("memory_gb = 12", "memory_gb = 16")
                .replace(
                    "runners = 1\n\n# end",
                    "runners = 1\n\n[[pool]]\nname = \"gamma\"\nrepository = \"octo/gamma\"\nrunners = 1\n\n# end"
                )
            })
        );
    }
}

#[test]
fn a_file_whose_last_line_is_not_ended_stays_so_when_another_value_changes() {
    let before = SAMPLE.trim_end_matches('\n').to_string();
    let config = Config::new(&before);
    config
        .edit(&[set(pool(0, Key::MemoryGb), Value::Integer(16))])
        .unwrap();
    assert_eq!(
        config.read(),
        before.replace("memory_gb = 12", "memory_gb = 16")
    );
}

#[test]
fn a_change_to_nothing_but_the_ending_of_the_file_leaves_it_alone() {
    let before = with_ending(SAMPLE, CRLF);
    let config = Config::new(&before);
    config
        .edit(&[set(pool(0, Key::MemoryGb), Value::Integer(12))])
        .unwrap();
    assert_eq!(config.read(), before);
}

fn fixtures(directory: &str) -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/host-configuration/toml")
        .join(directory);
    let mut found: Vec<(String, Vec<u8>)> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .map(|path| {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            (name, fs::read(path).unwrap())
        })
        .collect();
    found.sort();
    found
}

fn host_of(fixture: &str) -> Profile {
    if fixture.starts_with("macos-") {
        Profile::Axmc
    } else {
        Profile::Axpc
    }
}

#[test]
fn an_edit_that_changes_nothing_gives_back_every_accepted_fixture_byte_for_byte() {
    let noop = [Edit::Remove {
        path: path(&[K(Key::DiagnosticDirectory)]),
    }];
    let fixtures = fixtures("accepted");
    assert!(fixtures.len() >= 8);
    for (name, bytes) in fixtures {
        let text = String::from_utf8(bytes).unwrap();
        let variants = [
            text.clone(),
            with_ending(&text, CRLF),
            format!("{MARK}{text}"),
            format!("{MARK}{}", with_ending(&text, CRLF)),
            text.trim_end_matches('\n').to_string(),
        ];
        for variant in variants {
            let output = rewrite(variant.as_bytes(), host_of(&name), None, &noop)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(output, variant.as_bytes(), "{name}");
        }
    }
}

#[test]
fn the_rewrite_of_a_fixture_is_a_valid_file_with_the_change_in_its_plan() {
    let (_, bytes) = fixtures("accepted")
        .into_iter()
        .find(|(name, _)| name == "windows-full.toml")
        .unwrap();
    let output = rewrite(
        &bytes,
        Profile::Axpc,
        None,
        &[
            set(
                path(&[K(Key::Lifecycle), K(Key::KeepAwake)]),
                Value::Bool(false),
            ),
            Edit::AddEntry {
                array: path(&[K(Key::Repositories)]),
                values: gamma(),
            },
            Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(0), K(Key::Volumes), I(0)]),
            },
        ],
    )
    .unwrap();
    let plan = validate_toml(&output, Profile::Axpc, None).outcome.unwrap();
    assert_eq!(plan.entries.len(), 3);
    assert_eq!(plan.entries[0].volumes.len(), 1);
    assert_eq!(plan.settings.unwrap().lifecycle.keep_awake, Some(false));
}

#[test]
fn values_and_entries_written_inline_are_edited_in_place() {
    let before = "\
schema_version = 1
routing_label = 'axpc'
image_name = \"example-image:tag\"
pool = [
  { name = \"alpha\", repository = \"octo/hello\", runners = 0x2, cpus = 2 },
  { name = \"beta\", repository = \"octo/other\", runners = 1, volumes = [{ suffix = \"cache\", mount_path = \"/cache\" }] },
]
";
    let edited = |edits: &[Edit]| {
        String::from_utf8(rewrite(before.as_bytes(), Profile::Axpc, None, edits).unwrap()).unwrap()
    };
    assert_eq!(
        edited(&[set(pool(0, Key::Cpus), Value::Integer(3))]),
        before.replace("cpus = 2 }", "cpus = 3 }")
    );
    assert_eq!(
        edited(&[Edit::Remove {
            path: pool(0, Key::Cpus)
        }]),
        before.replace(", cpus = 2 }", " }")
    );
    assert_eq!(
        edited(&[set(
            path(&[
                K(Key::Repositories),
                I(1),
                K(Key::Volumes),
                I(0),
                K(Key::Suffix)
            ]),
            text("tools")
        )]),
        before.replace("suffix = \"cache\"", "suffix = \"tools\"")
    );
    let added = edited(&[Edit::AddEntry {
        array: volumes_of(1),
        values: volume("tools", "/tools"),
    }]);
    assert!(
        added.contains("{ suffix = \"cache\", mount_path = \"/cache\" }, { suffix = \"tools\", mount_path = \"/tools\" }"),
        "{added}"
    );
    let removed = edited(&[Edit::RemoveEntry {
        path: path(&[K(Key::Repositories), I(0)]),
    }]);
    assert!(!removed.contains("\"alpha\""), "{removed}");
    assert!(removed.contains("\"beta\""), "{removed}");
}

const DOTTED: &str = "\
schema_version = 1
routing_label = \"axpc\"
image_name = \"i:1\"
lifecycle.keep_awake = true # stay awake
lifecycle.resume_after_stop = false

[[pool]]
name = \"alpha\"
repository = \"octo/hello\"
runners = 1
";

#[test]
fn a_dotted_key_is_set_and_removed_on_its_own_line() {
    let lifecycle = |key| path(&[K(Key::Lifecycle), K(key)]);
    let edited = |edit: Edit| {
        let bytes = rewrite(DOTTED.as_bytes(), Profile::Axpc, None, &[edit]).unwrap();
        String::from_utf8(bytes).unwrap()
    };
    assert_eq!(
        edited(set(lifecycle(Key::KeepAwake), Value::Bool(false))),
        DOTTED.replace("keep_awake = true #", "keep_awake = false #")
    );
    assert_eq!(
        edited(Edit::Remove {
            path: lifecycle(Key::KeepAwake)
        }),
        DOTTED.replace("lifecycle.keep_awake = true # stay awake\n", "")
    );
    assert_eq!(
        edited(Edit::Remove {
            path: lifecycle(Key::ResumeAfterStop)
        }),
        DOTTED.replace("lifecycle.resume_after_stop = false\n", "")
    );
}

#[test]
fn several_edits_are_one_write() {
    let config = Config::new(SAMPLE);
    config
        .edit(&[
            Edit::CreateTable {
                path: path(&[K(Key::Lifecycle)]),
            },
            set(
                path(&[K(Key::Lifecycle), K(Key::StartAtLogin)]),
                Value::Bool(true),
            ),
            Edit::AddEntry {
                array: path(&[K(Key::Repositories)]),
                values: gamma(),
            },
            set(pool(2, Key::Labels), Value::Strings(vec!["gpu".into()])),
            Edit::RemoveEntry {
                path: path(&[K(Key::Repositories), I(0)]),
            },
        ])
        .unwrap();
    let plan = validate_toml(config.read().as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    let names: Vec<_> = plan
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["beta", "gamma"]);
    assert_eq!(plan.settings.unwrap().lifecycle.start_at_login, Some(true));
    config.assert_nothing_left_beside_the_file();
}

#[test]
fn the_start_up_clean_up_removes_what_an_interrupted_write_left_and_nothing_else() {
    let config = Config::new(SAMPLE);
    let leftover = config
        .dir
        .write("config.toml.0123456789abcdef.tmp", b"half a fi");
    let other = config.dir.write("config.toml.bak", b"mine");
    let cleanup = remove_leftover_temporary_files(config.dir.path()).unwrap();
    assert_eq!(cleanup.removed, std::slice::from_ref(&leftover));
    assert!(cleanup.failed.is_empty());
    assert!(!leftover.exists());
    assert_eq!(fs::read(other).unwrap(), b"mine");
    assert_eq!(config.read(), SAMPLE);
}

#[test]
fn a_list_written_over_several_lines_is_replaced_and_keeps_the_comment_after_it() {
    let before = SAMPLE.replace(
        "labels = [\"gpu\", \"fast\"] # picked by the workflows\n",
        "labels = [\n  \"gpu\", # the gpu pool\n  \"fast\",\n] # picked by the workflows\n",
    );
    let after = rewrite(
        before.as_bytes(),
        Profile::Axpc,
        None,
        &[set(
            pool(0, Key::Labels),
            Value::Strings(vec!["gpu".into()]),
        )],
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(after).unwrap(),
        SAMPLE.replace("[\"gpu\", \"fast\"]", "[\"gpu\"]")
    );
}

#[test]
fn a_table_is_created_after_the_root_keys_when_the_pools_are_written_inline() {
    let (_, bytes) = fixtures("accepted")
        .into_iter()
        .find(|(name, _)| name == "windows-other-toml-forms.toml")
        .unwrap();
    let before = String::from_utf8(bytes).unwrap();
    let after = rewrite(
        before.as_bytes(),
        Profile::Axpc,
        None,
        &[Edit::CreateTable {
            path: path(&[K(Key::Lifecycle)]),
        }],
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(after).unwrap(),
        format!("{before}\n[lifecycle]\n")
    );
}

#[test]
fn an_edit_that_changes_nothing_leaves_a_file_with_mixed_endings_as_it_is() {
    let before = mixed(CRLF, LF, 3);
    let config = Config::new(&before);
    let identity = file_identity(&config.path);
    config
        .edit(&[set(pool(0, Key::MemoryGb), Value::Integer(12))])
        .unwrap();
    assert_eq!(config.read(), before);
    assert_eq!(file_identity(&config.path), identity);
}

/// what tells one file from another that replaced it: the inode on Unix, nothing
/// elsewhere.
fn file_identity(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(fs::metadata(path).unwrap().ino())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

#[cfg(unix)]
#[test]
fn a_written_file_is_a_new_file_renamed_into_place() {
    let config = Config::new(SAMPLE);
    let identity = file_identity(&config.path);
    config
        .edit(&[set(pool(0, Key::MemoryGb), Value::Integer(16))])
        .unwrap();
    assert_ne!(file_identity(&config.path), identity);
}
