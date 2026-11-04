//! the runner releases this CLI has declared: each one's release date and the
//! SHA-256 of its osx-arm64 archive, the version the Linux image and the macOS
//! golden VM both install, and the debug-only override of that declaration; see
//! `docs/operations/cli.md`.

/// the variable that declares another runner in a debug build.
pub const TEST_RUNNER_VARIABLE: &str = "MARSHAL_TEST_RUNNER";

/// the runner version the CLI declares: the Linux image's base pins it, and the
/// macOS golden VM installs it.
pub const DECLARED_RUNNER_VERSION: &str = "2.337.0";

/// one runner release the CLI has declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerRelease {
    pub version: &'static str,
    /// the release's date as `YYYY-MM-DD`, read as 00:00 UTC.
    pub released: &'static str,
    /// the lowercase hexadecimal SHA-256 of `actions-runner-osx-arm64-<version>.tar.gz`,
    /// as the release notes list it.
    pub osx_arm64_sha256: &'static str,
}

/// every runner version the CLI has declared; a runner bump adds its entry, with
/// both values taken from the `actions/runner` release page.
pub const RUNNER_RELEASES: &[RunnerRelease] = &[RunnerRelease {
    version: "2.337.0",
    released: "2026-08-26",
    osx_arm64_sha256: "5a2cd92908a93d7276a194e1de6008099f3e7946f3f8e14aa7a1a7b4a31fdec2",
}];

/// the record's entry for `version`.
#[must_use]
pub fn release(version: &str) -> Option<&'static RunnerRelease> {
    RUNNER_RELEASES
        .iter()
        .find(|release| release.version == version)
}

/// the runner a golden macOS VM installs: its version and the SHA-256 its archive
/// must have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredRunner {
    pub version: String,
    pub osx_arm64_sha256: String,
}

/// resolves the declared runner: the record's entry for [`DECLARED_RUNNER_VERSION`].
/// A release build ignores `requested`, so a release binary cannot be made to
/// install a runner the record does not hold, and a debug build takes it as
/// `<version>=<sha256>` for the tests, refusing anything else.
pub fn declared_runner(requested: Option<&str>) -> Result<DeclaredRunner, String> {
    #[cfg(debug_assertions)]
    if let Some(requested) = requested {
        return parse_override(requested);
    }
    #[cfg(not(debug_assertions))]
    let _ = requested;
    release(DECLARED_RUNNER_VERSION)
        .map(|release| DeclaredRunner {
            version: release.version.to_string(),
            osx_arm64_sha256: release.osx_arm64_sha256.to_string(),
        })
        .ok_or_else(|| {
            format!(
                "the runner record has no entry for the declared runner {DECLARED_RUNNER_VERSION}"
            )
        })
}

#[cfg(any(debug_assertions, test))]
fn parse_override(requested: &str) -> Result<DeclaredRunner, String> {
    let refuse = |problem: &str| {
        Err(format!(
            "{TEST_RUNNER_VARIABLE} must be <version>=<sha256>, such as 2.337.0=<64 lowercase \
             hexadecimal digits>, but {problem}"
        ))
    };
    let Some((version, digest)) = requested.split_once('=') else {
        return refuse("it has no '='");
    };
    if !is_version(version) {
        return refuse("its version is not three dot-separated numbers without leading zeros");
    }
    if !is_sha256(digest) {
        return refuse("its SHA-256 is not 64 lowercase hexadecimal digits");
    }
    Ok(DeclaredRunner {
        version: version.to_string(),
        osx_arm64_sha256: digest.to_string(),
    })
}

/// whether `text` is a runner version: three dot-separated decimal numbers of at
/// most nine digits, none with a leading zero.
#[cfg(any(debug_assertions, test))]
fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            (1..=9).contains(&part.len())
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

