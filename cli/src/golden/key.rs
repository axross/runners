//! the check of Docker's apt signing key against the recorded fingerprint, over the
//! output of `gpg --show-keys --with-colons` that the guest printed.

use std::fmt;

use super::pins::DOCKER_KEY_FINGERPRINT;

/// the most bytes of listing accepted; a key with one subkey lists in about 350.
pub const MAX_LISTING_BYTES: usize = 8 * 1024;

const FINGERPRINT_DIGITS: usize = 40;
const RECORD_FIELDS: usize = 11;
const FINGERPRINT_FIELD: usize = 9;
const RECORD_KINDS: [&str; 4] = ["pub", "fpr", "uid", "sub"];

/// proof that a listing passed [`check_key_listing`]; the call that installs
/// packages from Docker's repository takes one, so it cannot be made before the
/// check.
#[derive(Debug)]
pub struct VerifiedKey {
    _checked: (),
}

/// why a listing was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyCheckError {
    TooLarge,
    /// the listing is not in the form `gpg` prints for one key; the text says how.
    Malformed(&'static str),
    /// the primary key is another one than the recorded key.
    Mismatch {
        found: String,
    },
}

impl fmt::Display for KeyCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(
                f,
                "the key listing is longer than {MAX_LISTING_BYTES} bytes, so the Docker signing key was not accepted"
            ),
            Self::Malformed(reason) => write!(
                f,
                "the key listing is not the form of one key ({reason}), so the Docker signing key was not accepted"
            ),
            Self::Mismatch { found } => write!(
                f,
                "the Docker signing key has the fingerprint {found}, not the recorded {DOCKER_KEY_FINGERPRINT}"
            ),
        }
    }
}

impl std::error::Error for KeyCheckError {}

/// accepts the listing of exactly one key, whose primary key's fingerprint is
/// [`DOCKER_KEY_FINGERPRINT`].
///
/// The form is `pub`, then its `fpr`, then any `uid`, `sub` and `fpr` records, each
/// ending in a newline. A second primary key is refused, because apt trusts every
/// key in the file.
pub fn check_key_listing(listing: &str) -> Result<VerifiedKey, KeyCheckError> {
    if listing.len() > MAX_LISTING_BYTES {
        return Err(KeyCheckError::TooLarge);
    }
    if listing.is_empty() {
        return Err(KeyCheckError::Malformed("it is empty"));
    }
    if !listing.ends_with('\n')
        || listing
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\n')
    {
        return Err(KeyCheckError::Malformed(
            "it has a control character or an unfinished line",
        ));
    }
    let records: Vec<&str> = listing.lines().collect();
    for record in &records {
        let kind = record.split(':').next().unwrap_or_default();
        if !RECORD_KINDS.contains(&kind) {
            return Err(KeyCheckError::Malformed("it has an unexpected record"));
        }
    }
    let primary_keys = records
        .iter()
        .filter(|record| record.starts_with("pub:"))
        .count();
    if primary_keys != 1 {
        return Err(KeyCheckError::Malformed("it does not list one primary key"));
    }
    let found = match records.as_slice() {
        [first, second, ..] if first.starts_with("pub:") => fingerprint_of(second)?,
        _ => {
            return Err(KeyCheckError::Malformed(
                "it does not start with the primary key and its fingerprint",
            ));
        }
    };
    if found != DOCKER_KEY_FINGERPRINT {
        return Err(KeyCheckError::Mismatch {
            found: found.to_string(),
        });
    }
    Ok(VerifiedKey { _checked: () })
}

