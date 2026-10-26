//! Reading a pool's token file.
//!
//! The file is read again at every registration, so a rotated token applies to the
//! next job without a restart. The token is trimmed and never printed. On macOS the
//! file must also be private: a regular file owned by the current user with mode
//! `0600`. On Windows its access control list is not checked, because no safe call
//! reads it.

use std::fmt;
use std::io::{self, Read};
use std::path::Path;

use super::secret::Secret;

/// A token file is a few dozen bytes; this only stops a mistaken path from being
/// read whole.
pub const MAX_TOKEN_FILE_BYTES: u64 = 1024 * 1024;

/// Why a token file cannot be used. The text names the path and never the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenFileError {
    NotFound(String),
    Empty(String),
    TooLarge(String),
    NotText(String),
    /// The file is not private to the current user; the text says how.
    NotPrivate(String),
    Unreadable(String, io::ErrorKind),
    /// The current user's ID, which the privacy check needs, cannot be read.
    CurrentUserUnknown(io::ErrorKind),
}

impl fmt::Display for TokenFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "token file not found at {path}"),
            Self::Empty(path) => write!(f, "token file at {path} is empty"),
            Self::TooLarge(path) => write!(
                f,
                "token file at {path} is larger than {MAX_TOKEN_FILE_BYTES} bytes"
            ),
            Self::NotText(path) => write!(f, "token file at {path} is not UTF-8 text"),
            Self::NotPrivate(detail) => write!(
                f,
                "token file is not private: {detail}; it must be a regular file owned by the \
                 current user with mode 0600"
            ),
            Self::Unreadable(path, kind) => {
                write!(f, "token file at {path} cannot be read ({kind})")
            }
            Self::CurrentUserUnknown(kind) => write!(
                f,
                "the current user's ID cannot be read from a file in the temporary \
                 directory ({kind}), so no token file can be checked"
            ),
        }
    }
}

impl std::error::Error for TokenFileError {}

/// Reads and trims the token. A file that is missing, is not a regular file, is
/// blank after trimming, or is not UTF-8 text is an error, and so on macOS is one
/// that is not private.
///
/// # Errors
///
/// See [`TokenFileError`].
pub fn read_token(path: &Path) -> Result<Secret, TokenFileError> {
    let shown = path.display().to_string();
    let file = std::fs::File::open(path).map_err(|error| classify(&shown, &error))?;
    let metadata = file.metadata().map_err(|error| classify(&shown, &error))?;
    if !metadata.is_file() {
        return Err(TokenFileError::NotFound(shown));
    }
    #[cfg(target_os = "macos")]
    require_private(path, &metadata, current_uid()?)?;
    let mut bytes = Vec::new();
    file.take(MAX_TOKEN_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| classify(&shown, &error))?;
    if bytes.len() as u64 > MAX_TOKEN_FILE_BYTES {
        return Err(TokenFileError::TooLarge(shown));
    }
    let text = String::from_utf8(bytes).map_err(|_| TokenFileError::NotText(shown.clone()))?;
    let token = text.trim_start_matches('\u{feff}').trim();
    if token.is_empty() {
        return Err(TokenFileError::Empty(shown));
    }
    Ok(Secret::new(token.to_string()))
}

/// Checks the open file's metadata, so the answer is about the file that is read.
/// Only macOS calls it; the tests on the other Unix systems check it too.
#[cfg(all(unix, any(target_os = "macos", test)))]
fn require_private(
    path: &Path,
    metadata: &std::fs::Metadata,
    uid: u32,
) -> Result<(), TokenFileError> {
    crate::private::check_private_file(path, metadata, uid)
        .map_err(|error| TokenFileError::NotPrivate(error.to_string()))
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<u32, TokenFileError> {
    crate::private::current_uid().map_err(|error| TokenFileError::CurrentUserUnknown(error.kind()))
}

fn classify(path: &str, error: &io::Error) -> TokenFileError {
    if error.kind() == io::ErrorKind::NotFound {
        TokenFileError::NotFound(path.to_string())
    } else {
        TokenFileError::Unreadable(path.to_string(), error.kind())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("marshal-token-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token");
        std::fs::write(&file, bytes).unwrap();
        #[cfg(unix)]
        set_mode(&file, 0o600);
        (dir, file)
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn the_token_is_trimmed_and_a_byte_order_mark_is_ignored() {
        let (dir, file) = temp("trim", b"\xEF\xBB\xBF  ghp_abc \r\n");
        assert_eq!(read_token(&file).unwrap().expose(), "ghp_abc");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn blank_missing_and_directory_paths_are_errors_that_name_the_path_only() {
        let (dir, file) = temp("blank", b" \t\r\n");
        let error = read_token(&file).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("token file at {} is empty", file.display())
        );
        assert!(matches!(
            read_token(&dir.join("absent")),
            Err(TokenFileError::NotFound(_))
        ));
        assert!(matches!(
            read_token(&dir),
            Err(TokenFileError::NotFound(_)) | Err(TokenFileError::Unreadable(..))
        ));
        std::fs::write(&file, [0xFF, 0xFE, b'a']).unwrap();
        assert!(matches!(read_token(&file), Err(TokenFileError::NotText(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_current_user_that_cannot_be_read_is_reported_as_itself_and_names_no_token_file() {
        let text = TokenFileError::CurrentUserUnknown(io::ErrorKind::PermissionDenied).to_string();
        assert!(
            text.starts_with("the current user's ID cannot be read"),
            "{text}"
        );
        assert!(text.contains("permission denied"), "{text}");
        assert!(!text.contains("token file at"), "{text}");
    }

    #[test]
    fn a_file_over_the_limit_is_refused() {
        let big = vec![b'a'; usize::try_from(MAX_TOKEN_FILE_BYTES).unwrap() + 1];
        let (dir, file) = temp("big", &big);
        assert!(matches!(
            read_token(&file),
            Err(TokenFileError::TooLarge(_))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_is_not_0600_or_not_the_users_own_is_not_private() {
        let (dir, file) = temp("private", b"ghp_abc\n");
        let uid = crate::private::current_uid().unwrap();
        let metadata = std::fs::metadata(&file).unwrap();
        assert_eq!(require_private(&file, &metadata, uid), Ok(()));
        let error = require_private(&file, &metadata, uid.wrapping_add(1)).unwrap_err();
        assert!(matches!(error, TokenFileError::NotPrivate(_)), "{error}");
        assert!(error.to_string().contains("owned by user"), "{error}");
        for mode in [0o644, 0o640, 0o400] {
            set_mode(&file, mode);
            let metadata = std::fs::metadata(&file).unwrap();
            let error = require_private(&file, &metadata, uid).unwrap_err();
            assert!(error.to_string().contains("0600"), "{mode:o}: {error}");
            assert!(!error.to_string().contains("ghp_abc"), "{error}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_a_token_file_that_is_not_private_is_refused() {
        let (dir, file) = temp("macos", b"ghp_abc\n");
        assert_eq!(read_token(&file).unwrap().expose(), "ghp_abc");
        set_mode(&file, 0o644);
        assert!(matches!(
            read_token(&file),
            Err(TokenFileError::NotPrivate(_))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
