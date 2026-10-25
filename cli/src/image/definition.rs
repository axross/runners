//! The image definition `build` sends to Docker: the files embedded in the binary
//! at compile time, or, in a debug build only, a test definition read from a
//! directory.

use std::path::Path;

/// The runner image's Dockerfile.
pub const DOCKERFILE: &[u8] = include_bytes!("../../../images/actions-runner/Dockerfile");

/// Every file of the image definition, at its path in `images/actions-runner/`,
/// sorted by that path, which is the order the build context writes them in. A unit
/// test fails when this is not exactly the Dockerfile and every file it copies.
pub const EMBEDDED: &[(&str, &[u8])] = &[
    ("Dockerfile", DOCKERFILE),
    (
        "install-ndks.sh",
        include_bytes!("../../../images/actions-runner/install-ndks.sh"),
    ),
    (
        "runner-diagnostics.cpp",
        include_bytes!("../../../images/actions-runner/runner-diagnostics.cpp"),
    ),
];

/// The most files, and the most bytes in one file, a test definition may hold.
#[cfg(debug_assertions)]
const MAX_TEST_FILES: usize = 16;
#[cfg(debug_assertions)]
const MAX_TEST_FILE_BYTES: u64 = 1024 * 1024;

/// One file of a definition, at its path in the build context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// The files a build context holds, sorted by path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub files: Vec<DefinitionFile>,
}

impl Definition {
    /// The definition embedded in this binary.
    #[must_use]
    pub fn embedded() -> Self {
        Self {
            files: EMBEDDED
                .iter()
                .map(|(path, bytes)| DefinitionFile {
                    path: (*path).to_string(),
                    bytes: bytes.to_vec(),
                })
                .collect(),
        }
    }
}

/// The definition `build` sends.
///
/// A release build always returns the embedded definition and ignores
/// `requested`, as it ignores the other test overrides, so a release binary builds
/// only what it embeds. A debug build reads `requested` when given one, for the
/// real-Docker test: every regular file directly in that directory.
///
/// # Errors
///
/// A test definition that cannot be read, or holds anything but a few small
/// regular files.
pub fn resolve_definition(requested: Option<&Path>) -> Result<Definition, String> {
    #[cfg(debug_assertions)]
    if let Some(directory) = requested {
        return read_test_definition(directory);
    }
    #[cfg(not(debug_assertions))]
    let _ = requested;
    Ok(Definition::embedded())
}

#[cfg(debug_assertions)]
fn read_test_definition(directory: &Path) -> Result<Definition, String> {
    use std::io::Read;

    let unreadable = |error: std::io::Error| {
        format!(
            "the test definition {} cannot be read: {error}",
            directory.display()
        )
    };
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        let path = entry.file_name().into_string().map_err(|name| {
            format!("the test definition holds a name that is not UTF-8: {name:?}")
        })?;
        if !entry.file_type().map_err(unreadable)?.is_file() {
            return Err(format!(
                "the test definition holds {path}, which is not a regular file"
            ));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(entry.path())
            .map_err(unreadable)?
            .take(MAX_TEST_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(unreadable)?;
        if bytes.len() as u64 > MAX_TEST_FILE_BYTES {
            return Err(format!("the test definition's {path} is too large"));
        }
        files.push(DefinitionFile { path, bytes });
    }
    if files.len() > MAX_TEST_FILES {
        return Err("the test definition holds too many files".to_string());
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Definition { files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_files_are_sorted_by_path_and_equal_the_checkout() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../images/actions-runner");
        let paths: Vec<&str> = EMBEDDED.iter().map(|(path, _)| *path).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted);
        for (path, bytes) in EMBEDDED {
            assert_eq!(
                &std::fs::read(directory.join(path)).unwrap(),
                bytes,
                "{path}"
            );
        }
    }

    #[test]
    fn without_a_request_the_definition_is_the_embedded_one() {
        assert_eq!(resolve_definition(None).unwrap(), Definition::embedded());
    }
}
