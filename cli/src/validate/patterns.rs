//! the value patterns of the host configuration; each matches the whole value, as
//! the PowerShell validator's `\z` does, so a trailing newline never passes.

use std::sync::LazyLock;

use regex::Regex;

#[expect(
    clippy::expect_used,
    reason = "every source is a constant that the unit tests compile"
)]
fn compile(source: &str) -> Regex {
    Regex::new(source).expect("a constant pattern is valid")
}

static NAME: LazyLock<Regex> = LazyLock::new(|| compile(r"^[a-z0-9][a-z0-9-]*\z"));
static OWNER: LazyLock<Regex> = LazyLock::new(|| compile(r"^[A-Za-z0-9][A-Za-z0-9-]*\z"));
static REPOSITORY: LazyLock<Regex> = LazyLock::new(|| compile(r"^[A-Za-z0-9_.-]+\z"));
static IMAGE_NAME: LazyLock<Regex> = LazyLock::new(|| compile(r"^[A-Za-z0-9][A-Za-z0-9_.:/@-]*\z"));
static LABEL: LazyLock<Regex> = LazyLock::new(|| compile(r"^[A-Za-z0-9][A-Za-z0-9._:/-]*\z"));
static MOUNT_PATH: LazyLock<Regex> = LazyLock::new(|| compile(r"^/[A-Za-z0-9_./-]+\z"));
// a drive letter and a backslash, or a UNC path; a path that is only rooted, such
// as C:name or \name, resolves against a directory or drive the host does not control.
static WINDOWS_TOKEN_PATH: LazyLock<Regex> = LazyLock::new(|| compile(r"^(?:[A-Za-z]:\\|\\\\)\S"));
static WINDOWS_DIRECTORY: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"\A[A-Za-z]:\\[^\x00-\x1f"<>|?*:/]+\z"#));

#[must_use]
pub fn is_name(text: &str) -> bool {
    NAME.is_match(text)
}

#[must_use]
pub fn is_owner(text: &str) -> bool {
    OWNER.is_match(text)
}

#[must_use]
pub fn is_repository(text: &str) -> bool {
    text != "." && text != ".." && REPOSITORY.is_match(text)
}

#[must_use]
pub fn is_image_name(text: &str) -> bool {
    IMAGE_NAME.is_match(text)
}

#[must_use]
pub fn is_label(text: &str) -> bool {
    LABEL.is_match(text)
}

#[must_use]
pub fn is_mount_path(text: &str) -> bool {
    MOUNT_PATH.is_match(text)
}

#[must_use]
pub fn is_windows_token_path(text: &str) -> bool {
    WINDOWS_TOKEN_PATH.is_match(text)
}

#[must_use]
pub fn is_windows_directory(text: &str) -> bool {
    WINDOWS_DIRECTORY.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_compiles() {
        let _ = (
            is_name("a"),
            is_owner("a"),
            is_repository("a"),
            is_image_name("a"),
            is_label("a"),
            is_mount_path("/a"),
            is_windows_token_path("C:\\a"),
            is_windows_directory("C:\\a"),
        );
    }

    #[test]
    fn a_trailing_newline_never_passes() {
        assert!(!is_name("abc\n"));
        assert!(!is_owner("abc\n"));
        assert!(!is_repository("abc\n"));
        assert!(!is_image_name("abc\n"));
        assert!(!is_label("abc\n"));
        assert!(!is_mount_path("/abc\n"));
        assert!(!is_windows_directory("C:\\abc\n"));
    }

    #[test]
    fn a_repository_is_not_a_dot_component() {
        assert!(!is_repository("."));
        assert!(!is_repository(".."));
        assert!(is_repository("..."));
        assert!(is_repository(".github"));
    }

    #[test]
    fn a_windows_token_path_needs_a_drive_or_unc_prefix_and_a_visible_character() {
        assert!(is_windows_token_path("C:\\t"));
        assert!(is_windows_token_path("\\\\server\\share"));
        assert!(!is_windows_token_path("C:t"));
        assert!(!is_windows_token_path("\\t"));
        assert!(!is_windows_token_path("C:\\ "));
        assert!(!is_windows_token_path("C:\\"));
    }
}
