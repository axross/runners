#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use marshal_cli::profile::{Os, Profile};
use marshal_cli::validate::{Failure, Plan, PoolTarget, validate, validate_toml};

fn configuration(pool: &str) -> String {
    format!(
        "schema_version = 1\nrouting_label = 'route'\nimage_name = 'image:tag'\n\
         [[pool]]\nname = 'a'\nrunners = 1\n{pool}\n"
    )
}

fn accepted(text: &str, profile: Profile) -> Plan {
    validate_toml(text.as_bytes(), profile, None)
        .outcome
        .unwrap()
}

fn problems(text: &str, profile: Profile) -> Vec<String> {
    match validate_toml(text.as_bytes(), profile, None).outcome {
        Err(Failure::Invalid(problems)) => problems,
        other => panic!("expected configuration problems, got {other:?}"),
    }
}

/// target selection must preserve configured spelling and group names while
/// retaining the profile's defaults for unrelated pool fields.
#[test]
fn toml_keeps_the_repository_or_organization_target_without_mirrored_fields() {
    let repository = accepted(
        &configuration("repository = 'Example/Project'"),
        Profile::Axpc,
    );
    assert_eq!(
        repository.entries[0].target,
        PoolTarget::Repository {
            owner: "Example".to_string(),
            repository: "Project".to_string(),
        }
    );
    let organization = accepted(
        &configuration("organization = 'Example'\nrunner_group = 'Private build workers'"),
        Profile::Axpc,
    );
    let entry = &organization.entries[0];
    assert_eq!(
        entry.target,
        PoolTarget::Organization {
            organization: "Example".to_string(),
            runner_group: "Private build workers".to_string(),
        }
    );
    assert_eq!(entry.target.owner(), "Example");
    assert_eq!(
        entry.target.to_string(),
        "Example (runner group Private build workers)"
    );
    assert_eq!(repository.entries[0].target.to_string(), "Example/Project");
    assert_eq!(entry.os, Some(Os::Linux));
    assert_eq!(entry.labels, ["self-hosted", "linux", "x64", "route"]);
    assert!(entry.volumes.is_empty());
    assert!(entry.token_path.is_none());
}

/// cross-key errors belong to the offending key, or the table header when no
/// single key owns the problem; missing target and stray group can coexist.
#[test]
fn target_and_group_cross_key_problems_keep_the_key_and_line() {
    for (pool, expected) in [
        (
            "repository = 'Example/Project'\norganization = 'Other'\nrunner_group = 'workers'",
            "line 4: pool[0]: names both repository and organization",
        ),
        (
            "",
            "line 4: pool[0]: names neither repository nor organization",
        ),
        (
            "organization = 'Example'",
            "line 4: pool[0].runner_group: required with organization",
        ),
        (
            "repository = 'Example/Project'\nrunner_group = 'workers'",
            "line 8: pool[0].runner_group: only with organization",
        ),
        (
            "runner_group = 'workers'",
            "line 7: pool[0].runner_group: only with organization",
        ),
    ] {
        let found = problems(&configuration(pool), Profile::Axpc);
        assert!(
            found.iter().any(|problem| problem == expected),
            "{pool}: {found:?}"
        );
        if !pool.starts_with("runner_group") {
            assert_eq!(found.len(), 1, "{found:?}");
        } else {
            assert_eq!(
                found[0],
                "line 4: pool[0]: names neither repository nor organization"
            );
            assert_eq!(found.len(), 2);
        }
    }
}

/// input types are rejected rather than coerced; long valid names remain
/// accepted without inventing a local limit absent from the owner rule.
#[test]
fn invalid_target_and_group_types_are_not_quoted_or_coerced() {
    for value in ["42", "false", "[]", "{}", "2026-01-01", "'bad/name'", "''"] {
        let found = problems(
            &configuration(&format!("organization = {value}\nrunner_group = 'workers'")),
            Profile::Axpc,
        );
        assert_eq!(
            found,
            [
                "line 7: pool[0].organization: must be a GitHub owner name (letters, digits and hyphens)"
            ],
            "{value}"
        );
    }
    for value in [
        "42",
        "false",
        "[]",
        "{}",
        "2026-01-01",
        "''",
        "'   '",
        "\"bad\\nname\"",
    ] {
        let found = problems(
            &configuration(&format!("organization = 'Example'\nrunner_group = {value}")),
            Profile::Axpc,
        );
        assert_eq!(
            found,
            [
                "line 8: pool[0].runner_group: must be a non-empty runner group name without control characters"
            ],
            "{value}"
        );
    }
    let name = "Build workers 日本語 ".repeat(20);
    let organization = "a".repeat(100);
    let plan = accepted(
        &configuration(&format!(
            "organization = '{organization}'\nrunner_group = '{name}'"
        )),
        Profile::Axpc,
    );
    assert_eq!(plan.entries[0].target.owner(), organization);
    assert!(
        matches!(&plan.entries[0].target, PoolTarget::Organization { runner_group, .. } if runner_group == &name)
    );
}