/// whether `text` is a SHA-256 as 64 lowercase hexadecimal digits.
#[cfg(any(debug_assertions, test))]
fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::super::check::date_seconds;
    use super::super::contract::RUNNER_VERSION_LABEL;
    use super::super::definition::declared_label;
    use super::*;

    const DIGEST: &str = "5a2cd92908a93d7276a194e1de6008099f3e7946f3f8e14aa7a1a7b4a31fdec2";

    /// what the record lacks for `declared`, or holds malformed, as the message a
    /// runner bump needs.
    fn record_problems(declared: &str, table: &[RunnerRelease]) -> Vec<String> {
        let mut problems = Vec::new();
        if !table.iter().any(|release| release.version == declared) {
            problems.push(format!(
                "the declared runner {declared} has no entry in RUNNER_RELEASES; add its release \
                 date and the SHA-256 of actions-runner-osx-arm64-{declared}.tar.gz from the \
                 actions/runner release page"
            ));
        }
        for (index, release) in table.iter().enumerate() {
            let version = release.version;
            if table[..index]
                .iter()
                .any(|earlier| earlier.version == version)
            {
                problems.push(format!("runner {version} is listed more than once"));
            }
            if release.released.is_empty() {
                problems.push(format!("runner {version} has no release date"));
            } else if date_seconds(release.released).is_none() {
                problems.push(format!(
                    "runner {version}'s release date {:?} is not a YYYY-MM-DD date",
                    release.released
                ));
            }
            if release.osx_arm64_sha256.is_empty() {
                problems.push(format!("runner {version} has no osx-arm64 SHA-256"));
            } else if !is_sha256(release.osx_arm64_sha256) {
                problems.push(format!(
                    "runner {version}'s osx-arm64 SHA-256 is not 64 lowercase hexadecimal digits"
                ));
            }
        }
        problems
    }

    fn entry(released: &'static str, osx_arm64_sha256: &'static str) -> RunnerRelease {
        RunnerRelease {
            version: "2.338.0",
            released,
            osx_arm64_sha256,
        }
    }

    #[test]
    fn the_declared_version_is_the_one_the_linux_images_base_pins() {
        assert_eq!(
            declared_label(RUNNER_VERSION_LABEL),
            Some(DECLARED_RUNNER_VERSION)
        );
    }

    #[test]
    fn the_declared_runner_has_a_release_date_and_an_osx_arm64_digest() {
        let problems = record_problems(DECLARED_RUNNER_VERSION, RUNNER_RELEASES);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_missing_entry_date_or_digest_is_a_problem_that_names_it() {
        let table = [entry("2026-09-01", DIGEST)];
        assert!(record_problems("2.338.0", &table).is_empty());

        let missing = record_problems("2.339.0", &table);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(
            missing[0].contains("2.339.0")
                && missing[0].contains("release date")
                && missing[0].contains("actions-runner-osx-arm64-2.339.0.tar.gz"),
            "{missing:?}"
        );

        let no_date = record_problems("2.338.0", &[entry("", DIGEST)]);
        assert_eq!(no_date, ["runner 2.338.0 has no release date"]);
        let no_digest = record_problems("2.338.0", &[entry("2026-09-01", "")]);
        assert_eq!(no_digest, ["runner 2.338.0 has no osx-arm64 SHA-256"]);
    }

    #[test]
    fn a_malformed_date_or_digest_or_a_repeated_version_is_a_problem() {
        for date in ["2026-02-30", "26-09-01", "2026-9-1"] {
            assert_eq!(
                record_problems("2.338.0", &[entry(date, DIGEST)]).len(),
                1,
                "{date}"
            );
        }
        let digests = [
            &DIGEST[1..],
            String::leak(DIGEST.to_uppercase()),
            "zz",
            String::leak(format!("{DIGEST}0")),
        ];
        for digest in digests {
            let problems = record_problems("2.338.0", &[entry("2026-09-01", digest)]);
            assert_eq!(problems.len(), 1, "{digest}: {problems:?}");
            assert!(problems[0].contains("64 lowercase"), "{problems:?}");
        }
        let twice = [entry("2026-09-01", DIGEST), entry("2026-09-01", DIGEST)];
        assert_eq!(
            record_problems("2.338.0", &twice),
            ["runner 2.338.0 is listed more than once"]
        );
    }

    #[test]
    fn the_override_is_a_version_and_a_sha256_joined_by_an_equals_sign() {
        let declared = parse_override(&format!("2.300.1={DIGEST}")).unwrap();
        assert_eq!(declared.version, "2.300.1");
        assert_eq!(declared.osx_arm64_sha256, DIGEST);
        assert_eq!(
            parse_override(&format!("0.0.0={DIGEST}")).unwrap().version,
            "0.0.0"
        );
    }

    #[test]
    fn anything_else_is_refused_with_the_format() {
        let upper = DIGEST.to_uppercase();
        for requested in [
            String::new(),
            DIGEST.to_string(),
            "2.337.0".to_string(),
            "2.337.0=".to_string(),
            format!("={DIGEST}"),
            format!("2.337={DIGEST}"),
            format!("2.337.0.1={DIGEST}"),
            format!("2.337.x={DIGEST}"),
            format!("02.337.0={DIGEST}"),
            format!("2.337.0000={DIGEST}"),
            format!("v2.337.0={DIGEST}"),
            format!("2.1234567890.0={DIGEST}"),
            format!("2.337.0={upper}"),
            format!("2.337.0={}", &DIGEST[1..]),
            format!("2.337.0={DIGEST}0"),
            format!("2.337.0={DIGEST}=x"),
            format!(" 2.337.0={DIGEST}"),
            format!("2.337.0={DIGEST} "),
            format!("2.337.0={DIGEST}\n"),
            format!("2.337.0 ;touch x={DIGEST}"),
        ] {
            let error = parse_override(&requested).unwrap_err();
            assert!(
                error.starts_with("MARSHAL_TEST_RUNNER must be <version>=<sha256>"),
                "{requested:?}: {error}"
            );
        }
    }

    #[test]
    fn without_a_request_the_declared_runner_is_the_records() {
        let declared = declared_runner(None).unwrap();
        assert_eq!(declared.version, DECLARED_RUNNER_VERSION);
        assert_eq!(
            Some(declared.osx_arm64_sha256.as_str()),
            release(DECLARED_RUNNER_VERSION).map(|release| release.osx_arm64_sha256)
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_debug_build_takes_the_request_and_refuses_a_malformed_one() {
        let other = format!("2.300.1={DIGEST}");
        assert_eq!(declared_runner(Some(&other)).unwrap().version, "2.300.1");
        assert!(declared_runner(Some("2.300.1")).is_err());
    }
}
