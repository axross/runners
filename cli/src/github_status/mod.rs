//! GitHub status: what GitHub says about this host's runners, for the window only.
//! While a client holds a renewed `github` subscription the agent reads each
//! scope's runner list, looks up jobs for repository runners only, and keeps the
//! request budget, with conditional requests sent one at a time; the value it
//! publishes never carries a token, a URL from GitHub or a raw response body.
//! `docs/operations/cli.md` describes the contract.

pub mod answer;
pub mod budget;
pub mod clean;
pub mod collector;
pub mod http;

use serde::{Deserialize, Serialize};

/// the value of the `github` topic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GithubStatus {
    /// `None` until a display request has been answered.
    #[serde(default)]
    pub budget: Option<Budget>,
    /// the repositories with a registered runner and a stored token, sorted by
    /// name, each with what the agent last learned of it.
    #[serde(default)]
    pub repositories: Vec<RepositoryStatus>,
    #[serde(default)]
    pub organizations: Vec<OrganizationStatus>,
    /// group entries left out, after runners and scale sets, to keep the message bounded.
    #[serde(default)]
    pub omitted_organizations: u32,
    /// runner entries left out from the end to keep the value within the agent's
    /// message limit.
    #[serde(default)]
    pub omitted_runners: u32,
    #[serde(default)]
    pub scale_sets: Vec<ScaleSetStatus>,
    /// scale-set entries left out, after runners, to keep the message bounded.
    #[serde(default)]
    pub omitted_scale_sets: u32,
}

/// counts from a pool's existing listener; no display request is made for this scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ScaleSetStatus {
    pub pool: String,
    pub repository: String,
    pub name: String,
    pub assigned: Option<u64>,
    pub running: Option<u64>,
    pub registered_idle: Option<u64>,
}

/// an organization group's last check, without repository job information.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OrganizationStatus {
    #[serde(default)]
    pub organization: String,
    #[serde(default)]
    pub runner_group: String,
    /// milliseconds since the Unix epoch.
    #[serde(default)]
    pub checked_at: Option<u64>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub runners: Vec<OrganizationRunnerStatus>,
}

/// an organization runner; its token cannot read repository jobs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OrganizationRunnerStatus {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub github: String,
}

/// the hourly request budget as display responses report it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Budget {
    pub limit: u64,
    /// the lowest count seen in the current reset window.
    pub remaining: u64,
    /// milliseconds since the Unix epoch.
    pub resets_at: u64,
    /// when the latest display response arrived, in milliseconds since the epoch.
    pub seen_at: u64,
    /// `normal`, `slowed` or `paused`; an open set.
    pub level: String,
}

/// one repository's last check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RepositoryStatus {
    /// `owner/name` in lower case, the token store's key.
    #[serde(default)]
    pub repository: String,
    /// when GitHub last answered the runner list, in milliseconds since the epoch.
    #[serde(default)]
    pub checked_at: Option<u64>,
    /// `ok`, or the cause: `token_rejected`, `not_found`, `moved`, `unreachable`,
    /// `error` or `token_unreadable`; an open set.
    #[serde(default)]
    pub status: String,
    /// when the agent tries again after a failure, in milliseconds since the epoch.
    #[serde(default)]
    pub retry_at: Option<u64>,
    /// `unknown`, `shown`, `hidden` or `unreadable`; an open set.
    #[serde(default)]
    pub job_names: String,
    /// why job names are unreadable, in the words of `status`.
    #[serde(default)]
    pub job_names_cause: Option<String>,
    /// when the agent looks up jobs again after job names became unreadable, in
    /// milliseconds since the epoch.
    #[serde(default)]
    pub job_names_retry_at: Option<u64>,
    /// this host's runners that GitHub lists, by name.
    #[serde(default)]
    pub runners: Vec<RunnerStatus>,
}

