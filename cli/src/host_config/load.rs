//! the loads: at the agent's start, which migrates `config.json` when it must, and at
//! Start all.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::command::MAX_CONFIG_BYTES;
use crate::edit::{
    CONFIG_FILE_NAME, CreateFailure, CreateOutcome, CreateStep, create_new,
    remove_leftover_temporary_files,
};
use crate::profile::Profile;
use crate::runtime::clock::Clock;
use crate::validate::Failure;

use super::{Checks, FileStatus, InUse, Loaded, Migration, MigrationFailure, Origin};

/// the file name of the JSON configuration that a migration reads.
pub const JSON_FILE_NAME: &str = "config.json";

/// loads the configuration when the agent starts, in `folder`, which holds both files.
///
/// it removes the temporary files an interrupted write left, then reads `config.toml`.
/// when that is missing and `config.json` exists, it migrates it: the JSON is checked,
/// converted and the conversion checked, and the TOML is written only where no file of
/// that name exists. the file is then read as at launch. a `config.toml` that appeared
/// meanwhile is left as it is and read.
pub fn load_at_start<K: Checks, C: Clock>(
    folder: &Path,
    profile: Profile,
    clock: &C,
    checks: &K,
) -> Loaded {
    // a folder that cannot be listed has no leftovers to remove, and reading the file
    // reports a folder that cannot be read
    let _ = remove_leftover_temporary_files(folder);
    let loader = Loader {
        folder,
        profile,
        clock,
        checks,
    };
    match loader.read_file() {
        FileRead::Missing => loader.migrate(),
        read => finish(read, Migration::None),
    }
}

/// loads the configuration at Start all from a stopped state: `config.toml` is read
/// again, and `previous` is the result of the load before, whose configuration in use
/// is the one the agent holds.
///
/// a valid file is used. a file with problems, or none, leaves the held configuration
/// in use, and a held configuration read from the file is then the last valid one.
/// nothing is migrated: that happens only at start, so a missing file beside
/// `config.json` is a pending migration, unless the migration already failed.
pub fn load_at_start_all<K: Checks, C: Clock>(
    folder: &Path,
    profile: Profile,
    clock: &C,
    checks: &K,
    previous: &Loaded,
) -> Loaded {
    let loader = Loader {
        folder,
        profile,
        clock,
        checks,
    };
    let (file, in_use, problems) = match loader.read_file() {
        FileRead::Valid(in_use) => (FileStatus::Valid, Some(in_use), Vec::new()),
        FileRead::Problems { status, problems } => (status, held(previous), problems),
        FileRead::Missing => (FileStatus::Missing, held(previous), Vec::new()),
    };
    let json_exists = file == FileStatus::Missing && fs::metadata(loader.json_path()).is_ok();
    Loaded {
        file,
        in_use,
        migration: migration_after(&previous.migration, file, json_exists),
        problems,
    }
}

/// the configuration the agent holds, which stays in use as the last valid one when
/// it came from the file.
fn held(previous: &Loaded) -> Option<InUse> {
    previous.in_use.clone().map(|mut in_use| {
        if in_use.origin == Origin::File {
            in_use.origin = Origin::LastValid;
        }
        in_use
    })
}

/// the migration's state after a load at Start all. a failure stays while the file is
/// still missing, and goes when a file is there, which is read alone; a migration that
/// succeeded stays for as long as the agent runs, unless the file has gone again and
/// `config.json` is waiting.
fn migration_after(previous: &Migration, file: FileStatus, json_exists: bool) -> Migration {
    match (file, previous) {
        (FileStatus::Missing, Migration::Failed(_)) => previous.clone(),
        (FileStatus::Missing, _) if json_exists => Migration::Pending,
        (_, Migration::Migrated { .. }) => previous.clone(),
        _ => Migration::None,
    }
}

/// what reading `config.toml` gave.
enum FileRead {
    Valid(InUse),
    Problems {
        status: FileStatus,
        problems: Vec<String>,
    },
    Missing,
}

fn finish(read: FileRead, migration: Migration) -> Loaded {
    match read {
        FileRead::Valid(in_use) => Loaded {
            file: FileStatus::Valid,
            in_use: Some(in_use),
            migration,
            problems: Vec::new(),
        },
        FileRead::Problems { status, problems } => Loaded {
            file: status,
            in_use: None,
            migration,
            problems,
        },
        FileRead::Missing => Loaded {
            file: FileStatus::Missing,
            in_use: None,
            migration,
            problems: Vec::new(),
        },
    }
}