/// an empty volume list is still a forbidden organisation key; a repository pool's
/// volumes are unaffected.
#[test]
fn organization_volumes_are_rejected_when_present_even_if_empty_or_wrong_typed() {
    for value in [
        "[]",
        "false",
        "[{ suffix = 'cache', mount_path = '/cache' }]",
    ] {
        for profile in [Profile::Axpc, Profile::Axmc] {
            let pool =
                format!("organization = 'Example'\nrunner_group = 'workers'\nvolumes = {value}");
            let mut text = configuration(&pool);
            if profile == Profile::Axmc {
                text.push_str("disk_gb = 40\n[budget]\ncpus = 8\nmemory_gb = 32\ndisk_gb = 200\n");
            }
            assert_eq!(
                problems(&text, profile),
                ["line 9: pool[0].volumes: not allowed with organization"]
            );
            let absent = text.replace(&format!("volumes = {value}\n"), "");
            assert!(accepted(&absent, profile).entries[0].volumes.is_empty());
        }
    }
    let repository = accepted(
        &configuration(
            "repository = 'Example/Project'\nvolumes = [{ suffix = 'cache', mount_path = '/cache' }]",
        ),
        Profile::Axpc,
    );
    assert_eq!(repository.entries[0].volumes[0].name, "a-cache");
}

/// `scale_set` is reported as `not allowed with organization` on an organisation
/// pool whatever its value, and stays an unknown key on a repository pool.
#[test]
fn scale_set_is_refused_for_organizations_without_admitting_it_to_repository_pools() {
    for value in ["'builds'", "false", "[]"] {
        assert_eq!(
            problems(
                &configuration(&format!(
                    "organization = 'Example'\nrunner_group = 'workers'\nscale_set = {value}"
                )),
                Profile::Axpc
            ),
            ["line 9: pool[0].scale_set: not allowed with organization"]
        );
        assert_eq!(
            problems(
                &configuration(&format!(
                    "repository = 'Example/Project'\nscale_set = {value}"
                )),
                Profile::Axpc
            ),
            ["line 8: pool[0].scale_set: unknown key"]
        );
    }
}

/// TOML's extension must not admit new JSON keys or replace JSON's missing
/// owner/repository diagnostics with TOML's cross-key problems.
#[test]
fn json_does_not_gain_an_organization_target_or_group_rules() {
    let repository = r#"{"imageName":"image:tag","repositories":[{"owner":"Example","repository":"Project","name":"a","slots":1,"tokenPath":"C:\\t","volumes":[]"#;
    let accepted_json = format!("{repository}}}]}}");
    assert!(
        validate(accepted_json.as_bytes(), Profile::Axpc, None)
            .outcome
            .is_ok()
    );
    for keys in [
        r#", "organization":"Other", "runner_group":"workers""#,
        r#", "organization":"Other", "runnerGroup":42"#,
    ] {
        let text = format!("{repository}{keys}}}]}}");
        let Err(Failure::Invalid(found)) = validate(text.as_bytes(), Profile::Axpc, None).outcome
        else {
            panic!("expected JSON unknown-field problems");
        };
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0], "repositories[0].organization: unknown field");
        assert!(found[1].ends_with(": unknown field"));
    }
    let organization_only = r#"{"imageName":"image:tag","repositories":[{"organization":"Example","runner_group":"workers","name":"a","slots":1,"tokenPath":"C:\\t","volumes":[]}]}"#;
    let Err(Failure::Invalid(found)) =
        validate(organization_only.as_bytes(), Profile::Axpc, None).outcome
    else {
        panic!("expected legacy JSON missing-field problems");
    };
    assert_eq!(
        found,
        [
            "repositories[0].organization: unknown field",
            "repositories[0].runner_group: unknown field",
            "repositories[0].owner: required field is missing",
            "repositories[0].repository: required field is missing",
        ]
    );
}

