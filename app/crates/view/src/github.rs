//! what GitHub says about the runners, beside their local state: each runner's GitHub
//! state with the job a busy one runs, a pool's GitHub notices, the request budget,
//! the pause, and the job page Open run opens. Workflow and job names come from GitHub
//! and are shown only as plain text.

use marshal_cli::agent::snapshot::{PoolInfo, RunnerInfo, Snapshot};
use marshal_cli::github_status::{GithubStatus, RepositoryStatus, RunnerStatus, clean};
use marshal_cli::token_store::TokenKey;
use serde::Serialize;

use crate::banner::{Banner, BannerKind, Tone};
use crate::host::WINDOWS;
use crate::input::GithubFeed;
use crate::state::Context;
use crate::text::{counted, plain};
use crate::view::Attention;

/// where the hidden-names notice sends the reader, in one string so a platform where
/// that document does not apply changes it in one place.
pub const HIDDEN_NAMES: &str = "Job names hidden: this repository's token has no Actions: read. Windows Runner Host says how to add it.";

/// [`HIDDEN_NAMES`] with the host's document.
fn hidden_names(context: &Context<'_>) -> String {
    HIDDEN_NAMES.replace(WINDOWS.host_document, context.input.words.host_document)
}

pub const UNAVAILABLE: &str = "GitHub status unavailable: the running agent does not report it.";

pub const UNAVAILABLE_FOR_NOW: &str = "GitHub status unavailable for now: the agent has no room for another subscription. Asking again every 60 s.";

pub const JOB_ENDED: &str = "That job has ended.";

const BUDGET_LABEL: &str = "GitHub requests";

/// a busy runner's Open run link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenRun {
    pub label: String,
    /// the link's accessible name, which names the job and the runner.
    pub name: String,
    /// the runner's name, which the page passes back to open its job.
    pub runner: String,
}

/// a runner's GitHub state, as the row shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerGithub {
    /// "GitHub: idle".
    pub state: String,
    pub busy: bool,
    /// "CI · test (ubuntu-latest)".
    pub job: Option<String>,
    pub open_run: Option<OpenRun>,
    /// "checked 03:41", while the state is GitHub's last known, not a fresh one.
    pub checked: Option<String>,
}

/// a GitHub notice on a pool's card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GithubNote {
    pub text: String,
    /// "Last checked 03:41 · trying again at 03:46."
    pub line: Option<String>,
    /// what the live region says when the notice appears, and the key it is said once
    /// under.
    #[serde(skip)]
    pub attention: Option<Attention>,
}

fn status<'a>(context: &Context<'a>) -> Option<(u64, Option<&'a GithubStatus>)> {
    match context.input.github {
        GithubFeed::Live { opened_ms, status } => Some((opened_ms, status)),
        _ => None,
    }
}

fn key_of(repository: &str) -> String {
    TokenKey::parse(repository)
        .map(|key| key.as_str().to_string())
        .unwrap_or_else(|_| repository.to_ascii_lowercase())
}

fn repository<'a>(status: &'a GithubStatus, pool: &PoolInfo) -> Option<&'a RepositoryStatus> {
    let key = key_of(&pool.repository);
    status
        .repositories
        .iter()
        .find(|repository| repository.repository == key)
}

fn paused(status: Option<&GithubStatus>) -> bool {
    status
        .and_then(|status| status.budget.as_ref())
        .is_some_and(|budget| budget.level == "paused")
}

/// a job's two names, cleaned again here since the value comes from another process.
fn job_text(workflow: &str, name: &str) -> String {
    format!(
        "{} · {}",
        plain(&clean::name(workflow)),
        plain(&clean::name(name))
    )
}

fn entry<'a>(repository: &'a RepositoryStatus, workload: &str) -> Option<&'a RunnerStatus> {
    repository
        .runners
        .iter()
        .find(|runner| runner.name == workload)
}