/// the fingerprint in an `fpr` record, whose other fields are all empty.
fn fingerprint_of(record: &str) -> Result<&str, KeyCheckError> {
    let fields: Vec<&str> = record.split(':').collect();
    let well_formed = fields.len() == RECORD_FIELDS
        && fields.first() == Some(&"fpr")
        && fields
            .iter()
            .enumerate()
            .all(|(index, field)| index == 0 || index == FINGERPRINT_FIELD || field.is_empty());
    let Some(fingerprint) = fields.get(FINGERPRINT_FIELD).filter(|_| well_formed) else {
        return Err(KeyCheckError::Malformed(
            "the primary key is not followed by a fingerprint record",
        ));
    };
    let digits_ok = fingerprint.len() == FINGERPRINT_DIGITS
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte));
    if !digits_ok {
        return Err(KeyCheckError::Malformed(
            "the fingerprint is not 40 uppercase hexadecimal digits",
        ));
    }
    Ok(fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_LISTING: &str = include_str!("fixtures/docker-key-listing.txt");
    const OTHER_FINGERPRINT: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

    fn refused(listing: &str) -> KeyCheckError {
        check_key_listing(listing).unwrap_err()
    }

    #[test]
    fn accepts_the_listing_gpg_printed_for_the_recorded_key() {
        assert!(check_key_listing(KEY_LISTING).is_ok());
    }

    #[test]
    fn refuses_another_primary_key_and_names_it() {
        let listing = KEY_LISTING.replacen(DOCKER_KEY_FINGERPRINT, OTHER_FINGERPRINT, 1);
        assert_eq!(
            refused(&listing),
            KeyCheckError::Mismatch {
                found: OTHER_FINGERPRINT.to_string()
            }
        );
        assert!(refused(&listing).to_string().contains(OTHER_FINGERPRINT));
    }

    #[test]
    fn the_subkey_fingerprint_is_not_the_primary_key() {
        let listing = KEY_LISTING.replacen(
            DOCKER_KEY_FINGERPRINT,
            "D3306A018370199E527AE7997EA0A9C3F273FCD8",
            1,
        );
        assert!(matches!(refused(&listing), KeyCheckError::Mismatch { .. }));
    }

    #[test]
    fn the_recorded_fingerprint_on_a_subkey_alone_is_refused() {
        let listing = KEY_LISTING
            .replacen(DOCKER_KEY_FINGERPRINT, OTHER_FINGERPRINT, 1)
            .replacen(
                "D3306A018370199E527AE7997EA0A9C3F273FCD8",
                DOCKER_KEY_FINGERPRINT,
                1,
            );
        assert!(matches!(refused(&listing), KeyCheckError::Mismatch { .. }));
    }

    #[test]
    fn refuses_a_listing_with_a_second_primary_key() {
        let second = format!(
            "pub:-:4096:1:AAAAAAAAAAAAAAAA:1:::-:::scESC:::::::\nfpr:::::::::{OTHER_FINGERPRINT}:\n"
        );
        for listing in [
            format!("{KEY_LISTING}{second}"),
            format!("{second}{KEY_LISTING}"),
        ] {
            assert!(matches!(refused(&listing), KeyCheckError::Malformed(_)));
        }
    }

    #[test]
    fn refuses_what_is_not_the_form_gpg_prints() {
        let without_newline = KEY_LISTING.trim_end_matches('\n');
        let lowercase = KEY_LISTING.replacen(
            DOCKER_KEY_FINGERPRINT,
            &DOCKER_KEY_FINGERPRINT.to_lowercase(),
            1,
        );
        let truncated_fingerprint = KEY_LISTING.replacen(DOCKER_KEY_FINGERPRINT, "9DC8", 1);
        let extra_field = KEY_LISTING.replacen(
            &format!("{DOCKER_KEY_FINGERPRINT}:"),
            &format!("{DOCKER_KEY_FINGERPRINT}::x"),
            1,
        );
        let cases = [
            "",
            "\n",
            "gpg: no valid OpenPGP data found.\n",
            "pub:-:4096:1:8D81803C0EBFCD88:1487788586:::-:::escaESCA::::::23::0:\n",
            "uid:-::::1487792064::::Docker Release::::::::::0:\nfpr:::::::::9DC858229FC7DD38854AE2D88D81803C0EBFCD88:\n",
            without_newline,
            &KEY_LISTING.replace('\n', "\r\n"),
            &KEY_LISTING.replacen("uid:", "sig:", 1),
            &lowercase,
            &truncated_fingerprint,
            &extra_field,
        ];
        for listing in cases {
            assert!(
                matches!(refused(listing), KeyCheckError::Malformed(_)),
                "{listing:?}"
            );
        }
    }

    #[test]
    fn refuses_a_listing_over_the_bound_before_reading_it() {
        let padding = "uid:-::::1::::x::::::::::0:\n".repeat(MAX_LISTING_BYTES / 28 + 1);
        let listing = format!("{KEY_LISTING}{padding}");
        assert_eq!(refused(&listing), KeyCheckError::TooLarge);
    }

    #[test]
    fn the_messages_say_which_key_was_not_accepted() {
        assert!(
            refused("")
                .to_string()
                .contains("Docker signing key was not accepted")
        );
        assert!(
            refused(&"x".repeat(MAX_LISTING_BYTES + 1))
                .to_string()
                .contains("Docker signing key was not accepted")
        );
    }
}
