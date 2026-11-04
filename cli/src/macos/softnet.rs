//! finding Softnet as Tart does, judging its file, and reading its version. Tart
//! takes no path for Softnet: it runs the first `softnet` on its own `PATH`, so the
//! checks run that file and no other.

use std::ffi::OsStr;
use std::fmt;
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runtime::docker::client_command;
use crate::runtime::tart::client::{Captured, TartError, capture_command};

/// the name Tart looks for.
pub const SOFTNET_PROGRAM: &str = "softnet";

/// the variable that replaces the owner and mode read from Softnet's file in a debug
/// build, as `<user ID>:<octal mode>`, such as `0:4755`; a release build ignores it.
pub const TEST_OWNER_MODE_VARIABLE: &str = "MARSHAL_TEST_SOFTNET_OWNER_MODE";

const SUID_BIT: u32 = 0o4000;
const GROUP_WRITE_BIT: u32 = 0o020;
const OTHER_WRITE_BIT: u32 = 0o002;
const PERMISSION_BITS: u32 = 0o7777;
const ROOT_USER_ID: u32 = 0;

/// the first `softnet` on `path`, found as Tart finds it: each entry in order, empty
/// entries skipped, the first where `softnet` exists with links followed. Tart tests
/// existence alone, not the executable bit or the file's type, and a link that leads
/// nowhere does not exist.
#[must_use]
pub fn find_on_path(path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|directory| !directory.as_os_str().is_empty())
        .map(|directory| directory.join(SOFTNET_PROGRAM))
        .find(|candidate| std::fs::metadata(candidate).is_ok())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Directory,
    Other,
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Regular => "a regular file",
            Self::Directory => "a directory",
            Self::Other => "neither a regular file nor a directory",
        })
    }
}

/// the owner and permission bits of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerMode {
    pub uid: u32,
    pub mode: u32,
}

/// what the file check reads of the file, after links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFacts {
    pub kind: FileKind,
    pub owner_mode: OwnerMode,
}

/// why Softnet's file is not set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProblem {
    NotRegular(FileKind),
    NotOwnedByRoot { uid: u32 },
    NoSuidBit,
    GroupWritable,
    OtherWritable,
}

/// the first way `facts` fails to be a regular file owned by root with the SUID bit
/// set and no write permission for its group or others; the checks run in that order.
#[must_use]
pub fn setup_problem(facts: FileFacts) -> Option<SetupProblem> {
    let OwnerMode { uid, mode } = facts.owner_mode;
    if facts.kind != FileKind::Regular {
        return Some(SetupProblem::NotRegular(facts.kind));
    }
    if uid != ROOT_USER_ID {
        return Some(SetupProblem::NotOwnedByRoot { uid });
    }
    if mode & SUID_BIT == 0 {
        return Some(SetupProblem::NoSuidBit);
    }
    if mode & GROUP_WRITE_BIT != 0 {
        return Some(SetupProblem::GroupWritable);
    }
    if mode & OTHER_WRITE_BIT != 0 {
        return Some(SetupProblem::OtherWritable);
    }
    None
}

/// the owner and mode a debug build takes from `requested` in place of the file's: a
/// release build always returns `Ok(None)`, so a release binary cannot be made to
/// accept a file it would refuse. A debug build refuses a malformed value.
pub fn resolve_owner_mode_override(requested: Option<&OsStr>) -> Result<Option<OwnerMode>, String> {
    #[cfg(debug_assertions)]
    if let Some(requested) = requested {
        return parse_owner_mode(requested).map(Some);
    }
    #[cfg(not(debug_assertions))]
    let _ = requested;
    Ok(None)
}

/// the override this process was given: `MARSHAL_TEST_SOFTNET_OWNER_MODE` in a debug
/// build, else none.
pub fn owner_mode_override_from_environment() -> Result<Option<OwnerMode>, String> {
    resolve_owner_mode_override(std::env::var_os(TEST_OWNER_MODE_VARIABLE).as_deref())
}

#[cfg(debug_assertions)]
fn parse_owner_mode(value: &OsStr) -> Result<OwnerMode, String> {
    let malformed = || {
        format!(
            "{TEST_OWNER_MODE_VARIABLE} must be <user ID>:<octal mode>, such as 0:4755, not {value:?}"
        )
    };
    let (uid, mode) = value
        .to_str()
        .and_then(|text| text.split_once(':'))
        .ok_or_else(malformed)?;
    Ok(OwnerMode {
        uid: uid.parse().map_err(|_| malformed())?,
        mode: u32::from_str_radix(mode, 8).map_err(|_| malformed())? & PERMISSION_BITS,
    })
}

/// the type, owner and mode of the file at `path`, with links followed.
/// `owner_mode_override` replaces the owner and mode and leaves the type.
pub fn read_file_facts(
    path: &Path,
    owner_mode_override: Option<OwnerMode>,
) -> io::Result<FileFacts> {
    let metadata = std::fs::metadata(path)?;
    let OwnerMode { uid, mode } = match owner_mode_override {
        Some(replacement) => replacement,
        None => owner_and_mode(&metadata)?,
    };
    let owner_mode = OwnerMode {
        uid,
        mode: mode & PERMISSION_BITS,
    };
    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        FileKind::Regular
    } else if file_type.is_dir() {
        FileKind::Directory
    } else {
        FileKind::Other
    };
    Ok(FileFacts { kind, owner_mode })
}

#[cfg(unix)]
fn owner_and_mode(metadata: &Metadata) -> io::Result<OwnerMode> {
    use std::os::unix::fs::MetadataExt;

    Ok(OwnerMode {
        uid: metadata.uid(),
        mode: metadata.mode(),
    })
}

