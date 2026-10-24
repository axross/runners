//! Where the runtime's messages go.

use std::io::Write;

/// A sink for the lines the runtime reports. Callers never put a secret in a line.
pub trait Log {
    fn info(&self, line: &str);
    fn warn(&self, line: &str);
}

/// Writes to standard error, ignoring a stream that cannot be written.
#[derive(Debug, Clone, Copy, Default)]
pub struct StderrLog;

impl Log for StderrLog {
    fn info(&self, line: &str) {
        let _ = writeln!(std::io::stderr(), "{line}");
    }

    fn warn(&self, line: &str) {
        let _ = writeln!(std::io::stderr(), "WARNING: {line}");
    }
}
