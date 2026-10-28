//! `identity.txt`: the fixed allowlist of a container's inspect fields; the readback
//! is capped at 1 KiB and must match one fixed shape, so an inspect that prints
//! anything else, such as an unfiltered value, is refused without being repeated.

use std::sync::OnceLock;

use regex::Regex;

/// the most bytes of inspect output read; output that fills it is refused.
pub const MAX_IDENTITY_BYTES: usize = 1024;

/// the first line of `identity.txt`, naming the fields of the second.
pub const IDENTITY_HEADER: &str = "image|nano_cpus|memory_bytes|memory_swap_bytes|cpu_quota|cpu_period|allowed_cpus|container_oom_killed|container_exit_code";

const SHAPE: &str = r"\Asha256:[0-9a-f]{64}\|[0-9]+\|[0-9]+\|-?[0-9]+\|-?[0-9]+\|[0-9]+\|[0-9,-]*\|(true|false)\|-?[0-9]+\z";

/// why a readback was refused; neither variant carries the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityError {
    /// the output reached [`MAX_IDENTITY_BYTES`].
    OutputLimit,
    /// the output is not the allowlisted fields in their shape.
    Shape,
}

fn shape() -> Option<&'static Regex> {
    static SHAPE_REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    SHAPE_REGEX.get_or_init(|| Regex::new(SHAPE).ok()).as_ref()
}

/// the text of `identity.txt` for inspect `output`: the header line, then the trimmed
/// readback.
pub fn identity_text(output: &[u8]) -> Result<String, IdentityError> {
    if output.len() >= MAX_IDENTITY_BYTES {
        return Err(IdentityError::OutputLimit);
    }
    let text = std::str::from_utf8(output)
        .ok()
        .filter(|text| text.is_ascii())
        .map(str::trim)
        .ok_or(IdentityError::Shape)?;
    if !shape().is_some_and(|shape| shape.is_match(text)) {
        return Err(IdentityError::Shape);
    }
    Ok(format!("{IDENTITY_HEADER}\n{text}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the readback the PowerShell test's fake `docker` prints.
    fn readback() -> String {
        format!(
            "sha256:{}|1500000000|900|900|150000|100000|1,3-4|false|7",
            "0".repeat(64)
        )
    }

    #[test]
    fn the_allowlisted_readback_is_recorded_under_its_header() {
        let text = identity_text(format!("{}\n", readback()).as_bytes()).unwrap();
        assert_eq!(text, format!("{IDENTITY_HEADER}\n{}\n", readback()));
    }

    #[test]
    fn an_unfiltered_value_is_refused_without_being_repeated() {
        let marker = "4f0c3a51b8e24d6f9a7e2c1d0b9a8f7e";
        let error = identity_text(format!("{marker}\n").as_bytes()).unwrap_err();
        assert_eq!(error, IdentityError::Shape);
        assert!(!format!("{error:?}").contains(marker));
    }

    #[test]
    fn output_of_a_kibibyte_or_more_is_refused() {
        let mut long = readback();
        long.push_str(&" ".repeat(MAX_IDENTITY_BYTES - long.len()));
        assert_eq!(
            identity_text(long.as_bytes()),
            Err(IdentityError::OutputLimit)
        );
        long.pop();
        assert!(
            identity_text(long.as_bytes()).is_ok(),
            "1023 bytes are read"
        );
    }

    #[test]
    fn a_wrong_shape_is_refused() {
        for wrong in [
            readback().replace("false", "maybe"),
            readback().replace("sha256:", "sha512:"),
            format!("{}|extra", readback()),
            readback().replace("|1500000000|", "|-1|"),
            format!("{}\nsecond line", readback()),
            String::new(),
        ] {
            assert_eq!(
                identity_text(wrong.as_bytes()),
                Err(IdentityError::Shape),
                "{wrong:?}"
            );
        }
    }
}