#[cfg(not(unix))]
fn owner_and_mode(_metadata: &Metadata) -> io::Result<OwnerMode> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "a file's owner and mode are read on Unix only",
    ))
}

/// runs `program --version` and returns what it printed, within `budget`: its own
/// process group, killed when the future is dropped, standard input null, output cut
/// at the Tart client's bound, the JIT variable cleared and `path` as its `PATH`.
pub async fn capture_version(
    program: &Path,
    path: Option<&OsStr>,
    budget: Duration,
) -> Result<Captured, TartError> {
    let mut command = client_command(program.as_os_str(), &[], &["--version".to_string()]);
    match path {
        Some(path) => command.env("PATH", path),
        None => command.env_remove("PATH"),
    };
    capture_command(command, None, budget).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(kind: FileKind, uid: u32, mode: u32) -> FileFacts {
        FileFacts {
            kind,
            owner_mode: OwnerMode { uid, mode },
        }
    }

    #[test]
    fn a_regular_file_owned_by_root_with_the_suid_bit_and_no_group_or_other_write_passes() {
        for mode in [0o4755, 0o4555, 0o4711, 0o4700, 0o4750, 0o4751, 0o4505] {
            assert_eq!(
                setup_problem(facts(FileKind::Regular, 0, mode)),
                None,
                "{mode:o}"
            );
        }
    }

    #[test]
    fn anything_but_a_regular_file_is_refused() {
        assert_eq!(
            setup_problem(facts(FileKind::Directory, 0, 0o4755)),
            Some(SetupProblem::NotRegular(FileKind::Directory))
        );
        assert_eq!(
            setup_problem(facts(FileKind::Other, 0, 0o4755)),
            Some(SetupProblem::NotRegular(FileKind::Other))
        );
    }

    #[test]
    fn a_file_owned_by_anyone_but_root_is_refused() {
        for uid in [1, 501, 4_294_967_295] {
            assert_eq!(
                setup_problem(facts(FileKind::Regular, uid, 0o4755)),
                Some(SetupProblem::NotOwnedByRoot { uid })
            );
        }
    }

    #[test]
    fn a_file_without_the_suid_bit_is_refused_whatever_else_its_mode_holds() {
        for mode in [0o0755, 0o2755, 0o1755, 0o0000, 0o0555] {
            assert_eq!(
                setup_problem(facts(FileKind::Regular, 0, mode)),
                Some(SetupProblem::NoSuidBit),
                "{mode:o}"
            );
        }
    }

    #[test]
    fn a_file_its_group_or_others_can_write_is_refused() {
        for mode in [0o4775, 0o4770, 0o4720, 0o6775] {
            assert_eq!(
                setup_problem(facts(FileKind::Regular, 0, mode)),
                Some(SetupProblem::GroupWritable),
                "{mode:o}"
            );
        }
        for mode in [0o4757, 0o4702, 0o4777, 0o4752] {
            let expected = if mode & GROUP_WRITE_BIT == 0 {
                SetupProblem::OtherWritable
            } else {
                SetupProblem::GroupWritable
            };
            assert_eq!(
                setup_problem(facts(FileKind::Regular, 0, mode)),
                Some(expected),
                "{mode:o}"
            );
        }
    }

    #[test]
    fn the_problems_are_found_in_the_order_type_owner_suid_group_other() {
        assert_eq!(
            setup_problem(facts(FileKind::Directory, 501, 0o0777)),
            Some(SetupProblem::NotRegular(FileKind::Directory))
        );
        assert_eq!(
            setup_problem(facts(FileKind::Regular, 501, 0o0777)),
            Some(SetupProblem::NotOwnedByRoot { uid: 501 })
        );
        assert_eq!(
            setup_problem(facts(FileKind::Regular, 0, 0o0777)),
            Some(SetupProblem::NoSuidBit)
        );
        assert_eq!(
            setup_problem(facts(FileKind::Regular, 0, 0o4777)),
            Some(SetupProblem::GroupWritable)
        );
    }

    #[test]
    fn the_override_names_the_owner_and_mode_in_a_debug_build_only() {
        let requested = OsStr::new("0:4755");
        let resolved = resolve_owner_mode_override(Some(requested)).unwrap();
        if cfg!(debug_assertions) {
            assert_eq!(
                resolved,
                Some(OwnerMode {
                    uid: 0,
                    mode: 0o4755
                })
            );
        } else {
            assert_eq!(resolved, None);
        }
        assert_eq!(resolve_owner_mode_override(None).unwrap(), None);
    }

    #[test]
    fn a_debug_build_refuses_a_malformed_override_and_a_release_build_ignores_it() {
        for malformed in [
            "",
            "0",
            "0:",
            ":4755",
            "root:4755",
            "0:9",
            "0:4755:1",
            "-1:4755",
        ] {
            let resolved = resolve_owner_mode_override(Some(OsStr::new(malformed)));
            if cfg!(debug_assertions) {
                assert!(
                    resolved.unwrap_err().contains(TEST_OWNER_MODE_VARIABLE),
                    "{malformed:?}"
                );
            } else {
                assert_eq!(resolved, Ok(None));
            }
        }
    }

    #[test]
    fn the_override_keeps_the_permission_bits_only() {
        if cfg!(debug_assertions) {
            let resolved = resolve_owner_mode_override(Some(OsStr::new("501:100755")));
            assert_eq!(
                resolved,
                Ok(Some(OwnerMode {
                    uid: 501,
                    mode: 0o0755
                }))
            );
        }
    }

    #[test]
    fn nothing_is_found_without_a_path() {
        assert_eq!(find_on_path(None), None);
        assert_eq!(find_on_path(Some(OsStr::new(""))), None);
    }
}
