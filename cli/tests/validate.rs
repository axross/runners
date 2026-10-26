//! The validator's error collection across fields and entries: a problem in one
//! place must not hide another.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use marshal_cli::profile::Profile;
use marshal_cli::validate::{Failure, validate};

fn errors(profile: Profile, configuration: &str) -> Vec<String> {
    match validate(configuration.as_bytes(), profile, None).outcome {
        Err(Failure::Invalid(errors)) => errors,
        other => panic!("expected a rejected configuration, got {other:?}"),
    }
}

#[test]
fn a_missing_name_is_reported_beside_other_entries_errors() {
    let found = errors(
        Profile::Axpc,
        r#"{
            "imageName": "image:tag",
            "repositories": [
                { "owner": "o", "repository": "r", "slots": 1, "tokenPath": "C:\\t", "volumes": [] },
                { "owner": "o", "repository": "r", "name": "b", "slots": 0, "tokenPath": "C:\\t", "volumes": [] }
            ]
        }"#,
    );
    assert_eq!(
        found,
        [
            "repositories[0].name: required field is missing",
            "repositories[1].slots: must be an integer from 1 to 16",
        ]
    );
}

#[test]
fn a_non_list_repositories_is_reported_beside_the_budgets_own_field_errors() {
    let found = errors(
        Profile::Axmc,
        r#"{ "budget": { "cpus": 0, "memoryGb": "x" }, "repositories": 5 }"#,
    );
    assert_eq!(
        found,
        [
            "budget.cpus: must be a number greater than 0 and at most 64",
            "budget.memoryGb: must be an integer from 1 to 1024",
            "budget.diskGb: required field is missing",
            "repositories: must be a list",
        ]
    );
}

#[test]
fn an_unusable_entry_beside_usable_ones_still_lets_the_budget_totals_appear() {
    let found = errors(
        Profile::Axmc,
        r#"{
            "budget": { "cpus": 1, "memoryGb": 1, "diskGb": 1 },
            "repositories": [
                { "owner": "o", "repository": "r", "name": "a", "os": "linux", "slots": 2,
                  "tokenPath": "/t", "cpus": 2, "memoryGb": 8, "diskGb": 40 },
                { "owner": "o", "repository": "r", "name": "b", "os": "linux", "slots": 0,
                  "tokenPath": "/t", "diskGb": 40 }
            ]
        }"#,
    );
    assert_eq!(
        found,
        [
            "repositories[1].slots: must be an integer from 1 to 16",
            "budget: pools total 4 CPUs (the sum of slots times cpus), above the budget of 1",
            "budget: pools total 16 GB of memory (the sum of slots times memoryGb), above the budget of 1 GB",
            "budget: pools total 80 GB of disk (the sum of slots times diskGb), above the budget of 1 GB",
        ]
    );
}

#[test]
fn axpc_ignores_the_axmc_only_fields_after_reporting_them_unknown() {
    let found = errors(
        Profile::Axpc,
        r#"{
            "imageName": "image:tag",
            "repositories": [
                { "owner": "o", "repository": "r", "name": "a", "slots": 1, "tokenPath": "C:\\t",
                  "volumes": [], "os": "bogus", "diskGb": 0 }
            ]
        }"#,
    );
    assert_eq!(
        found,
        [
            "repositories[0].os: unknown field",
            "repositories[0].diskGb: unknown field",
        ]
    );
}

#[test]
fn axmc_reads_volumes_only_to_reject_it_even_as_null() {
    let found = errors(
        Profile::Axmc,
        r#"{
            "budget": { "cpus": 4, "memoryGb": 16, "diskGb": 100 },
            "repositories": [
                { "owner": "o", "repository": "r", "name": "a", "os": "linux", "slots": 1,
                  "tokenPath": "/t", "diskGb": 40, "volumes": null }
            ]
        }"#,
    );
    assert_eq!(
        found,
        ["repositories[0].volumes: not supported on axmc, because VM pools keep no volumes"]
    );
}

#[test]
fn collisions_from_several_pairs_are_reported_in_the_order_of_the_pairs() {
    let found = errors(
        Profile::Axpc,
        r#"{
            "imageName": "image:tag",
            "repositories": [
                { "owner": "Acme", "repository": "Site", "name": "web", "slots": 1, "volumes": [],
                  "tokenPath": "C:\\t\\Sit\u00e9.token", "labels": ["lin", "shared", "Gpu"] },
                { "owner": "acme", "repository": "site", "name": "web-api", "slots": 1, "volumes": [],
                  "tokenPath": "C:\\T\\SIT\u00c9.token", "labels": ["GPU", "x"] },
                { "owner": "acme", "repository": "other", "name": "docs", "slots": 1, "volumes": [],
                  "tokenPath": "C:\\T\\sit\u00e9.TOKEN", "labels": ["shared", "axpc"] },
                { "owner": "ACME", "repository": "SITE", "name": "web", "slots": 1, "volumes": [],
                  "tokenPath": "C:\\other.token", "labels": ["x", "gpu", "SHARED"] },
                { "owner": "Acme", "repository": "Other", "name": "docs-two", "slots": 1, "volumes": [],
                  "tokenPath": "C:\\t\\Sit\u00c9.token", "labels": ["Shared", "AXPC"] }
            ]
        }"#,
    );
    assert_eq!(
        found,
        [
            "repositories[1].name: name 'web-api' collides with 'web' at repositories[0]; set a distinct name on one entry",
            "repositories[1].labels: ambiguous same-repository routing; labels 'GPU' also appear at repositories[0]",
            "repositories[2].tokenPath: token file 'C:\\T\\sité.TOKEN' is already used at repositories[0]; each repository needs its own token",
            "repositories[2].tokenPath: token file 'C:\\T\\sité.TOKEN' is already used at repositories[1]; each repository needs its own token",
            "repositories[3].name: name 'web' collides with 'web' at repositories[0]; set a distinct name on one entry",
            "repositories[3].tokenPath: pools for the same repository must use the same token file as repositories[0]",
            "repositories[3].labels: ambiguous same-repository routing; labels 'gpu, SHARED' also appear at repositories[0]",
            "repositories[3].name: name 'web' collides with 'web-api' at repositories[1]; set a distinct name on one entry",
            "repositories[3].tokenPath: pools for the same repository must use the same token file as repositories[1]",
            "repositories[3].labels: ambiguous same-repository routing; labels 'x, gpu' also appear at repositories[1]",
            "repositories[4].tokenPath: token file 'C:\\t\\SitÉ.token' is already used at repositories[0]; each repository needs its own token",
            "repositories[4].tokenPath: token file 'C:\\t\\SitÉ.token' is already used at repositories[1]; each repository needs its own token",
            "repositories[4].name: name 'docs-two' collides with 'docs' at repositories[2]; set a distinct name on one entry",
            "repositories[4].labels: ambiguous same-repository routing; labels 'Shared, AXPC' also appear at repositories[2]",
        ]
    );
}
