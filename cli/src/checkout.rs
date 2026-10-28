//! finding the project checkout that private directories must lie outside.

use std::path::{Component, Path, PathBuf};

/// returns the first directory at or above the configuration file that holds both
/// `.git` and `cli/Cargo.toml`, so a dotfiles repository at the home directory is not
/// taken for it, walking lexically without resolving a link.
#[must_use]
pub fn find(config: &Path) -> Option<PathBuf> {
    let absolute = lexical_absolute(config)?;
    absolute
        .ancestors()
        .skip(1)
        .find(|directory| is_checkout(directory))
        .map(Path::to_path_buf)
}

fn is_checkout(directory: &Path) -> bool {
    directory.join(".git").exists() && directory.join("cli").join("Cargo.toml").is_file()
}

/// makes the path absolute and resolves `.` and `..` components lexically.
fn lexical_absolute(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other.as_os_str()),
        }
    }
    Some(resolved)
}
