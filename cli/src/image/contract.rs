//! The image contract: what a built image must declare for this CLI to run jobs in
//! it.
//!
//! The Dockerfile's final stage declares the contract version, the runner version
//! its base image pins, and that release's date. The CLI's own contract version is
//! read from the same embedded Dockerfile when the binary is compiled, so the two
//! cannot drift inside one binary. Changing the run contract, or what the image
//! must provide, bumps the version; the tests below go red when the run contract
//! changes without it, and when the runner pin and its release date disagree.

use super::definition::DOCKERFILE;

/// The image contract version an image declares.
pub const CONTRACT_LABEL: &str = "com.github.axross.runners.contract";

/// The runner version the image's base pins.
pub const RUNNER_VERSION_LABEL: &str = "com.github.axross.runners.runner-version";

/// That runner release's date, as `YYYY-MM-DD`, read as 00:00 UTC.
pub const RUNNER_RELEASED_LABEL: &str = "com.github.axross.runners.runner-released";

/// The label `build`, and the PowerShell rebuild script, give every image they
/// build; both prune dangling images by it.
pub const IMAGE_LABEL: &str = "com.github.axross.runners.image";

/// The value of [`IMAGE_LABEL`].
pub const IMAGE_LABEL_VALUE: &str = "actions-runner";

/// The build time `build` gives an image, in Unix seconds.
pub const BUILT_LABEL: &str = "com.github.axross.runners.built";

/// The image contract version of this CLI, as the embedded Dockerfile declares it.
#[allow(clippy::panic)] // evaluated at compile time, so a bad declaration fails the build
pub const CONTRACT_VERSION: u32 = match declared_contract(DOCKERFILE) {
    Some(version) => version,
    None => panic!("the Dockerfile does not declare a positive contract version"),
};

/// The text the contract version follows in the Dockerfile: the label, `=` and
/// the opening quote.
const CONTRACT_DECLARATION: &[u8] = b"com.github.axross.runners.contract=\"";

/// The version in the first `com.github.axross.runners.contract="<n>"` of `text`:
/// a positive decimal integer without a leading zero.
const fn declared_contract(mut text: &[u8]) -> Option<u32> {
    loop {
        if let Some(rest) = strip_prefix(text, CONTRACT_DECLARATION) {
            return positive_integer_before_quote(rest);
        }
        match text {
            [_, rest @ ..] => text = rest,
            [] => return None,
        }
    }
}

const fn strip_prefix<'a>(mut text: &'a [u8], mut prefix: &[u8]) -> Option<&'a [u8]> {
    loop {
        match (text, prefix) {
            (_, []) => return Some(text),
            ([have, text_rest @ ..], [want, prefix_rest @ ..]) if *have == *want => {
                text = text_rest;
                prefix = prefix_rest;
            }
            _ => return None,
        }
    }
}

