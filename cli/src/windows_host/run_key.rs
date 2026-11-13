//! the current user's Run key value that starts `marshal-app.exe` at sign-in, and
//! whether Windows' Startup apps setting turned it off. The agent writes and removes
//! only its own value, and reads `StartupApproved\Run` without writing there.

use std::fmt;
use std::path::Path;

/// the argument that starts the app with its icon and without its window.
pub const SIGN_IN_ARGUMENT: &str = "--sign-in";

/// the value's name: `Marshal` in a release build, and a test name in a debug build,
/// so a test never touches the installed copy's value.
pub const VALUE_NAME: &str = if cfg!(debug_assertions) {
    "MarshalTest"
} else {
    "Marshal"
};

/// the longest command line a Run value may hold, in UTF-16 code units.
pub const LONGEST_COMMAND: usize = 260;

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const STARTUP_APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// the sign-in item as the snapshot reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    Registered,
    NotRegistered,
    /// the value is there, and Windows' Startup apps setting keeps it from running.
    TurnedOffInStartupApps,
    /// the value could not be written, removed or read; the text says which.
    Failed(String),
}

/// why a value was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    TooLong {
        length: usize,
    },
    /// a path with a quotation mark cannot be quoted on a command line.
    Quote,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { length } => write!(
                f,
                "the sign-in command would be {length} characters, and Windows allows {LONGEST_COMMAND}"
            ),
            Self::Quote => f.write_str("the program's path holds a quotation mark"),
        }
    }
}

/// the value's command line: the executable's path in quotation marks, then the
/// sign-in argument.
pub fn command(executable: &Path) -> Result<String, CommandError> {
    let path = executable.to_string_lossy();
    if path.contains('"') {
        return Err(CommandError::Quote);
    }
    let command = format!("\"{path}\" {SIGN_IN_ARGUMENT}");
    let length = command.encode_utf16().count();
    if length > LONGEST_COMMAND {
        return Err(CommandError::TooLong { length });
    }
    Ok(command)
}

/// whether a `StartupApproved\Run` value says the entry is turned off. Windows does
/// not document the value; its first byte is even while the entry is on and odd
/// once the user turns it off, and an entry with no value is on.
#[must_use]
pub fn is_turned_off(approved: Option<&[u8]>) -> bool {
    approved
        .and_then(<[u8]>::first)
        .is_some_and(|first| first & 1 == 1)
}

#[cfg(windows)]
mod system {
    use std::path::Path;

    use winsafe::{self as w, co};

    use super::{RUN_KEY, STARTUP_APPROVED_KEY, SignIn, VALUE_NAME, command, is_turned_off};

    fn open(sub_key: &str, access: co::KEY) -> w::SysResult<w::guard::RegCloseKeyGuard> {
        w::HKEY::CURRENT_USER.RegOpenKeyEx(Some(sub_key), co::REG_OPTION::NoValue, access)
    }

    /// writes the value for `executable`, or replaces one an older or moved copy left.
    pub fn register(executable: &Path) -> SignIn {
        let command = match command(executable) {
            Ok(command) => command,
            Err(error) => return SignIn::Failed(error.to_string()),
        };
        let written = open(RUN_KEY, co::KEY::SET_VALUE)
            .and_then(|key| key.RegSetValueEx(Some(VALUE_NAME), w::RegistryValue::Sz(command)));
        match written {
            Ok(()) => read(),
            Err(error) => {
                SignIn::Failed(format!("the Run key value could not be written: {error}"))
            }
        }
    }

    /// removes the value; one that is not there stays absent.
    pub fn unregister() -> SignIn {
        let removed =
            open(RUN_KEY, co::KEY::SET_VALUE).and_then(|key| key.RegDeleteValue(Some(VALUE_NAME)));
        match removed {
            Ok(()) | Err(co::ERROR::FILE_NOT_FOUND) => SignIn::NotRegistered,
            Err(error) => {
                SignIn::Failed(format!("the Run key value could not be removed: {error}"))
            }
        }
    }

    /// whether the value is there, and whether Startup apps turned it off.
    pub fn read() -> SignIn {
        let value = open(RUN_KEY, co::KEY::QUERY_VALUE)
            .and_then(|key| key.RegQueryValueEx(Some(VALUE_NAME)));
        match value {
            Ok(_) => {}
            Err(co::ERROR::FILE_NOT_FOUND) => return SignIn::NotRegistered,
            Err(error) => {
                return SignIn::Failed(format!("the Run key value could not be read: {error}"));
            }
        }
        let approved = open(STARTUP_APPROVED_KEY, co::KEY::QUERY_VALUE)
            .and_then(|key| key.RegQueryValueEx(Some(VALUE_NAME)));
        match approved {
            Ok(w::RegistryValue::Binary(bytes)) if is_turned_off(Some(&bytes)) => {
                SignIn::TurnedOffInStartupApps
            }
            _ => SignIn::Registered,
        }
    }
}

