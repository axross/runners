//! Where the runtime's messages go, and the masking of the secrets they must not
//! show.

use std::cell::RefCell;
use std::io::Write;

use super::secret::Secret;

/// What a masked secret becomes in a line.
pub const MASK: &str = "<masked>";

/// A sink for the lines the runtime reports. Callers never put a secret in a line on
/// purpose; a sink also replaces every secret it was told to mask, so a secret that
/// reaches a line by another route, such as a container's output, is still hidden.
pub trait Log {
    fn info(&self, line: &str);
    fn warn(&self, line: &str);

    /// Hides `secret` in every later line until [`unmask`](Self::unmask) is called
    /// for it. Masking a secret that is already masked changes nothing.
    fn mask(&self, secret: &Secret);

    /// Stops hiding `secret`.
    fn unmask(&self, secret: &Secret);
}

/// The secrets a sink hides.
#[derive(Debug, Default)]
pub struct Masks {
    secrets: RefCell<Vec<String>>,
}

impl Masks {
    pub fn add(&self, secret: &Secret) {
        let value = secret.expose();
        let mut secrets = self.secrets.borrow_mut();
        if !value.is_empty() && !secrets.iter().any(|held| held == value) {
            secrets.push(value.to_string());
        }
    }

    pub fn remove(&self, secret: &Secret) {
        let mut secrets = self.secrets.borrow_mut();
        if let Some(position) = secrets.iter().position(|held| held == secret.expose()) {
            secrets.swap_remove(position);
        }
    }

    /// `line` with every secret replaced by [`MASK`], the longest first, so a
    /// secret that holds a shorter one is still replaced whole.
    #[must_use]
    pub fn apply(&self, line: &str) -> String {
        let mut secrets = self.secrets.borrow().clone();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets
            .iter()
            .fold(line.to_string(), |text, secret| text.replace(secret, MASK))
    }
}

/// Writes to standard error, ignoring a stream that cannot be written.
#[derive(Debug, Default)]
pub struct StderrLog {
    masks: Masks,
}

impl Log for StderrLog {
    fn info(&self, line: &str) {
        let _ = writeln!(std::io::stderr(), "{}", self.masks.apply(line));
    }

    fn warn(&self, line: &str) {
        let _ = writeln!(std::io::stderr(), "WARNING: {}", self.masks.apply(line));
    }

    fn mask(&self, secret: &Secret) {
        self.masks.add(secret);
    }

    fn unmask(&self, secret: &Secret) {
        self.masks.remove(secret);
    }
}

/// Masks a secret for as long as it lives, however the scope that holds it ends.
pub struct Masked<'a> {
    log: &'a dyn Log,
    secret: &'a Secret,
}

impl<'a> Masked<'a> {
    pub fn new(log: &'a dyn Log, secret: &'a Secret) -> Self {
        log.mask(secret);
        Self { log, secret }
    }
}

impl Drop for Masked<'_> {
    fn drop(&mut self) {
        self.log.unmask(self.secret);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(value: &str) -> Secret {
        Secret::new(value.to_string())
    }

    #[test]
    fn every_masked_secret_is_replaced_and_an_unmasked_one_no_longer_is() {
        let masks = Masks::default();
        let (token, jit) = (secret("ghp_token"), secret("jitvalue"));
        masks.add(&token);
        masks.add(&jit);
        assert_eq!(
            masks.apply("a ghp_token b jitvalue c ghp_token"),
            "a <masked> b <masked> c <masked>"
        );
        masks.remove(&jit);
        assert_eq!(masks.apply("ghp_token jitvalue"), "<masked> jitvalue");
    }

    #[test]
    fn a_secret_that_holds_a_shorter_one_is_replaced_whole() {
        let masks = Masks::default();
        let (short, long) = (secret("abc"), secret("xxabcxx"));
        masks.add(&short);
        masks.add(&long);
        assert_eq!(masks.apply("1 xxabcxx 2 abc"), "1 <masked> 2 <masked>");
    }

    #[test]
    fn a_token_read_again_is_held_once() {
        let masks = Masks::default();
        for _ in 0..3 {
            masks.add(&secret("same"));
        }
        assert_eq!(masks.secrets.borrow().len(), 1);
        masks.remove(&secret("same"));
        assert_eq!(masks.apply("same"), "same");
    }

    #[test]
    fn the_guard_unmasks_when_it_is_dropped() {
        let log = StderrLog::default();
        let value = secret("in-flight");
        {
            let _masked = Masked::new(&log, &value);
            assert_eq!(log.masks.apply("in-flight"), "<masked>");
        }
        assert_eq!(log.masks.apply("in-flight"), "in-flight");
    }
}
