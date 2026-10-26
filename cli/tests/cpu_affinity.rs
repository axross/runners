//! CPU placement: `validate`'s summary and the backend agree on the offsets, and the
//! selection reproduces the PowerShell golden case of `test-cpu-affinity.ps1`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use marshal_cli::affinity::select_cpus;
use marshal_cli::profile::Profile;
use marshal_cli::runtime::pool::pools_from_plan;
use marshal_cli::summary::plan_summary;
use marshal_cli::validate::validate;

/// The configuration the PowerShell test builds from `split-pools.json`: two slots
/// of 1.5 CPUs, then one slot of a hair over 1 CPU, whose affinity count is 2.
const CONFIG: &str = r#"{
  "imageName": "actions-runner:synthetic",
  "repositories": [
    { "owner": "o", "repository": "one", "name": "example-general", "slots": 2, "cpus": 1.5,
      "tokenPath": "C:\\one.token", "volumes": [] },
    { "owner": "o", "repository": "two", "name": "example-android", "slots": 1, "cpus": 1.0000000001,
      "tokenPath": "C:\\two.token", "volumes": [] }
  ]
}"#;

fn summary_numbers(lines: &[String], prefix: &str) -> Vec<u64> {
    lines
        .iter()
        .filter_map(|line| line.trim().strip_prefix(prefix))
        .map(|value| value.split_whitespace().next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn validate_and_the_backend_compute_the_same_counts_and_offsets() {
    let plan = validate(CONFIG.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    let summary = plan_summary(&plan);
    let pools = pools_from_plan(&plan).unwrap();

    let counts = summary_numbers(&summary, "affinity per slot:");
    let positions = summary_numbers(&summary, "affinity position:");
    assert_eq!(counts, [2, 2]);
    assert_eq!(positions, [0, 4]);
    assert_eq!(
        pools.iter().map(|pool| pool.cpu_count).collect::<Vec<_>>(),
        counts
    );
    assert_eq!(
        pools.iter().map(|pool| pool.cpu_offset).collect::<Vec<_>>(),
        positions
    );
    // a slot starts after the CPUs of the same pool's earlier slots.
    let slots = pools[0].slot_specs();
    assert_eq!(
        slots.iter().map(|s| s.cpu_offset).collect::<Vec<_>>(),
        [0, 2]
    );
}

#[test]
fn the_selection_reproduces_the_powershell_golden_case() {
    let plan = validate(CONFIG.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    let inventory = "2,5-6,11,14-15";
    let mut sets = Vec::new();
    for pool in pools_from_plan(&plan).unwrap() {
        for slot in pool.slot_specs() {
            sets.push(select_cpus(inventory, slot.cpu_count, slot.cpu_offset).unwrap());
        }
    }
    for offset in [6, 5] {
        sets.push(select_cpus(inventory, 2, offset).unwrap());
    }
    assert_eq!(sets.join("|"), "2,5|6,11|14-15|2,5|2,15");
}