fn pair(first: &str, second: &str) -> String {
    format!(
        "{}[[pool]]\nname = 'b'\nrunners = 1\n{second}\n",
        configuration(first)
    )
}

/// groups do not establish disjoint routing domains: pools may serve the same
/// repository, even with different groups, owner casing or input order.
#[test]
fn organization_overlap_is_case_insensitive_in_both_pool_orders_and_across_groups() {
    for (first, second) in [
        (
            "organization = 'EXample'\nrunner_group = 'one'",
            "repository = 'example/Project'",
        ),
        (
            "repository = 'EXample/Other'",
            "organization = 'example'\nrunner_group = 'two'",
        ),
        (
            "organization = 'EXample'\nrunner_group = 'one'",
            "organization = 'example'\nrunner_group = 'two'",
        ),
    ] {
        for (a, b, label, routing_label_shared) in [
            ("", "", "route", true),
            ("\nlabels = []", "", "route", true),
            (
                "\nlabels = ['Build']",
                "\nlabels = ['build']",
                "build",
                false,
            ),
            ("\nlabels = ['route']", "", "route", true),
            ("", "\nlabels = ['ROUTE']", "ROUTE", true),
            (
                "\nlabels = ['shared', 'only-first']",
                "\nlabels = ['only-second', 'SHARED']",
                "SHARED",
                false,
            ),
            (
                "\nlabels = ['route', 'shared']",
                "\nlabels = ['SHARED', 'Route']",
                "SHARED, Route",
                true,
            ),
        ] {
            let advice = if routing_label_shared {
                "set distinct explicit labels on each pool instead of the shared routing_label"
            } else {
                "give each pool its own labels"
            };
            let text = pair(&format!("{first}{a}"), &format!("{second}{b}"));
            let line = text
                .lines()
                .enumerate()
                .filter(|(_, line)| *line == "[[pool]]")
                .nth(1)
                .unwrap()
                .0
                + 1;
            assert_eq!(
                problems(&text, Profile::Axpc),
                [format!(
                    "line {line}: pool[1].labels: ambiguous routing for pools that may serve the same repository; labels '{label}' also appear at pool[0]; {advice}"
                )]
            );
        }
        let text = pair(
            &format!("{first}\nlabels = ['first']"),
            &format!("{second}\nlabels = ['second']"),
        );
        assert_eq!(accepted(&text, Profile::Axpc).entries.len(), 2);
    }
}

/// owner equality alone is not overlap for repository-only pools; otherwise a
/// TOML file valid before organisation support would change validity.
#[test]
fn labels_only_overlap_for_targets_that_can_serve_the_same_repository() {
    for (first, second) in [
        ("repository = 'Example/One'", "repository = 'example/Two'"),
        (
            "organization = 'Example'\nrunner_group = 'workers'",
            "organization = 'Other'\nrunner_group = 'workers'",
        ),
        (
            "organization = 'Example'\nrunner_group = 'workers'",
            "repository = 'Other/Project'",
        ),
    ] {
        assert_eq!(
            accepted(&pair(first, second), Profile::Axpc).entries.len(),
            2
        );
    }
    let same_repository = pair(
        "repository = 'Example/Project'",
        "repository = 'example/project'",
    );
    assert_eq!(
        problems(&same_repository, Profile::Axpc),
        [
            "line 8: pool[1].labels: ambiguous same-repository routing; labels 'route' also appear at pool[0]"
        ]
    );
}

/// platform labels separate routing across operating systems, so pools whose
/// operating systems differ may share a custom label.
#[test]
fn matching_organization_owners_can_share_labels_when_their_os_differs() {
    let text = pair(
        "organization = 'Example'\nrunner_group = 'workers'\nos = 'linux'\ndisk_gb = 40",
        "repository = 'example/Project'\nos = 'macos'",
    ) + "[budget]\ncpus = 8\nmemory_gb = 32\ndisk_gb = 250\n";
    let plan = accepted(&text, Profile::Axmc);
    assert_eq!(plan.entries[0].os, Some(Os::Linux));
    assert_eq!(plan.entries[1].os, Some(Os::Macos));
    let same_os = text.replace("os = 'macos'", "os = 'linux'\ndisk_gb = 40");
    assert!(
        problems(&same_os, Profile::Axmc)
            .iter()
            .any(|problem| problem.contains("pool[1].labels: ambiguous routing"))
    );
}
