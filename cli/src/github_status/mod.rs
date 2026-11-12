//! GitHub status: what GitHub says about this host's runners, for the window only.
//! While a client holds a renewed `github` subscription the agent reads each
//! repository's runner list, looks up the job a busy runner runs, and keeps the
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
    /// runner entries left out from the end to keep the value within the agent's
    /// message limit.
    #[serde(default)]
    pub omitted_runners: u32,
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
    /// leaves out runner entries from the end until the value's JSON is at most
    /// `max_bytes`, and counts them in `omitted_runners`.
    pub fn fit(&mut self, max_bytes: usize) {
        while serde_json::to_vec(self).map_or(usize::MAX, |json| json.len()) > max_bytes {
            let Some(runners) = self
                .repositories
                .iter_mut()
                .rev()
                .map(|repository| &mut repository.runners)
                .find(|runners| !runners.is_empty())
            else {
                return;
            };
            runners.pop();
            self.omitted_runners += 1;
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
}