/// one runner as GitHub lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RunnerStatus {
    /// the runner's name, which is the snapshot's `workload`.
    #[serde(default)]
    pub name: String,
    /// `offline`, `idle` or `busy`; an open set.
    #[serde(default)]
    pub github: String,
    /// the job a busy runner runs, once it is known.
    #[serde(default)]
    pub job: Option<Job>,
}

/// a running job; the names are cleaned and cut as [`clean::name`] does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Job {
    #[serde(default)]
    pub workflow: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub run_id: u64,
    #[serde(default)]
    pub job_id: u64,
}

impl GithubStatus {
    /// leaves entries out from the end until the value's JSON is at most `max_bytes`, in
    /// this order: organization runner entries, then repository runner entries, both
    /// counted in `omitted_runners`; then scale sets, in `omitted_scale_sets`; then
    /// organization group entries, in `omitted_organizations`. A scale set holds only
    /// counts, while a group entry says whether its organization's token and runner list
    /// work.
    pub fn fit(&mut self, max_bytes: usize) {
        while serde_json::to_vec(self).map_or(usize::MAX, |json| json.len()) > max_bytes {
            if let Some(runners) = self
                .organizations
                .iter_mut()
                .rev()
                .map(|organization| &mut organization.runners)
                .find(|runners| !runners.is_empty())
            {
                runners.pop();
                self.omitted_runners += 1;
            } else if let Some(runners) = self
                .repositories
                .iter_mut()
                .rev()
                .map(|repository| &mut repository.runners)
                .find(|runners| !runners.is_empty())
            {
                runners.pop();
                self.omitted_runners += 1;
            } else if self.scale_sets.pop().is_some() {
                self.omitted_scale_sets += 1;
            } else if self.organizations.pop().is_some() {
                self.omitted_organizations += 1;
            } else {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner(index: usize) -> RunnerStatus {
        RunnerStatus {
            name: format!("app-{index}-20261104031522114"),
            github: "busy".to_string(),
            job: Some(Job {
                workflow: "w".repeat(120),
                name: "n".repeat(120),
                run_id: 1,
                job_id: 2,
            }),
        }
    }

    #[test]
    fn an_oversize_value_leaves_out_runners_from_the_end_and_counts_them() {
        let mut status = GithubStatus {
            repositories: vec![
                RepositoryStatus {
                    repository: "o/a".to_string(),
                    runners: (0..3).map(runner).collect(),
                    ..RepositoryStatus::default()
                },
                RepositoryStatus {
                    repository: "o/b".to_string(),
                    runners: (3..5).map(runner).collect(),
                    ..RepositoryStatus::default()
                },
            ],
            ..GithubStatus::default()
        };
        let whole = serde_json::to_vec(&status).unwrap().len();
        let one = serde_json::to_vec(&runner(0)).unwrap().len();
        status.fit(whole - one - one / 2);
        assert_eq!(status.omitted_runners, 2);
        assert_eq!(status.repositories[0].runners.len(), 3);
        assert!(status.repositories[1].runners.is_empty());
        assert!(serde_json::to_vec(&status).unwrap().len() <= whole - one - one / 2);
    }

    #[test]
    fn scale_sets_alone_cannot_exceed_the_message_bound() {
        let mut status = GithubStatus {
            scale_sets: (0..4)
                .map(|index| ScaleSetStatus {
                    pool: format!("pool-{index}"),
                    repository: "owner/repository".to_string(),
                    name: format!("axpc-pool-{index}"),
                    assigned: Some(5),
                    running: Some(2),
                    registered_idle: Some(1),
                })
                .collect(),
            ..GithubStatus::default()
        };
        let mut expected = status.clone();
        expected.scale_sets.truncate(1);
        expected.omitted_scale_sets = 3;
        let bound = serde_json::to_vec(&expected).unwrap().len();
        status.fit(bound);
        assert_eq!(status, expected);
        assert!(serde_json::to_vec(&status).unwrap().len() <= bound);
    }
}
