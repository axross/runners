//! the agent's configuration loader: `config.toml` at start and at Start all, the
//! migration of `config.json`, and the create-only write it uses, all in temporary
//! folders with a clock the tests set.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use marshal_cli::convert::ConversionFailure;
use marshal_cli::edit::{CreateOutcome, CreateStep, create_new};
use marshal_cli::host_config::{
    Checks, FileStatus, InUse, Loaded, Migration, MigrationFailure, Origin, StandardChecks,
    load_at_start, load_at_start_all,
};
use marshal_cli::profile::Profile;
use marshal_cli::runtime::clock::Clock;
use marshal_cli::validate::{Validation, validate, validate_toml};
use support::TempDir;

/// 2026-11-04T22:26:51Z.
const SET_TIME: u64 = 1_793_831_211;
const SET_TIME_TEXT: &str = "2026-11-04T22:26:51Z";

/// a token-shaped string that no service issued.
const SECRET: &str = "ghp_MadeUp0123";

const HOST: Profile = Profile::Axpc;

struct SetClock(Cell<u64>);

impl SetClock {
    fn at(seconds: u64) -> Self {
        Self(Cell::new(seconds))
    }

    fn time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.0.get())
    }
}

impl Clock for SetClock {
    fn now(&self) -> SystemTime {
        self.time()
    }

    fn sleep(&self, _duration: Duration) -> impl Future<Output = ()> {
        std::future::ready(())
    }
}

/// what runs just before a conversion, given the path of `config.json`.
type Hook<'a> = Box<dyn Fn(&Path) + 'a>;

/// the library's checks, counting what each is asked and with hooks that force a
/// conversion to fail or run something just before it.
#[derive(Default)]
struct Seam<'a> {
    inner: StandardChecks<'a>,
    toml_checks: Cell<usize>,
    json_checks: Cell<usize>,
    conversions: Cell<usize>,
    fail_conversion: bool,
    during_conversion: Option<Hook<'a>>,
    sources: RefCell<Vec<PathBuf>>,
}

impl Checks for Seam<'_> {
    fn check_toml(&self, bytes: &[u8], profile: Profile) -> Validation {
        self.toml_checks.set(self.toml_checks.get() + 1);
        self.inner.check_toml(bytes, profile)
    }

    fn check_json(&self, bytes: &[u8], profile: Profile) -> Validation {
        self.json_checks.set(self.json_checks.get() + 1);
        self.inner.check_json(bytes, profile)
    }

    fn convert<C: Clock>(
        &self,
        json: &[u8],
        profile: Profile,
        source: &Path,
        clock: &C,
    ) -> Result<String, ConversionFailure> {
        self.conversions.set(self.conversions.get() + 1);
        self.sources.borrow_mut().push(source.to_path_buf());
        if let Some(hook) = &self.during_conversion {
            hook(source);
        }
        if self.fail_conversion {
            return Err(ConversionFailure::NotEquivalent("forced".to_string()));
        }
        self.inner.convert(json, profile, source, clock)
    }
}

fn pool(name: &str, runners: u32) -> String {
    format!("\n[[pool]]\nname = \"{name}\"\nrepository = \"octo/{name}\"\nrunners = {runners}\n")
}

fn toml_with(runners: u32) -> String {
    format!(
        "# edited by hand\nschema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"runner:1\"\n{}",
        pool("alpha", runners)
    )
}

const TOKEN_FILE: &str = "C:\\\\path\\\\to\\\\alpha.token";

fn json_with(runners: u32) -> String {
    format!(
        r#"{{
  "imageName": "runner:1",
  "repositories": [
    {{
      "owner": "octo",
      "repository": "alpha",
      "name": "alpha",
      "slots": {runners},
      "tokenPath": "{TOKEN_FILE}",
      "volumes": []
    }}
  ]
}}
"#
    )
}

/// a folder holding the files the test names.
struct Folder {
    dir: TempDir,
}