/// the runner's GitHub state, or `None` while the view shows no GitHub status or the
/// runner has no registration.
pub fn runner(context: &Context<'_>, pool: &PoolInfo, runner: &RunnerInfo) -> Option<RunnerGithub> {
    let (opened_ms, status) = status(context)?;
    let workload = runner.workload.as_deref()?;
    if !pool.has_token {
        return None;
    }
    let checking = RunnerGithub {
        state: "GitHub: checking…".to_string(),
        busy: false,
        job: None,
        open_run: None,
        checked: None,
    };
    let Some(value) = status else {
        return Some(checking);
    };
    let Some(repository) = repository(value, pool) else {
        return Some(checking);
    };
    // while paused, or while GitHub cannot answer, the last values stay, marked with
    // their time, even when they were read before the window opened
    let held = cause(&repository.status).is_some() || paused(status);
    let since = if held {
        runner.since
    } else {
        runner.since.max(opened_ms)
    };
    let Some(checked_at) = repository.checked_at.filter(|at| *at > since) else {
        return Some(checking);
    };
    let checked = held.then(|| format!("checked {}", context.input.clock.when(checked_at)));
    let Some(listed) = entry(repository, workload) else {
        let state = if value.omitted_runners > 0 {
            "GitHub: not shown"
        } else {
            "GitHub: not listed"
        };
        return Some(RunnerGithub {
            state: state.to_string(),
            checked,
            ..checking
        });
    };
    let busy = listed.github == "busy";
    // a word this reader does not know is not known: the runner reads as checking
    let state = match listed.github.as_str() {
        "offline" => "GitHub: offline",
        "idle" => "GitHub: idle",
        "busy" => "GitHub: running a job",
        _ => "GitHub: checking…",
    }
    .to_string();
    let job = listed
        .job
        .as_ref()
        .filter(|_| busy)
        .map(|job| job_text(&job.workflow, &job.name));
    let open_run = job
        .as_ref()
        .filter(|_| !context.is_stale())
        .map(|job| OpenRun {
            label: "Open run".to_string(),
            name: format!(
                "Open run of {job}, {}, Runner {}, on GitHub",
                plain(&pool.name),
                runner.slot
            ),
            runner: plain(workload),
        });
    Some(RunnerGithub {
        state,
        busy,
        job,
        open_run,
        checked,
    })
}

/// how many of the listed runners GitHub reports running a job.
pub fn running_jobs(context: &Context<'_>) -> u32 {
    let busy = context
        .pools
        .iter()
        .flat_map(|pool| pool.runners.iter().map(move |each| (pool, each)))
        .filter(|(pool, each)| runner(context, pool, each).is_some_and(|github| github.busy))
        .count();
    u32::try_from(busy).unwrap_or(u32::MAX)
}

/// " · 2 running jobs", or nothing while no runner runs one.
pub fn running_jobs_phrase(context: &Context<'_>) -> String {
    match running_jobs(context) {
        0 => String::new(),
        count => format!(" · {}", counted(count, "running job", "running jobs")),
    }
}

/// a repository's cause in words, or `None` for `ok` and a word this reader does not
/// know, which says nothing.
fn cause(word: &str) -> Option<&'static str> {
    Some(match word {
        "token_rejected" => {
            "GitHub rejected this repository's token. Replace it on the Tokens tab."
        }
        "not_found" => "GitHub cannot find this repository with its token.",
        "moved" => "This repository has moved on GitHub.",
        "unreachable" => "GitHub could not be reached.",
        "error" => "GitHub answered with an error.",
        "token_unreadable" => "This repository's token cannot be read.",
        _ => return None,
    })
}

fn time_line(context: &Context<'_>, checked_at: Option<u64>, retry_at: Option<u64>) -> String {
    let clock = &context.input.clock;
    let checked = checked_at.map_or_else(
        || "Not checked yet".to_string(),
        |at| format!("Last checked {}", clock.when(at)),
    );
    match retry_at {
        Some(at) => format!("{checked} · trying again at {}.", clock.when(at)),
        None => format!("{checked}."),
    }
}

/// the GitHub notices of a pool's card. `token_note` says the card already gives W1's
/// reason for an unusable token, which W2 does not repeat.
pub fn notes(context: &Context<'_>, pool: &PoolInfo, token_note: bool) -> Vec<GithubNote> {
    let name = plain(&pool.name);
    let mut found = Vec::new();
    if let Some(at) = context
        .input
        .ended_jobs
        .and_then(|ended| ended.get(&pool.name))
    {
        found.push(GithubNote {
            text: JOB_ENDED.to_string(),
            line: None,
            attention: Some(Attention {
                key: format!("github:ended:{name}:{at}"),
                text: format!("Pool {name}: {JOB_ENDED}"),
            }),
        });
    }
    let Some((_, Some(value))) = status(context) else {
        return found;
    };
    if !pool.has_token {
        return found;
    }
    let Some(repository) = repository(value, pool) else {
        return found;
    };
    let key = plain(&repository.repository);
    let token_cause = matches!(
        repository.status.as_str(),
        "token_rejected" | "token_unreadable"
    );
    if let Some(text) = cause(&repository.status).filter(|_| !(token_cause && token_note)) {
        let text = text.to_string();
        found.push(GithubNote {
            line: Some(time_line(
                context,
                repository.checked_at,
                repository.retry_at,
            )),
            attention: Some(Attention {
                key: format!("github:repository:{key}:{}", repository.status),
                text: format!("{key}: {text}"),
            }),
            text,
        });
    }
    match repository.job_names.as_str() {
        "hidden" => found.push(GithubNote {
            text: hidden_names(context),
            line: None,
            attention: Some(Attention {
                key: format!("github:hidden:{name}"),
                text: format!("Pool {name}: {}", hidden_names(context)),
            }),
        }),
        "unreadable" => {
            // a cause this reader does not know reads as an error answer
            let why = repository
                .job_names_cause
                .as_deref()
                .and_then(cause)
                .unwrap_or("GitHub answered with an error.");
            let text = format!("Job names could not be read: {why}");
            found.push(GithubNote {
                line: Some(time_line(
                    context,
                    repository.checked_at,
                    repository.job_names_retry_at,
                )),
                attention: Some(Attention {
                    key: format!("github:unreadable:{name}:{why}"),
                    text: format!("Pool {name}: {text}"),
                }),
                text,
            });
        }
        _ => {}
    }
    found
}