struct Loader<'a, K, C> {
    folder: &'a Path,
    profile: Profile,
    clock: &'a C,
    checks: &'a K,
}

impl<K: Checks, C: Clock> Loader<'_, K, C> {
    fn toml_path(&self) -> PathBuf {
        self.folder.join(CONFIG_FILE_NAME)
    }

    fn json_path(&self) -> PathBuf {
        self.folder.join(JSON_FILE_NAME)
    }

    fn read_file(&self) -> FileRead {
        let bytes = match read_capped(&self.toml_path()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return FileRead::Missing,
            Err(error) => {
                return FileRead::Problems {
                    status: FileStatus::Unreadable,
                    problems: vec![unreadable(CONFIG_FILE_NAME, &error)],
                };
            }
        };
        let validation = self.checks.check_toml(&bytes, self.profile);
        match validation.outcome {
            Ok(plan) => FileRead::Valid(InUse {
                origin: Origin::File,
                plan,
                read_at: self.clock.now(),
            }),
            Err(failure) => FileRead::Problems {
                status: match failure {
                    Failure::Unreadable(_) => FileStatus::Unreadable,
                    Failure::Invalid(_) => FileStatus::Invalid,
                },
                problems: problems_of(failure),
            },
        }
    }

    /// `config.toml` is missing: migrates `config.json`, if there is one.
    fn migrate(&self) -> Loaded {
        let json = match read_capped(&self.json_path()) {
            Ok(bytes) if bytes.len() as u64 > MAX_CONFIG_BYTES => {
                return self.rejected(vec![format!(
                    "the file is larger than {MAX_CONFIG_BYTES} bytes (1 MiB)"
                )]);
            }
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return finish(FileRead::Missing, Migration::None);
            }
            Err(error) => return self.rejected(vec![unreadable(JSON_FILE_NAME, &error)]),
        };
        let validation = self.checks.check_json(&json, self.profile);
        let plan = match validation.outcome {
            Ok(plan) => plan,
            Err(failure) => return self.rejected(problems_of(failure)),
        };
        let from_json = InUse {
            origin: Origin::Json,
            plan,
            read_at: self.clock.now(),
        };
        let document = match self
            .checks
            .convert(&json, self.profile, &self.json_path(), self.clock)
        {
            Ok(document) => document,
            Err(failure) => {
                return fallback(
                    from_json,
                    MigrationFailure::CheckFailed(failure.to_string()),
                );
            }
        };
        match create_new(&self.toml_path(), document.as_bytes()) {
            CreateOutcome::Written => finish(
                self.read_file(),
                Migration::Migrated {
                    at: self.clock.now(),
                },
            ),
            CreateOutcome::Taken => finish(self.read_file(), Migration::None),
            CreateOutcome::Failed(failure) => {
                fallback(from_json, MigrationFailure::WriteFailed(why(&failure)))
            }
        }
    }

    /// `config.json` cannot be migrated and nothing is in use.
    fn rejected(&self, problems: Vec<String>) -> Loaded {
        Loaded {
            file: FileStatus::Missing,
            in_use: None,
            migration: Migration::Failed(MigrationFailure::JsonRejected(problems)),
            problems: Vec::new(),
        }
    }
}

/// `config.toml` was not written, and `config.json`'s configuration is in use.
fn fallback(in_use: InUse, failure: MigrationFailure) -> Loaded {
    Loaded {
        file: FileStatus::Missing,
        in_use: Some(in_use),
        migration: Migration::Failed(failure),
        problems: Vec::new(),
    }
}

/// the file's name and the class of the system's error, never the file's contents.
fn unreadable(name: &str, error: &io::Error) -> String {
    format!("{name} could not be read: {}", error.kind())
}

/// why the file was not written, naming the file and not its folder.
fn why(failure: &CreateFailure) -> String {
    match failure.step {
        CreateStep::Write => format!(
            "could not write a new file beside {CONFIG_FILE_NAME}: {}",
            failure.kind
        ),
        CreateStep::Link => format!("could not create {CONFIG_FILE_NAME}: {}", failure.kind),
    }
}

fn problems_of(failure: Failure) -> Vec<String> {
    match failure {
        Failure::Unreadable(problem) => vec![problem],
        Failure::Invalid(problems) => problems,
    }
}

/// the file's bytes, at most 1 MiB and one more: the extra byte tells a longer file,
/// which the checks refuse.
fn read_capped(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}
