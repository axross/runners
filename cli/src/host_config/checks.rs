//! the rules that check the files, behind a seam: the caller supplies them, so the
//! checkout rule is the caller's and the loader never decides it.

use std::path::Path;

use crate::convert::{ConversionFailure, convert_to_toml};
use crate::profile::Profile;
use crate::runtime::clock::Clock;
use crate::validate::{CheckoutRule, Validation, validate_toml_with, validate_with};

/// the checks of the two files and the conversion between them.
pub trait Checks {
    /// checks the bytes of `config.toml` for `profile`.
    fn check_toml(&self, bytes: &[u8], profile: Profile) -> Validation;

    /// checks the bytes of `config.json` for `profile` under today's JSON rules.
    fn check_json(&self, bytes: &[u8], profile: Profile) -> Validation;

    /// converts the bytes of `config.json`, which `check_json` accepted, to the text of
    /// `config.toml`, or says why that text is not to be written. `source` is the path
    /// of `config.json`.
    fn convert<C: Clock>(
        &self,
        json: &[u8],
        profile: Profile,
        source: &Path,
        clock: &C,
    ) -> Result<String, ConversionFailure>;
}

/// the library's own rules, under one checkout rule.
#[derive(Debug, Clone, Copy, Default)]
pub struct StandardChecks<'a> {
    /// where the checkout that a diagnostic directory must lie outside comes from.
    pub rule: CheckoutRule<'a>,
}

impl Checks for StandardChecks<'_> {
    fn check_toml(&self, bytes: &[u8], profile: Profile) -> Validation {
        validate_toml_with(bytes, profile, self.rule)
    }

    fn check_json(&self, bytes: &[u8], profile: Profile) -> Validation {
        validate_with(bytes, profile, self.rule)
    }

    fn convert<C: Clock>(
        &self,
        json: &[u8],
        profile: Profile,
        source: &Path,
        clock: &C,
    ) -> Result<String, ConversionFailure> {
        convert_to_toml(json, profile, source, self.rule, clock)
    }
}