impl Folder {
    fn new() -> Self {
        Self {
            dir: TempDir::new(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn toml_path(&self) -> PathBuf {
        self.path().join("config.toml")
    }

    fn json_path(&self) -> PathBuf {
        self.path().join("config.json")
    }

    fn with_toml(self, text: &str) -> Self {
        fs::write(self.toml_path(), text).unwrap();
        self
    }

    fn with_json(self, text: &str) -> Self {
        fs::write(self.json_path(), text).unwrap();
        self
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn start(&self, clock: &SetClock, seam: &Seam) -> Loaded {
        load_at_start(self.path(), HOST, clock, seam)
    }

    fn start_all(&self, clock: &SetClock, seam: &Seam, previous: &Loaded) -> Loaded {
        load_at_start_all(self.path(), HOST, clock, seam, previous)
    }
}

fn in_use(loaded: &Loaded) -> &InUse {
    loaded.in_use.as_ref().expect("a configuration in use")
}

fn runners(loaded: &Loaded) -> u32 {
    in_use(loaded).plan.entries[0].slots
}

fn debug_text(loaded: &Loaded) -> String {
    format!("{loaded:?}")
}

// launch

#[test]
fn launch_with_a_valid_file_uses_it_and_stamps_the_time() {
    let folder = Folder::new().with_toml(&toml_with(2));
    let clock = SetClock::at(SET_TIME);
    let seam = Seam::default();
    let loaded = folder.start(&clock, &seam);
    assert_eq!(loaded.file, FileStatus::Valid);
    assert_eq!(loaded.migration, Migration::None);
    assert!(loaded.problems.is_empty());
    let used = in_use(&loaded);
    assert_eq!(used.origin, Origin::File);
    assert_eq!(used.read_at, clock.time());
    assert_eq!(used.plan.entries.len(), 1);
    assert_eq!(runners(&loaded), 2);
    assert_eq!(seam.toml_checks.get(), 1);
}

#[test]
fn launch_with_neither_file_has_nothing() {
    let folder = Folder::new();
    let loaded = folder.start(&SetClock::at(SET_TIME), &Seam::default());
    assert_eq!(
        loaded,
        Loaded {
            file: FileStatus::Missing,
            in_use: None,
            migration: Migration::None,
            problems: Vec::new(),
        }
    );
    assert!(folder.names().is_empty());
}

#[test]
fn launch_with_a_missing_folder_has_nothing() {
    let folder = Folder::new();
    let loaded = load_at_start(
        &folder.path().join("absent"),
        HOST,
        &SetClock::at(SET_TIME),
        &Seam::default(),
    );
    assert_eq!(loaded.file, FileStatus::Missing);
    assert!(loaded.in_use.is_none());
    assert_eq!(loaded.migration, Migration::None);
}

#[test]
fn launch_with_a_file_the_system_cannot_read_is_unreadable() {
    // a folder of that name is read as a file by no system
    let folder = Folder::new().with_json(&json_with(1));
    fs::create_dir(folder.toml_path()).unwrap();
    let seam = Seam::default();
    let loaded = folder.start(&SetClock::at(SET_TIME), &seam);
    assert_eq!(loaded.file, FileStatus::Unreadable);
    assert!(loaded.in_use.is_none());
    assert_eq!(loaded.problems.len(), 1);
    assert!(loaded.problems[0].starts_with("config.toml could not be read"));
    // the name exists, so config.json is not migrated or even read
    assert_eq!(loaded.migration, Migration::None);
    assert_eq!(seam.json_checks.get(), 0);
    assert!(folder.toml_path().is_dir());
}

#[test]
fn launch_with_a_syntax_error_is_unreadable_and_with_broken_rules_invalid() {
    let syntax = Folder::new().with_toml("schema_version = [");
    let loaded = syntax.start(&SetClock::at(SET_TIME), &Seam::default());
    assert_eq!(loaded.file, FileStatus::Unreadable);
    assert!(loaded.in_use.is_none());
    assert!(!loaded.problems.is_empty());

    let rules = Folder::new().with_toml("schema_version = 1\nrouting_label = \"axpc\"\n");
    let loaded = rules.start(&SetClock::at(SET_TIME), &Seam::default());
    assert_eq!(loaded.file, FileStatus::Invalid);
    assert!(loaded.in_use.is_none());
    assert!(!loaded.problems.is_empty());
    assert_eq!(loaded.migration, Migration::None);
}

#[test]
fn a_file_over_one_mebibyte_is_unreadable_without_being_read_whole() {
    let folder = Folder::new().with_toml(&"#".repeat(3 * 1024 * 1024));
    let loaded = folder.start(&SetClock::at(SET_TIME), &Seam::default());
    assert_eq!(loaded.file, FileStatus::Unreadable);
    assert!(loaded.problems[0].contains("1 MiB"));
}

// Start all

fn launched(folder: &Folder, clock: &SetClock) -> Loaded {
    let loaded = folder.start(clock, &Seam::default());
    assert_eq!(loaded.file, FileStatus::Valid);
    loaded
}

#[test]
fn start_all_with_a_valid_file_while_holding_one_uses_the_file() {
    let folder = Folder::new().with_toml(&toml_with(2));
    let clock = SetClock::at(SET_TIME);
    let before = launched(&folder, &clock);
    fs::write(folder.toml_path(), toml_with(3)).unwrap();
    clock.0.set(SET_TIME + 60);
    let after = folder.start_all(&clock, &Seam::default(), &before);
    assert_eq!(after.file, FileStatus::Valid);
    assert_eq!(runners(&after), 3);
    assert_eq!(in_use(&after).origin, Origin::File);
    assert_eq!(in_use(&after).read_at, clock.time());
    assert!(after.problems.is_empty());
}

#[test]
fn start_all_with_a_valid_file_while_holding_none_uses_the_file() {
    let folder = Folder::new().with_toml("schema_version = 9");
    let clock = SetClock::at(SET_TIME);
    let before = folder.start(&clock, &Seam::default());
    assert!(before.in_use.is_none());
    fs::write(folder.toml_path(), toml_with(4)).unwrap();
    let after = folder.start_all(&clock, &Seam::default(), &before);
    assert_eq!(after.file, FileStatus::Valid);
    assert_eq!(runners(&after), 4);
    assert!(after.problems.is_empty());
}

/// the ways a file at Start all is missing, unreadable or invalid.
fn spoil(folder: &Folder, how: &str) -> FileStatus {
    let _ = fs::remove_file(folder.toml_path());
    let _ = fs::remove_dir(folder.toml_path());
    match how {
        "missing" => FileStatus::Missing,
        "unreadable" => {
            fs::create_dir(folder.toml_path()).unwrap();
            FileStatus::Unreadable
        }
        "syntax" => {
            fs::write(folder.toml_path(), "runners = [").unwrap();
            FileStatus::Unreadable
        }
        "invalid" => {
            fs::write(folder.toml_path(), toml_with(0)).unwrap();
            FileStatus::Invalid
        }
        other => panic!("{other}"),
    }
}

const SPOILS: [&str; 4] = ["missing", "unreadable", "syntax", "invalid"];

#[test]
fn start_all_with_a_bad_file_while_holding_one_keeps_the_last_valid_configuration() {
    for how in SPOILS {
        let folder = Folder::new().with_toml(&toml_with(2));
        let clock = SetClock::at(SET_TIME);
        let before = launched(&folder, &clock);
        let status = spoil(&folder, how);
        clock.0.set(SET_TIME + 60);
        let after = folder.start_all(&clock, &Seam::default(), &before);
        assert_eq!(after.file, status, "{how}");
        let used = in_use(&after);
        assert_eq!(used.origin, Origin::LastValid, "{how}");
        assert_eq!(used.plan, in_use(&before).plan, "{how}");
        assert_eq!(used.read_at, in_use(&before).read_at, "{how}");
        assert_eq!(after.problems.is_empty(), how == "missing", "{how}");
        assert_eq!(after.migration, Migration::None, "{how}");
        // a second Start all keeps it, still as the last valid one
        let again = folder.start_all(&clock, &Seam::default(), &after);
        assert_eq!(in_use(&again), used, "{how}");
    }
}

#[test]
fn start_all_with_a_bad_file_while_holding_none_holds_none() {
    for how in SPOILS {
        let folder = Folder::new().with_toml("schema_version = 9");
        let clock = SetClock::at(SET_TIME);
        let before = folder.start(&clock, &Seam::default());
        assert!(before.in_use.is_none());
        let status = spoil(&folder, how);
        let after = folder.start_all(&clock, &Seam::default(), &before);
        assert_eq!(after.file, status, "{how}");
        assert!(after.in_use.is_none(), "{how}");
        assert_eq!(after.problems.is_empty(), how == "missing", "{how}");
    }
}

#[test]
fn a_missing_file_beside_config_json_at_start_all_is_a_pending_migration() {
    let folder = Folder::new().with_toml(&toml_with(2));
    let clock = SetClock::at(SET_TIME);
    let before = launched(&folder, &clock);
    fs::remove_file(folder.toml_path()).unwrap();
    fs::write(folder.json_path(), json_with(5)).unwrap();
    let seam = Seam::default();
    let after = folder.start_all(&clock, &seam, &before);
    assert_eq!(after.file, FileStatus::Missing);
    assert_eq!(after.migration, Migration::Pending);
    assert_eq!(in_use(&after).origin, Origin::LastValid);
    assert_eq!(runners(&after), 2);
    // Start all neither migrates nor reads config.json
    assert!(!folder.toml_path().exists());
    assert_eq!(seam.json_checks.get(), 0);
    assert_eq!(seam.conversions.get(), 0);
    assert_eq!(
        fs::read_to_string(folder.json_path()).unwrap(),
        json_with(5)
    );
}

#[test]
fn a_pending_migration_with_nothing_held_holds_nothing_and_clears_with_a_file() {
    let folder = Folder::new();
    let clock = SetClock::at(SET_TIME);
    let before = folder.start(&clock, &Seam::default());
    fs::write(folder.json_path(), json_with(1)).unwrap();
    let pending = folder.start_all(&clock, &Seam::default(), &before);
    assert_eq!(pending.file, FileStatus::Missing);
    assert_eq!(pending.migration, Migration::Pending);
    assert!(pending.in_use.is_none());

    fs::write(folder.toml_path(), toml_with(2)).unwrap();
    let after = folder.start_all(&clock, &Seam::default(), &pending);
    assert_eq!(after.migration, Migration::None);
    assert_eq!(after.file, FileStatus::Valid);

    fs::remove_file(folder.json_path()).unwrap();
    fs::remove_file(folder.toml_path()).unwrap();
    let gone = folder.start_all(&clock, &Seam::default(), &pending);
    assert_eq!(gone.migration, Migration::None, "no config.json is waiting");
}

#[test]
fn a_bad_file_beside_config_json_at_start_all_mentions_only_the_file() {
    let folder = Folder::new()
        .with_toml(&toml_with(2))
        .with_json(&json_with(7));
    let clock = SetClock::at(SET_TIME);
    let before = launched(&folder, &clock);
    spoil(&folder, "invalid");
    let after = folder.start_all(&clock, &Seam::default(), &before);
    assert_eq!(after.file, FileStatus::Invalid);
    assert_eq!(after.migration, Migration::None);
    assert!(!debug_text(&after).contains("config.json"));
}

#[test]
fn a_failed_migration_stays_failed_at_start_all_while_the_file_is_missing() {
    let folder = Folder::new().with_json(&json_with(3));
    let clock = SetClock::at(SET_TIME);
    let failing = Seam {
        fail_conversion: true,
        ..Seam::default()
    };
    let before = folder.start(&clock, &failing);
    assert!(matches!(
        before.migration,
        Migration::Failed(MigrationFailure::CheckFailed(_))
    ));
    clock.0.set(SET_TIME + 60);
    let after = folder.start_all(&clock, &Seam::default(), &before);
    assert_eq!(after.file, FileStatus::Missing);
    assert_eq!(after.migration, before.migration);
    assert_eq!(in_use(&after), in_use(&before), "held, and not re-read");
    assert_eq!(in_use(&after).origin, Origin::Json);
    assert!(!folder.toml_path().exists());

    // a file that appears is read alone, and the failure no longer applies
    fs::write(folder.toml_path(), toml_with(4)).unwrap();
    let later = folder.start_all(&clock, &Seam::default(), &after);
    assert_eq!(later.file, FileStatus::Valid);
    assert_eq!(later.migration, Migration::None);
    assert_eq!(in_use(&later).origin, Origin::File);
}

#[test]
fn a_migration_that_succeeded_is_remembered_until_the_file_goes() {
    let folder = Folder::new().with_json(&json_with(3));
    let clock = SetClock::at(SET_TIME);
    let before = folder.start(&clock, &Seam::default());
    let migrated = Migration::Migrated { at: clock.time() };
    assert_eq!(before.migration, migrated);
    clock.0.set(SET_TIME + 60);
    let after = folder.start_all(&clock, &Seam::default(), &before);
    assert_eq!(after.migration, migrated);
    assert_eq!(after.file, FileStatus::Valid);

    fs::remove_file(folder.toml_path()).unwrap();
    let gone = folder.start_all(&clock, &Seam::default(), &after);
    assert_eq!(gone.migration, Migration::Pending);
    assert_eq!(in_use(&gone).origin, Origin::LastValid);
}

#[test]
fn an_edit_then_start_all_then_a_fix_then_start_all() {
    let folder = Folder::new().with_toml(&toml_with(2));
    let clock = SetClock::at(SET_TIME);
    let launch = launched(&folder, &clock);
    assert_eq!(runners(&launch), 2);

    // an invalid edit: the last valid configuration stays, with the file's problems
    fs::write(folder.toml_path(), toml_with(0)).unwrap();
    clock.0.set(SET_TIME + 60);
    let invalid = folder.start_all(&clock, &Seam::default(), &launch);
    assert_eq!(invalid.file, FileStatus::Invalid);
    assert_eq!(runners(&invalid), 2);
    assert_eq!(in_use(&invalid).origin, Origin::LastValid);
    assert_eq!(
        in_use(&invalid).read_at,
        clock.time() - Duration::from_secs(60)
    );
    assert!(!invalid.problems.is_empty());
    assert!(invalid.problems.iter().all(|problem| !problem.is_empty()));

    // a fix: the file's configuration is used and the problems clear
    fs::write(folder.toml_path(), toml_with(3)).unwrap();
    clock.0.set(SET_TIME + 120);
    let fixed = folder.start_all(&clock, &Seam::default(), &invalid);
    assert_eq!(fixed.file, FileStatus::Valid);
    assert_eq!(runners(&fixed), 3);
    assert_eq!(in_use(&fixed).origin, Origin::File);
    assert_eq!(in_use(&fixed).read_at, clock.time());
    assert!(fixed.problems.is_empty());
}

// migration

#[test]
fn migrating_writes_the_file_and_leaves_the_json_alone() {
    let json = json_with(2);
    let folder = Folder::new().with_json(&json);
    let clock = SetClock::at(SET_TIME);
    let seam = Seam::default();
    let loaded = folder.start(&clock, &seam);

    assert_eq!(loaded.file, FileStatus::Valid);
    assert_eq!(loaded.migration, Migration::Migrated { at: clock.time() });
    assert!(loaded.problems.is_empty());
    let used = in_use(&loaded);
    assert_eq!(used.origin, Origin::File);
    assert_eq!(used.read_at, clock.time());
    assert_eq!(runners(&loaded), 2);

    assert_eq!(fs::read(folder.json_path()).unwrap(), json.as_bytes());
    let written = fs::read_to_string(folder.toml_path()).unwrap();
    assert!(
        written.starts_with(&format!(
            "# Converted from config.json by Marshal at {SET_TIME_TEXT}."
        )),
        "{written}"
    );
    assert!(written.contains("routing_label = \"axpc\""));
    for absent in ["tokenPath", "token_path", "alpha.token", "C:"] {
        assert!(!written.contains(absent), "{absent}: {written}");
    }
    assert_eq!(folder.names(), ["config.json", "config.toml"]);
    assert_eq!(seam.sources.borrow().as_slice(), [folder.json_path()]);

    // the pools are the JSON file's
    let from_json = validate(json.as_bytes(), HOST, None).outcome.unwrap();
    let from_toml = validate_toml(written.as_bytes(), HOST, None)
        .outcome
        .unwrap();
    assert_eq!(from_json.entries.len(), from_toml.entries.len());
    assert_eq!(from_json.entries[0].slots, used.plan.entries[0].slots);
    assert_eq!(from_json.entries[0].labels, used.plan.entries[0].labels);

    // the next start reads only the file it wrote
    let next = folder.start(&clock, &seam);
    assert_eq!(next.migration, Migration::None);
    assert_eq!(seam.conversions.get(), 1);
    assert_eq!(seam.json_checks.get(), 1);
}

#[test]
fn an_invalid_json_file_writes_nothing_and_fails_with_its_problems() {
    let broken = [
        json_with(0),
        "{".to_string(),
        String::new(),
        "x".repeat(1024 * 1024 + 1),
    ];
    for json in broken {
        let folder = Folder::new().with_json(&json);
        let seam = Seam::default();
        let loaded = folder.start(&SetClock::at(SET_TIME), &seam);
        assert_eq!(loaded.file, FileStatus::Missing);
        assert!(loaded.in_use.is_none());
        assert!(loaded.problems.is_empty());
        let Migration::Failed(MigrationFailure::JsonRejected(problems)) = &loaded.migration else {
            panic!("{:?}", loaded.migration);
        };
        assert!(!problems.is_empty());
        assert_eq!(folder.names(), ["config.json"]);
        assert_eq!(fs::read(folder.json_path()).unwrap(), json.as_bytes());
        assert_eq!(seam.conversions.get(), 0);
        // the next start tries again
        let again = folder.start(&SetClock::at(SET_TIME), &seam);
        assert_eq!(again.migration, loaded.migration);
    }
}

#[test]
fn json_problems_are_in_todays_wording() {
    let json = json_with(0);
    let folder = Folder::new().with_json(&json);
    let loaded = folder.start(&SetClock::at(SET_TIME), &Seam::default());
    let Migration::Failed(MigrationFailure::JsonRejected(problems)) = loaded.migration else {
        panic!("not rejected");
    };
    let expected = match validate(json.as_bytes(), HOST, None).outcome {
        Err(marshal_cli::validate::Failure::Invalid(problems)) => problems,
        other => panic!("{other:?}"),
    };
    assert_eq!(problems, expected);
}

#[test]
fn a_failed_check_leaves_no_file_uses_the_json_and_is_tried_again() {
    let json = json_with(3);
    let folder = Folder::new().with_json(&json);
    let clock = SetClock::at(SET_TIME);
    let failing = Seam {
        fail_conversion: true,
        ..Seam::default()
    };
    let loaded = folder.start(&clock, &failing);

    assert_eq!(loaded.file, FileStatus::Missing);
    assert!(loaded.problems.is_empty());
    let Migration::Failed(MigrationFailure::CheckFailed(reason)) = &loaded.migration else {
        panic!("{:?}", loaded.migration);
    };
    assert!(reason.contains("forced"));
    let used = in_use(&loaded);
    assert_eq!(used.origin, Origin::Json);
    assert_eq!(used.read_at, clock.time());
    assert_eq!(
        used.plan,
        validate(json.as_bytes(), HOST, None).outcome.unwrap()
    );
    assert_eq!(folder.names(), ["config.json"]);
    assert_eq!(fs::read(folder.json_path()).unwrap(), json.as_bytes());

    let retry = folder.start(&clock, &Seam::default());
    assert_eq!(retry.migration, Migration::Migrated { at: clock.time() });
    assert_eq!(retry.file, FileStatus::Valid);
}

#[test]
fn a_failed_write_leaves_no_file_and_uses_the_json() {
    let json = json_with(3);
    let folder = Folder::new().with_json(&json);
    let path = folder.path().to_path_buf();
    // the folder goes while the conversion runs, so no file can be made in it
    let seam = Seam {
        during_conversion: Some(Box::new(move |_| fs::remove_dir_all(&path).unwrap())),
        ..Seam::default()
    };
    let loaded = folder.start(&SetClock::at(SET_TIME), &seam);

    assert_eq!(loaded.file, FileStatus::Missing);
    let Migration::Failed(MigrationFailure::WriteFailed(reason)) = &loaded.migration else {
        panic!("{:?}", loaded.migration);
    };
    assert!(reason.contains("config.toml"), "{reason}");
    assert!(
        !reason.contains(folder.path().to_str().unwrap()),
        "{reason}"
    );
    let used = in_use(&loaded);
    assert_eq!(used.origin, Origin::Json);
    assert_eq!(used.plan.entries[0].slots, 3);
    assert!(!folder.toml_path().exists());
}

#[test]
fn a_file_that_appears_meanwhile_is_left_as_it_is_and_read() {
    let json = json_with(3);
    let folder = Folder::new().with_json(&json);
    let theirs = toml_with(5);
    let toml = folder.toml_path();
    let text = theirs.clone();
    let seam = Seam {
        during_conversion: Some(Box::new(move |_| fs::write(&toml, &text).unwrap())),
        ..Seam::default()
    };
    let loaded = folder.start(&SetClock::at(SET_TIME), &seam);

    assert_eq!(loaded.file, FileStatus::Valid);
    assert_eq!(loaded.migration, Migration::None);
    assert_eq!(in_use(&loaded).origin, Origin::File);
    assert_eq!(runners(&loaded), 5);
    assert_eq!(fs::read_to_string(folder.toml_path()).unwrap(), theirs);
    assert_eq!(fs::read(folder.json_path()).unwrap(), json.as_bytes());
    assert_eq!(folder.names(), ["config.json", "config.toml"]);
}

#[test]
fn an_invalid_file_that_appears_meanwhile_is_left_as_it_is_and_reported() {
    let folder = Folder::new().with_json(&json_with(3));
    let toml = folder.toml_path();
    let seam = Seam {
        during_conversion: Some(Box::new(move |_| fs::write(&toml, toml_with(0)).unwrap())),
        ..Seam::default()
    };
    let loaded = folder.start(&SetClock::at(SET_TIME), &seam);
    assert_eq!(loaded.file, FileStatus::Invalid);
    assert!(loaded.in_use.is_none());
    assert_eq!(loaded.migration, Migration::None);
    assert!(!debug_text(&loaded).contains("config.json"));
    assert_eq!(
        fs::read_to_string(folder.toml_path()).unwrap(),
        toml_with(0)
    );
}

#[test]
fn with_both_files_only_the_toml_is_read_and_nothing_mentions_the_json() {
    for toml in [toml_with(2), toml_with(0), "schema_version = [".to_string()] {
        let folder = Folder::new().with_toml(&toml).with_json(&json_with(9));
        let seam = Seam::default();
        let loaded = folder.start(&SetClock::at(SET_TIME), &seam);
        assert_eq!(loaded.migration, Migration::None);
        assert_eq!(seam.json_checks.get(), 0);
        assert_eq!(seam.conversions.get(), 0);
        assert!(!debug_text(&loaded).contains("config.json"));
        assert_eq!(fs::read_to_string(folder.toml_path()).unwrap(), toml);
        if loaded.file == FileStatus::Valid {
            assert_eq!(runners(&loaded), 2);
        }
    }
}

// the start-up clean-up

fn leftover(number: u64) -> String {
    format!("config.toml.{number:032x}.tmp")
}

#[test]
fn the_start_removes_only_the_write_paths_temporary_files() {
    let others = [
        "config.json.0123456789abcdef0123456789abcdef.tmp",
        "config.toml.old.tmp",
        "config.toml.bak",
        "config.toml.0123456789ABCDEF0123456789ABCDEF.tmp",
        "notes.tmp",
    ];
    // with a file to read, and with a migration to do
    for json_only in [false, true] {
        let folder = Folder::new();
        let folder = if json_only {
            folder.with_json(&json_with(1))
        } else {
            folder.with_toml(&toml_with(1))
        };
        for name in [leftover(1), leftover(2)]
            .iter()
            .map(String::as_str)
            .chain(others)
        {
            fs::write(folder.path().join(name), name).unwrap();
        }
        let loaded = folder.start(&SetClock::at(SET_TIME), &Seam::default());
        assert_eq!(loaded.file, FileStatus::Valid);
        let mut expected: Vec<String> = others.iter().map(ToString::to_string).collect();
        expected.push(
            if json_only {
                "config.json"
            } else {
                "config.toml"
            }
            .to_string(),
        );
        if json_only {
            expected.push("config.toml".to_string());
        }
        expected.sort();
        assert_eq!(folder.names(), expected, "json only: {json_only}");
    }
}

#[test]
fn start_all_does_not_remove_temporary_files() {
    let folder = Folder::new().with_toml(&toml_with(1));
    let clock = SetClock::at(SET_TIME);
    let before = launched(&folder, &clock);
    fs::write(folder.path().join(leftover(1)), "x").unwrap();
    folder.start_all(&clock, &Seam::default(), &before);
    assert!(folder.names().contains(&leftover(1)));
}

// secrets

#[test]
fn a_token_shaped_string_is_echoed_by_no_problem_or_field() {
    let toml_cases = [
        format!("schema_version = 1\nrouting_label = {SECRET}\n"),
        format!(
            "schema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"runner:1\"\n[[pool]]\nname = \"alpha\"\nrepository = \"octo/alpha\"\nrunners = \"{SECRET}\"\n"
        ),
        format!(
            "schema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"runner:1\"\nextra = \"{SECRET}\"\n{}",
            pool("alpha", 1)
        ),
        format!(
            "schema_version = 1\nrouting_label = \"axpc\"\nimage_name = \"runner:1\"\n[[pool]]\nname = \"{SECRET}\"\nrepository = \"{SECRET}\"\nrunners = 1\nlabels = [\"{SECRET}\", 1]\n"
        ),
    ];
    for toml in toml_cases {
        let folder = Folder::new().with_toml(&toml);
        let loaded = folder.start(&SetClock::at(SET_TIME), &Seam::default());
        assert!(!debug_text(&loaded).contains(SECRET), "{toml}\n{loaded:?}");
        assert_ne!(loaded.file, FileStatus::Valid, "{toml}");
        // the held configuration of a later Start all carries none either
        let good = Folder::new().with_toml(&toml_with(1));
        let before = launched(&good, &SetClock::at(SET_TIME));
        fs::write(good.toml_path(), &toml).unwrap();
        let after = good.start_all(&SetClock::at(SET_TIME), &Seam::default(), &before);
        assert!(!debug_text(&after).contains(SECRET), "{toml}\n{after:?}");
    }

    let json_cases = [
        format!("{{\"imageName\": {SECRET}}}"),
        format!(
            "{{\"imageName\": \"runner:1\", \"repositories\": [{{\"owner\": \"octo\", \"repository\": \"alpha\", \"name\": \"alpha\", \"slots\": \"{SECRET}\", \"tokenPath\": \"C:\\\\x\"}}]}}"
        ),
        format!("{{\"imageName\": \"runner:1\", \"extra\": \"{SECRET}\", \"repositories\": []}}"),
        format!(
            "{{\"imageName\": \"runner:1\", \"repositories\": [{{\"owner\": \"{SECRET}\", \"repository\": \"{SECRET}\", \"name\": \"{SECRET}\", \"slots\": 1, \"tokenPath\": \"{SECRET}\", \"labels\": [\"{SECRET}\", 1]}}]}}"
        ),
    ];
    for json in json_cases {
        let folder = Folder::new().with_json(&json);
        let loaded = folder.start(&SetClock::at(SET_TIME), &Seam::default());
        assert!(matches!(loaded.migration, Migration::Failed(_)), "{json}");
        assert!(!debug_text(&loaded).contains(SECRET), "{json}\n{loaded:?}");
    }
}

#[test]
fn a_secret_in_a_token_path_that_migrates_reaches_no_toml_and_no_message() {
    let json = json_with(1).replace(TOKEN_FILE, &format!("C:\\\\{SECRET}.token"));
    let folder = Folder::new().with_json(&json);
    let loaded = folder.start(&SetClock::at(SET_TIME), &Seam::default());
    assert_eq!(loaded.file, FileStatus::Valid);
    assert!(
        !fs::read_to_string(folder.toml_path())
            .unwrap()
            .contains(SECRET)
    );
    assert!(!debug_text(&loaded).contains(SECRET));
}

// the create-only write

#[test]
fn create_new_writes_a_missing_file_and_leaves_no_temporary_file() {
    let folder = Folder::new();
    assert_eq!(
        create_new(&folder.toml_path(), b"new"),
        CreateOutcome::Written
    );
    assert_eq!(fs::read(folder.toml_path()).unwrap(), b"new");
    assert_eq!(folder.names(), ["config.toml"]);
}

#[test]
fn create_new_leaves_a_taken_name_alone_whatever_it_is() {
    let folder = Folder::new().with_toml("mine");
    assert_eq!(
        create_new(&folder.toml_path(), b"new"),
        CreateOutcome::Taken
    );
    assert_eq!(fs::read(folder.toml_path()).unwrap(), b"mine");
    assert_eq!(folder.names(), ["config.toml"]);

    // a folder of that name is not replaced either; systems differ on how they say so
    let other = Folder::new();
    fs::create_dir(other.toml_path()).unwrap();
    assert_ne!(
        create_new(&other.toml_path(), b"new"),
        CreateOutcome::Written
    );
    assert!(other.toml_path().is_dir());
    assert_eq!(other.names(), ["config.toml"]);
}

#[test]
fn create_new_in_a_folder_that_is_gone_fails_with_its_class_and_leaves_nothing() {
    let folder = Folder::new();
    let missing = folder.path().join("absent").join("config.toml");
    let CreateOutcome::Failed(failure) = create_new(&missing, b"new") else {
        panic!("expected a failure");
    };
    assert_eq!(failure.step, CreateStep::Write);
    assert_eq!(failure.kind, std::io::ErrorKind::NotFound);
    assert!(folder.names().is_empty());
}
