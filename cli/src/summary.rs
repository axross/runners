//! The plan summary `validate` prints.

use crate::affinity::{affinity_count, affinity_offset};
use crate::profile::Profile;
use crate::validate::{Plan, format_cpu_count, format_nanos, usage};

/// Returns the lines of the summary. On axpc they are the PowerShell supervisor's
/// `-ValidateOnly` lines, one for one.
#[must_use]
pub fn plan_summary(plan: &Plan) -> Vec<String> {
    match plan.profile {
        Profile::Axpc => axpc_summary(plan),
        Profile::Axmc => axmc_summary(plan),
    }
}

fn flag(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

fn axpc_summary(plan: &Plan) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Host configuration is valid: {} entries, image {}",
            plan.entries.len(),
            plan.image_name.as_deref().unwrap_or_default()
        ),
        "CPU policy: cpus is CPU-time quota; affinity count is ceiling(cpus), not reserved CPUs."
            .to_string(),
    ];
    for (index, entry) in plan.entries.iter().enumerate() {
        lines.push(String::new());
        lines.push(format!("Repository {}/{}", entry.owner, entry.repository));
        lines.push(format!("  slots:             {}", entry.slots));
        lines.push(format!("  labels:            {}", entry.labels.join(", ")));
        lines.push(format!("  token file:        {}", entry.token_path));
        lines.push(format!("  name:              {}", entry.name));
        lines.push(format!(
            "  containers:        {}-<index>-<timestamp>",
            entry.name
        ));
        lines.push(format!(
            "  cpus:              {}",
            format_cpu_count(entry.cpus)
        ));
        lines.push(format!(
            "  affinity per slot: {} CPUs; IDs discovered at launch",
            affinity_count(entry)
        ));
        lines.push(format!(
            "  affinity position: {}",
            affinity_offset(&plan.entries, index)
        ));
        lines.push(format!("  memory:            {} GB", entry.memory_gb));
        lines.push(format!("  diagnostics:       {}", flag(entry.diagnostics)));
        lines.push(format!(
            "  raw records:       {}",
            flag(entry.diagnostic_raw_records)
        ));
        for volume in &entry.volumes {
            lines.push(format!(
                "  volume:            {} -> {}",
                volume.name, volume.mount_path
            ));
        }
    }
    lines
}

fn axmc_summary(plan: &Plan) -> Vec<String> {
    let mut header = format!(
        "Host configuration is valid: {} entries, profile axmc",
        plan.entries.len()
    );
    if let Some(image) = &plan.image_name {
        header.push_str(&format!(", image {image}"));
    }
    let mut lines = vec![header];
    for entry in &plan.entries {
        lines.push(String::new());
        lines.push(format!("Repository {}/{}", entry.owner, entry.repository));
        if let Some(os) = entry.os {
            lines.push(format!("  os:                {os}"));
        }
        lines.push(format!("  slots:             {}", entry.slots));
        lines.push(format!("  labels:            {}", entry.labels.join(", ")));
        lines.push(format!("  token file:        {}", entry.token_path));
        lines.push(format!("  name:              {}", entry.name));
        lines.push(format!(
            "  cpus:              {}",
            format_cpu_count(entry.cpus)
        ));
        lines.push(format!("  memory:            {} GB", entry.memory_gb));
        if let Some(disk) = entry.disk_gb {
            lines.push(format!("  disk:              {disk} GB"));
        }
        lines.push(format!("  diagnostics:       {}", flag(entry.diagnostics)));
        lines.push(format!(
            "  raw records:       {}",
            flag(entry.diagnostic_raw_records)
        ));
    }
    if let Some(budget) = &plan.budget {
        let used = usage(&plan.entries);
        lines.push(String::new());
        lines.push("Budget use".to_string());
        lines.push(format!(
            "  macOS VMs:         {} of {}",
            used.macos_vms,
            crate::validate::MAX_MACOS_VMS
        ));
        lines.push(format!(
            "  cpus:              {} of {}",
            format_nanos(used.cpu_nanos),
            format_cpu_count(budget.cpus)
        ));
        lines.push(format!(
            "  memory:            {} of {} GB",
            used.memory_gb, budget.memory_gb
        ));
        lines.push(format!(
            "  disk:              {} of {} GB",
            used.disk_gb, budget.disk_gb
        ));
    }
    lines
}
