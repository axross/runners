//! changes an existing `config.toml` and keeps what the change leaves alone: comments,
//! key order, blank lines, the line ending, the byte-order mark and the comment on a
//! changed value's line. the result is checked with the full TOML rules for the host
//! before anything is written, and the file is replaced in one step, so a reader sees
//! the old file or the new one; see `docs/operations/cli.md`. a file that does not exist
//! yet is created in the same way, but only where no file of that name exists.

mod create;
mod document;
mod frame;
mod replace;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::command::MAX_CONFIG_BYTES;
use crate::profile::Profile;
use crate::validate::{Failure, Key, KeyPath, validate_toml};

use document::Refusal;
use frame::Frame;

pub use create::{CreateFailure, CreateOutcome, CreateStep, create_new};
pub use replace::{CONFIG_FILE_NAME, Cleanup, remove_leftover_temporary_files};

/// a value to put in the file.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    /// an array of strings, such as `labels`.
    Strings(Vec<String>),
}

/// one change to the file. a path names the keys from the root, and a position for an
/// entry of an array of tables: `pool[1].labels` is `Repositories`, `Index(1)`,
/// `Labels`.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// sets a value, or adds it to a table that exists.
    Set { path: KeyPath, value: Value },
    /// removes a value; one that is not there stays absent.
    Remove { path: KeyPath },
    /// creates an empty table that is missing, such as `lifecycle`; a table that exists
    /// stays as it is. at the root it goes before the first pool.
    CreateTable { path: KeyPath },
    /// appends an entry to an array of tables, such as `pool` or a pool's `volumes`,
    /// and creates the array when it is missing.
    AddEntry {
        array: KeyPath,
        values: Vec<(Key, Value)>,
    },
    /// removes an entry of an array of tables, with the comment lines directly above it.
    RemoveEntry { path: KeyPath },
}

/// what the system was asked to do when it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Read,
    Write,
    Replace,
}

/// why a change was not written. nothing was written in any of these cases, and no
/// temporary file is left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// the file no longer holds the bytes the change was made from.
    Changed,
    /// the result does not pass the TOML rules for the host, or the file the change
    /// starts from is not a TOML document; the failure lists the problems.
    Invalid(Failure),
    /// the file could not be read, or the new one could not be written or put in its
    /// place, for example because an editor holds the file open on Windows.
    Io {
        path: PathBuf,
        operation: Operation,
        kind: io::ErrorKind,
    },
    /// an edit names a place the document does not have, or one of another kind, which
    /// is a mistake of the caller; or the file is written in a form the write path does
    /// not edit, which only the user can change: it holds the removal marker, or the
    /// edit goes inside an inline table written over several lines. the text names the
    /// edit and the reason.
    Inapplicable(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed => write!(f, "the file changed after it was read"),
            Self::Invalid(Failure::Unreadable(problem)) => {
                write!(f, "the document is not valid: {problem}")
            }
            Self::Invalid(Failure::Invalid(problems)) => {
                write!(f, "the document is not valid: {}", problems.join("; "))
            }
            Self::Io {
                path,
                operation,
                kind,
            } => {
                let operation = match operation {
                    Operation::Read => "read",
                    Operation::Write => "write a new file beside",
                    Operation::Replace => "replace",
                };
                write!(f, "could not {operation} {}: {kind}", path.display())
            }
            Self::Inapplicable(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for EditError {}

/// the file as it was read, which a change is made from and compared with just before
/// the file is replaced.
#[derive(Clone)]
pub struct ConfigFile {
    path: PathBuf,
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}

impl fmt::Debug for ConfigFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigFile")
            .field("path", &self.path)
            .field("bytes", &format_args!("{} bytes", self.bytes.len()))
            .finish_non_exhaustive()
    }
}

impl ConfigFile {
    /// reads the file at `path`, which must exist.
    pub fn read(path: &Path) -> Result<Self, EditError> {
        let limit = usize::try_from(MAX_CONFIG_BYTES).unwrap_or(usize::MAX);
        let (bytes, permissions) =
            replace::read_limited(path, limit).map_err(|error| EditError::Io {
                path: path.to_path_buf(),
                operation: Operation::Read,
                kind: error.kind(),
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            bytes,
            permissions,
        })
    }

    /// the bytes that were read: the file's, or its first 1 MiB and one byte when it is
    /// larger, which no edit accepts.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// applies `edits` to the bytes that were read, in order, each against the document
    /// the ones before it left, and replaces the file with the result when it is valid
    /// for `host` under `checkout` and the file still holds the bytes that were read.
    /// when the edits change nothing, nothing is written.
    pub fn edit(
        &self,
        host: Profile,
        checkout: Option<&Path>,
        edits: &[Edit],
    ) -> Result<(), EditError> {
        let output = rewrite(&self.bytes, host, checkout, edits)?;
        if output == self.bytes {
            return Ok(());
        }
        replace::replace(&self.path, &self.bytes, &self.permissions, &output)
    }
}

/// reads the file at `path`, applies `edits` and replaces the file; see
/// [`ConfigFile::edit`].
pub fn edit_config(
    path: &Path,
    host: Profile,
    checkout: Option<&Path>,
    edits: &[Edit],
) -> Result<(), EditError> {
    ConfigFile::read(path)?.edit(host, checkout, edits)
}

/// the bytes of the file after `edits`, checked for `host` under `checkout`.
pub fn rewrite(
    bytes: &[u8],
    host: Profile,
    checkout: Option<&Path>,
    edits: &[Edit],
) -> Result<Vec<u8>, EditError> {
    let not_a_document = || match validate_toml(bytes, host, checkout).outcome {
        Err(failure) => EditError::Invalid(failure),
        Ok(_) => EditError::Inapplicable("the file could not be read as TOML".to_string()),
    };
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(not_a_document());
    }
    let Some((frame, text)) = Frame::split(bytes) else {
        return Err(not_a_document());
    };
    let edited = match document::apply(&text, edits) {
        Ok(edited) => edited,
        Err(Refusal::Syntax) => return Err(not_a_document()),
        Err(Refusal::Inapplicable(reason)) => return Err(EditError::Inapplicable(reason)),
    };
    let output = if edited == text {
        bytes.to_vec()
    } else {
        frame.join(&edited)
    };
    match validate_toml(&output, host, checkout).outcome {
        Ok(_) => Ok(output),
        Err(failure) => Err(EditError::Invalid(failure)),
    }
}
