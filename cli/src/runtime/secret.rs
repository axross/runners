//! a value that must not reach a log line, a command line, or an error message.

use std::fmt;

/// a token or JIT configuration with a redacted `Debug` and no `Display`, so a
/// formatting macro cannot print it by accident.
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// the value, for the one place that hands it to its destination.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debug_form_hides_the_value() {
        let secret = Secret::new("ghp_value".to_string());
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(format!("{secret:#?}"), "Secret(<redacted>)");
        assert_eq!(format!("{:?}", Some(&secret)), "Some(Secret(<redacted>))");
        assert_eq!(secret.expose(), "ghp_value");
    }
}
