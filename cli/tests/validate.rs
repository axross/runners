//! The validator's error collection across fields and entries: a problem in one
//! place must not hide another.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use runners_cli::profile::Profile;
use runners_cli::validate::{Failure, validate};

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