/// 4210 as "4,210".
fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// the footer's GitHub lines: the budget, or why there is no GitHub status, and the
/// runners an oversize value left out.
pub fn footer_lines(context: &Context<'_>) -> Vec<String> {
    let (opened_ms, value) = match context.input.github {
        GithubFeed::Off => return Vec::new(),
        GithubFeed::Unavailable => return vec![UNAVAILABLE.to_string()],
        GithubFeed::UnavailableForNow => return vec![UNAVAILABLE_FOR_NOW.to_string()],
        GithubFeed::Live { opened_ms, status } => (opened_ms, status),
    };
    let clock = &context.input.clock;
    let budget = value.and_then(|value| value.budget.as_ref());
    let mut lines = vec![match budget {
        None => format!("{BUDGET_LABEL}: not known yet"),
        Some(budget) => {
            let mut parts = vec![
                format!(
                    "{BUDGET_LABEL}: {} of {} left this hour",
                    thousands(budget.remaining),
                    thousands(budget.limit)
                ),
                format!("resets {}", clock.when(budget.resets_at)),
            ];
            match budget.level.as_str() {
                "normal" => parts.push("checked every 15 s".to_string()),
                "slowed" => {
                    parts.push("checks slowed to once a minute to save the budget".to_string());
                }
                "paused" => parts.push(format!(
                    "checks paused until {}",
                    clock.when(budget.resets_at)
                )),
                _ => {}
            }
            if budget.seen_at < opened_ms {
                parts.push(format!("as of {}", clock.when(budget.seen_at)));
            }
            parts.join(" · ")
        }
    }];
    if let Some(omitted) = value
        .map(|value| value.omitted_runners)
        .filter(|count| *count > 0)
    {
        lines.push(format!(
            "GitHub's state cannot be shown for {}: the agent's report is too large.",
            counted(omitted, "runner", "runners")
        ));
    }
    lines
}

/// the budget's level while the view shows GitHub status.
pub fn level(context: &Context<'_>) -> Option<String> {
    let (_, value) = status(context)?;
    value?.budget.as_ref().map(|budget| budget.level.clone())
}

/// the banner that says GitHub checks are paused, while they are.
pub fn paused_banner(context: &Context<'_>) -> Option<Banner> {
    let (_, value) = status(context)?;
    let budget = value?
        .budget
        .as_ref()
        .filter(|budget| budget.level == "paused")?;
    let heading = format!(
        "GitHub checks paused until {}",
        context.input.clock.when(budget.resets_at)
    );
    let body = format!(
        "{} of {} requests left this hour. Runner states on this computer still update.",
        thousands(budget.remaining),
        thousands(budget.limit)
    );
    Some(Banner {
        kind: BannerKind::GithubPaused,
        tone: Tone::Warning,
        announcement: format!("{heading}: {body}"),
        heading,
        lines: vec![body],
        action: None,
    })
}

/// the one-time announcements of the feed itself.
pub fn feed_attention(context: &Context<'_>) -> Option<Attention> {
    let (key, text) = match context.input.github {
        GithubFeed::Unavailable => ("github:unavailable", UNAVAILABLE),
        GithubFeed::UnavailableForNow => ("github:unavailable-for-now", UNAVAILABLE_FOR_NOW),
        _ => return None,
    };
    Some(Attention {
        key: key.to_string(),
        text: text.to_string(),
    })
}

/// the address of the job a runner of `pool` named `runner` runs, built from the pool's
/// configured repository and the two numbers GitHub gave, or `None` when the snapshot
/// no longer lists that runner or its job is not known.
#[must_use]
pub fn job_page(
    snapshot: &Snapshot,
    status: &GithubStatus,
    pool: &str,
    runner: &str,
) -> Option<String> {
    let pool = snapshot.pools.iter().find(|each| each.name == pool)?;
    pool.runners
        .iter()
        .find(|each| each.workload.as_deref() == Some(runner))?;
    let key = TokenKey::parse(&pool.repository).ok()?;
    let job = entry(repository(status, pool)?, runner)
        .filter(|listed| listed.github == "busy")?
        .job
        .as_ref()?;
    Some(format!(
        "https://github.com/{}/actions/runs/{}/job/{}",
        key.as_str(),
        job.run_id,
        job.job_id
    ))
}