#[cfg(windows)]
pub use system::{read, register, unregister};

/// the sign-in item the agent keeps in step with `start_at_login`.
pub trait SignInItem {
    /// writes the value for `executable`, or corrects one an older copy left.
    fn register(&self, executable: &Path) -> SignIn;
    /// removes the value.
    fn unregister(&self) -> SignIn;
}

/// the current user's Run key value.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunKey;

#[cfg(windows)]
impl SignInItem for RunKey {
    fn register(&self, executable: &Path) -> SignIn {
        register(executable)
    }

    fn unregister(&self) -> SignIn {
        unregister()
    }
}

/// a sign-in item held in memory, for the tests; a debug or test build only.
#[cfg(any(debug_assertions, test))]
#[derive(Debug, Default)]
pub struct MemorySignIn {
    /// the command the value would hold, while registered.
    pub command: std::cell::RefCell<Option<String>>,
    /// makes every write fail with this text while it is set.
    pub failure: std::cell::RefCell<Option<String>>,
}

#[cfg(any(debug_assertions, test))]
impl SignInItem for MemorySignIn {
    fn register(&self, executable: &Path) -> SignIn {
        if let Some(text) = self.failure.borrow().clone() {
            return SignIn::Failed(text);
        }
        match command(executable) {
            Ok(command) => {
                *self.command.borrow_mut() = Some(command);
                SignIn::Registered
            }
            Err(error) => SignIn::Failed(error.to_string()),
        }
    }

    fn unregister(&self) -> SignIn {
        if let Some(text) = self.failure.borrow().clone() {
            return SignIn::Failed(text);
        }
        *self.command.borrow_mut() = None;
        SignIn::NotRegistered
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_command_quotes_the_path_and_ends_with_the_sign_in_argument() {
        let path =
            PathBuf::from(r"C:\Users\Ada Lovelace\AppData\Local\Programs\marshal\marshal-app.exe");
        assert_eq!(
            command(&path).unwrap(),
            r#""C:\Users\Ada Lovelace\AppData\Local\Programs\marshal\marshal-app.exe" --sign-in"#
        );
    }

    #[test]
    fn a_command_of_260_utf16_units_is_written_and_one_more_is_refused() {
        let room = LONGEST_COMMAND - "\"\" --sign-in".len();
        let fits = PathBuf::from(format!("C:\\{}", "a".repeat(room - 3)));
        assert_eq!(
            command(&fits).unwrap().encode_utf16().count(),
            LONGEST_COMMAND
        );
        let over = PathBuf::from(format!("C:\\{}", "a".repeat(room - 2)));
        assert_eq!(
            command(&over),
            Err(CommandError::TooLong {
                length: LONGEST_COMMAND + 1
            })
        );
        // a character outside the basic plane is two UTF-16 units, not one
        let wide = PathBuf::from(format!("C:\\{}\u{1F600}", "a".repeat(room - 4)));
        assert_eq!(
            command(&wide),
            Err(CommandError::TooLong {
                length: LONGEST_COMMAND + 1
            })
        );
    }

    #[test]
    fn a_path_with_a_quotation_mark_is_refused() {
        assert_eq!(
            command(Path::new("C:\\a\"b\\m.exe")),
            Err(CommandError::Quote)
        );
    }

    #[test]
    fn startup_apps_turns_the_entry_off_with_an_odd_first_byte() {
        assert!(!is_turned_off(None));
        assert!(!is_turned_off(Some(&[])));
        assert!(!is_turned_off(Some(&[2, 0, 0, 0])));
        assert!(!is_turned_off(Some(&[6, 0, 0, 0])));
        assert!(is_turned_off(Some(&[3, 0, 0, 0])));
        assert!(is_turned_off(Some(&[7, 0, 0, 0])));
    }

    #[test]
    fn a_debug_build_never_names_the_installed_value() {
        assert_ne!(VALUE_NAME, "Marshal");
    }

    #[cfg(windows)]
    #[test]
    fn the_value_is_registered_read_back_and_removed_under_the_test_name() {
        let executable = std::env::current_exe().unwrap();
        assert_eq!(register(&executable), SignIn::Registered);
        assert_eq!(read(), SignIn::Registered);
        assert_eq!(unregister(), SignIn::NotRegistered);
        assert_eq!(read(), SignIn::NotRegistered);
        assert_eq!(unregister(), SignIn::NotRegistered);
    }
}