const fn positive_integer_before_quote(mut text: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    loop {
        match text {
            [b'"', ..] if value > 0 => return Some(value),
            [digit @ b'0'..=b'9', rest @ ..] if value > 0 || *digit != b'0' => {
                let Some(shifted) = value.checked_mul(10) else {
                    return None;
                };
                let Some(next) = shifted.checked_add((*digit - b'0') as u32) else {
                    return None;
                };
                value = next;
                text = rest;
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::super::check::labels_format;
    use super::super::definition::EMBEDDED;
    use super::super::dockerfile::{
        base_reference, final_stage_labels, local_copy_sources, reference_tag,
    };
    use super::*;
    use crate::runtime::backend::{ContainerSpec, Mount};
    use crate::runtime::docker_args::{
        DIAGNOSTIC_LABEL, JIT_VARIABLE, RUN_COMMAND, guard_script, image_inspect_arguments,
        image_list_arguments, job_run_arguments, pool_diagnostic_list_arguments,
        pool_list_arguments, probe_run_arguments, volume_chown_arguments,
    };
    use crate::validate::PlannedVolume;

    /// Each runner release the image has pinned, with its date from the
    /// `actions/runner` release page. A runner bump adds its row.
    const RUNNER_RELEASES: &[(&str, &str)] = &[("2.337.0", "2026-08-26")];

    /// Each contract version with the fingerprint of the run contract it was
    /// approved for. A change to the run contract bumps the version in the
    /// Dockerfile and adds a row; the old rows stay as the record.
    const APPROVED_CONTRACTS: &[(u32, u64)] = &[(1, 0xd875_2700_4418_2c5d)];

    fn dockerfile() -> String {
        String::from_utf8(DOCKERFILE.to_vec()).unwrap()
    }

    fn label(name: &str) -> Option<String> {
        final_stage_labels(&dockerfile())
            .into_iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    #[test]
    fn the_contract_version_is_the_one_the_final_stage_declares() {
        assert_eq!(
            CONTRACT_DECLARATION,
            format!("{CONTRACT_LABEL}=\"").as_bytes()
        );
        assert_eq!(label(CONTRACT_LABEL), Some(CONTRACT_VERSION.to_string()));
        assert_eq!(
            dockerfile().matches(CONTRACT_LABEL).count(),
            1,
            "the compile-time read takes the first declaration, so there is one"
        );
    }

    #[test]
    fn a_contract_version_is_a_positive_integer_without_a_leading_zero() {
        let read = |text: &str| declared_contract(text.as_bytes());
        assert_eq!(
            read("LABEL com.github.axross.runners.contract=\"12\""),
            Some(12)
        );
        for refused in [
            "LABEL com.github.axross.runners.contract=\"0\"",
            "LABEL com.github.axross.runners.contract=\"01\"",
            "LABEL com.github.axross.runners.contract=\"\"",
            "LABEL com.github.axross.runners.contract=\"1a\"",
            "LABEL com.github.axross.runners.contract=\"99999999999\"",
            "LABEL com.github.axross.runners.contract=1",
            "LABEL something.else=\"1\"",
        ] {
            assert_eq!(read(refused), None, "{refused}");
        }
    }

    /// Why the declared runner labels disagree with the base image's tag or the
    /// release table.
    fn release_problems(
        base_tag: Option<&str>,
        version: Option<&str>,
        released: Option<&str>,
        table: &[(&str, &str)],
    ) -> Vec<String> {
        let mut problems = Vec::new();
        if version != base_tag {
            problems.push(format!(
                "the {RUNNER_VERSION_LABEL} label is {version:?}, but the base image's tag is {base_tag:?}"
            ));
        }
        match table.iter().find(|(known, _)| Some(*known) == version) {
            None => problems.push(format!(
                "runner {version:?} has no row in RUNNER_RELEASES; add its date from the actions/runner release page"
            )),
            Some((_, date)) if Some(*date) != released => problems.push(format!(
                "the {RUNNER_RELEASED_LABEL} label is {released:?}, but runner {version:?} was released on {date}"
            )),
            Some(_) => {}
        }
        problems
    }

    #[test]
    fn the_declared_runner_is_the_base_images_and_its_date_is_the_release_tables() {
        let base = base_reference(&dockerfile()).unwrap();
        let problems = release_problems(
            reference_tag(&base),
            label(RUNNER_VERSION_LABEL).as_deref(),
            label(RUNNER_RELEASED_LABEL).as_deref(),
            RUNNER_RELEASES,
        );
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_runner_bump_without_its_labels_or_its_release_row_is_a_problem() {
        let table = &[("2.337.0", "2026-08-26")];
        let date = Some("2026-08-26");
        assert!(release_problems(Some("2.337.0"), Some("2.337.0"), date, table).is_empty());
        // the base moved on, the labels did not
        assert_eq!(
            release_problems(Some("2.338.0"), Some("2.337.0"), date, table).len(),
            1
        );
        // the labels moved on, the table did not
        let unknown = release_problems(Some("2.338.0"), Some("2.338.0"), date, table);
        assert_eq!(unknown.len(), 1);
        assert!(unknown[0].contains("no row"), "{unknown:?}");
        // the date is not the release's
        let wrong = release_problems(Some("2.337.0"), Some("2.337.0"), Some("2026-08-27"), table);
        assert!(wrong[0].contains("was released on 2026-08-26"), "{wrong:?}");
        // the labels are missing
        assert_eq!(
            release_problems(Some("2.337.0"), None, None, table).len(),
            2
        );
    }

    /// Why the embedded files are not exactly the Dockerfile and its local copies.
    fn embedded_set_problems(dockerfile: &str, embedded: &[&str]) -> Vec<String> {
        let mut wanted: BTreeSet<String> = match local_copy_sources(dockerfile) {
            Ok(sources) => sources.into_iter().collect(),
            Err(problem) => return vec![problem],
        };
        wanted.insert("Dockerfile".to_string());
        let held: BTreeSet<String> = embedded.iter().map(|path| (*path).to_string()).collect();
        let mut problems: Vec<String> = wanted
            .difference(&held)
            .map(|path| format!("the Dockerfile copies {path}, which the binary does not embed"))
            .collect();
        problems.extend(
            held.difference(&wanted).map(|path| {
                format!("the binary embeds {path}, which the Dockerfile does not copy")
            }),
        );
        problems
    }

    #[test]
    fn the_embedded_set_is_the_dockerfile_and_every_file_it_copies() {
        let embedded: Vec<&str> = EMBEDDED.iter().map(|(path, _)| *path).collect();
        let problems = embedded_set_problems(&dockerfile(), &embedded);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_new_copy_or_a_stale_embedded_file_is_a_problem() {
        let text = "FROM a AS b\nCOPY one.sh /tmp/\nFROM b\nCOPY --from=b /x /x\n";
        assert!(embedded_set_problems(text, &["Dockerfile", "one.sh"]).is_empty());
        let added = format!("{text}COPY two.sh /tmp/\n");
        assert_eq!(
            embedded_set_problems(&added, &["Dockerfile", "one.sh"]),
            ["the Dockerfile copies two.sh, which the binary does not embed"]
        );
        assert_eq!(
            embedded_set_problems(text, &["Dockerfile", "one.sh", "old.cpp"]),
            ["the binary embeds old.cpp, which the Dockerfile does not copy"]
        );
    }

    /// The PowerShell diagnostic variant's launcher, as `docker-commands.ps1` builds
    /// it: the guard, then the collector, in raw mode when asked, or the runner
    /// itself with a gap when the collector is missing.
    fn diagnostic_launcher(raw: bool) -> String {
        let mode = if raw { " raw-run" } else { "" };
        format!(
            "{}\nif [ -x /usr/local/bin/runner-diagnostics ]; then exec /usr/local/bin/runner-diagnostics{mode}; \
             else printf \"Diagnostic gap: collector missing.\\n\" >&2; exec /home/runner/run.sh; fi",
            guard_script()
        )
    }

    /// The PowerShell diagnostic variant's arguments: no `--rm`, the diagnostics
    /// label, and the diagnostic launcher. 3b's runtime gives these under contract 1.
    fn diagnostic_run_arguments(spec: &ContainerSpec, raw: bool) -> Vec<String> {
        let memory = format!("{}g", spec.memory_gb);
        let mut arguments: Vec<String> = [
            "run",
            "--pull",
            "never",
            "--name",
            &spec.name,
            "--label",
            DIAGNOSTIC_LABEL,
            "--cpus",
            &spec.cpus,
            "--cpuset-cpus",
            &spec.cpuset,
            "--memory",
            &memory,
            "--memory-swap",
            &memory,
        ]
        .iter()
        .map(|argument| (*argument).to_string())
        .collect();
        for mount in &spec.mounts {
            arguments.push("--mount".to_string());
            arguments.push(format!(
                "type=volume,source={},target={}",
                mount.volume, mount.target
            ));
        }
        arguments.extend(
            [
                "-e",
                JIT_VARIABLE,
                &spec.image,
                "bash",
                "-c",
                &diagnostic_launcher(raw),
                "--",
                &spec.cpuset,
                &spec.run_command,
            ]
            .iter()
            .map(|argument| (*argument).to_string()),
        );
        arguments
    }

    #[test]
    fn the_diagnostic_variant_is_the_powershell_one() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hosts/windows-docker-desktop/docker-commands.ps1");
        let source = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        for fragment in [
            "if ($DiagnosticRawRecords) { $mode = ' raw-run' }",
            "$launcher = $guard + \"`n\" + 'if [ -x /usr/local/bin/runner-diagnostics ]; then exec /usr/local/bin/runner-diagnostics' + $mode +",
            "'; else printf \"Diagnostic gap: collector missing.\\n\" >&2; exec /home/runner/run.sh; fi'",
            "return @('run', '--pull', 'never', '--name', $Name,",
            "'--label', 'runners.diagnostic-lifecycle=1',",
            "'--cpus', $Cpus, '--cpuset-cpus', $CpusetCpus, '--memory', \"${MemoryGb}g\", '--memory-swap', \"${MemoryGb}g\") + @($Mounts) +",
            "@('-e', $JitConfigVariable, $ImageName, 'bash', '-c', $launcher, '--', $CpusetCpus, $RunCommand)",
        ] {
            assert!(
                source.contains(fragment),
                "docker-commands.ps1 lost: {fragment}"
            );
        }
    }

    fn sample_spec() -> ContainerSpec {
        ContainerSpec {
            name: "pool-1-20240305060708009".to_string(),
            cpus: "1.5".to_string(),
            cpuset: "5,11".to_string(),
            memory_gb: 16,
            mounts: vec![Mount {
                volume: "pool-npm".to_string(),
                target: "/home/runner/.npm".to_string(),
            }],
            image: "actions-runner:local".to_string(),
            run_command: RUN_COMMAND.to_string(),
        }
    }

    /// Every argument list of the run contract for fixed inputs, and the labels the
    /// CLI reads, one list per line with its arguments NUL separated.
    fn run_contract_text() -> String {
        let spec = sample_spec();
        let volumes = [PlannedVolume {
            name: "pool-npm".to_string(),
            mount_path: "/home/runner/.npm".to_string(),
        }];
        let lists = [
            job_run_arguments(&spec),
            diagnostic_run_arguments(&spec, false),
            diagnostic_run_arguments(&spec, true),
            probe_run_arguments("pool-cpu-probe-1", &spec.image),
            volume_chown_arguments(&spec.image, &volumes),
            pool_list_arguments("pool"),
            pool_diagnostic_list_arguments("pool"),
            image_inspect_arguments(&spec.image),
            image_list_arguments(&spec.image),
            vec![
                CONTRACT_LABEL.to_string(),
                RUNNER_VERSION_LABEL.to_string(),
                RUNNER_RELEASED_LABEL.to_string(),
                BUILT_LABEL.to_string(),
                labels_format(),
            ],
        ];
        lists
            .iter()
            .map(|list| list.join("\0"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// FNV-1a, 64 bits: enough to notice a change, not a security check.
    fn fingerprint(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Why the run contract does not match the version it is declared under.
    fn contract_problem(version: u32, fingerprint: u64, table: &[(u32, u64)]) -> Option<String> {
        match table.iter().find(|(approved, _)| *approved == version) {
            None => Some(format!(
                "contract {version} has no row in APPROVED_CONTRACTS; add ({version}, {fingerprint:#018x})"
            )),
            Some((_, approved)) if *approved != fingerprint => Some(format!(
                "the run contract changed under contract {version}: bump {CONTRACT_LABEL} in the \
                 Dockerfile and add (<new version>, {fingerprint:#018x}) to APPROVED_CONTRACTS"
            )),
            Some(_) => None,
        }
    }

    #[test]
    fn the_run_contract_is_the_one_its_version_was_approved_for() {
        let problem = contract_problem(
            CONTRACT_VERSION,
            fingerprint(&run_contract_text()),
            APPROVED_CONTRACTS,
        );
        assert_eq!(problem, None);
    }

    #[test]
    fn a_changed_run_contract_or_an_unapproved_version_is_a_problem() {
        let text = run_contract_text();
        let table = &[(1, fingerprint(&text))];
        assert_eq!(contract_problem(1, fingerprint(&text), table), None);
        let changed = text.replace("--pull", "--pull=always");
        let problem = contract_problem(1, fingerprint(&changed), table).unwrap();
        assert!(problem.contains("bump"), "{problem}");
        let problem = contract_problem(2, fingerprint(&changed), table).unwrap();
        assert!(problem.contains("no row"), "{problem}");
    }

    #[test]
    fn the_build_test_definition_starts_from_the_same_base_and_declares_the_same_labels() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/build-definition/Dockerfile");
        let test_definition = std::fs::read_to_string(path).unwrap();
        assert_eq!(
            base_reference(&test_definition),
            base_reference(&dockerfile()),
            "a base bump updates the test definition's FROM too"
        );
        let declared = |text: &str| -> Vec<(String, String)> {
            final_stage_labels(text)
                .into_iter()
                .filter(|(key, _)| key.starts_with("com.github.axross.runners."))
                .collect()
        };
        assert_eq!(declared(&test_definition), declared(&dockerfile()));
        assert_eq!(declared(&dockerfile()).len(), 3);
    }
}
