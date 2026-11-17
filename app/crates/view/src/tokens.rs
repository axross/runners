//! the Tokens tab, and what a pool's card says of its repository's token. The view
//! never holds a token: it knows whether the store has one, what the agent says of a
//! registration GitHub refused, and what the last save came to.

use marshal_cli::agent::snapshot::{PoolInfo, RunnerState, Snapshot};
use marshal_cli::agent::tokens::{NO_TOKEN, NO_TOKEN_STORE, STORE_FAILED};
use marshal_cli::runtime::registration::REJECTED_MARK;
use serde::Serialize;

use crate::control::{Action, Confirmation, Control, Look, control};
use crate::host::HostWords;
use crate::input::{NotSaved, TokenResult};
use crate::state::{Context, Resident, is_rejection};
use crate::text::{capitalised, counted, list_of, plain};

const TOKEN_MAX: &str = "a token is at most 1,280 characters";
const ENTER_AGAIN: &str = "The field is empty; enter the token again.";

/// a repository the configuration names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// `owner/name` in lower case.
    pub key: String,
    pub has_token: bool,
    pub pools: Vec<String>,
}

/// the repositories of the snapshot, with any pool's repository the list left out.
pub fn repositories(context: &Context<'_>) -> Vec<Repository> {
    let Some(snapshot) = context.snapshot else {
        return Vec::new();
    };
    let mut found: Vec<Repository> = snapshot
        .repositories
        .iter()
        .map(|repository| Repository {
            key: plain(&repository.repository),
            has_token: repository.has_token,
            pools: repository.pools.iter().map(|pool| plain(pool)).collect(),
        })
        .collect();
    for pool in &snapshot.pools {
        let key = plain(&pool.repository);
        match found.iter_mut().find(|known| known.key == key) {
            Some(known) => {
                let name = plain(&pool.name);
                if !known.pools.contains(&name) {
                    known.pools.push(name);
                }
            }
            None => found.push(Repository {
                key,
                has_token: pool.has_token,
                pools: vec![plain(&pool.name)],
            }),
        }
    }
    found
}

/// why the agent skipped a pool, without the configuration path it puts first: a
/// skip's reason reads `pool[0]: no token.` from `config.toml`, and
/// `repositories[0]: no token.` from an agent that read `config.json`.
pub(crate) fn skip_cause(reason: &str) -> &str {
    let why = reason
        .split_once("]: ")
        .filter(|(path, _)| path.starts_with("pool[") || path.starts_with("repositories["))
        .map_or(reason, |(_, why)| why);
    why.strip_suffix('.').unwrap_or(why)
}

/// whether a pool was skipped for want of a token.
fn skipped_for_no_token(pool: &PoolInfo) -> bool {
    pool.reason.as_deref().map(skip_cause) == Some(NO_TOKEN)
}

/// why the agent could not read the repository's token, from a pool it skipped for
/// that: the store failed, or there is none.
pub(crate) fn store_problem(context: &Context<'_>, repository: &str) -> Option<String> {
    context
        .pools
        .iter()
        .filter(|pool| plain(&pool.repository) == repository)
        .filter_map(|pool| pool.reason.as_deref().map(skip_cause))
        .find(|cause| cause.starts_with(STORE_FAILED) || *cause == NO_TOKEN_STORE)
        .map(|cause| format!("{}.", capitalised(&plain(cause))))
}

/// the pools a save for `repository`, as the view keys it, starts at once: those
/// skipped for want of a token, while the agent runs. The app reads them before it sends the token.
#[must_use]
pub fn pools_started_by(snapshot: &Snapshot, repository: &str) -> Vec<String> {
    if snapshot.agent.state != "running" {
        return Vec::new();
    }
    snapshot
        .pools
        .iter()
        .filter(|pool| {
            plain(&pool.repository) == repository
                && pool.state == "skipped"
                && skipped_for_no_token(pool)
        })
        .map(|pool| plain(&pool.name))
        .collect()
}

/// why GitHub did not accept the token, as GitHub says it: the `message` of the body
/// the agent kept in the runner's reason.
fn github_message(reason: &str) -> String {
    reason
        .split_once(REJECTED_MARK)
        .and_then(|(_, after)| after.get(after.find('{')?..=after.rfind('}')?))
        .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
        .and_then(|value| value.get("message")?.as_str().map(plain))
        .unwrap_or_else(|| "Unauthorized".to_string())
}

/// the line a pool of `repository` shows when GitHub refuses its token, or `None`.
fn rejection(context: &Context<'_>, repository: &str) -> Option<String> {
    let reason = context
        .pools
        .iter()
        .filter(|pool| plain(&pool.repository) == repository)
        .flat_map(|pool| &pool.runners)
        .filter(|runner| runner.kind() == RunnerState::BackingOff)
        .filter_map(|runner| runner.reason.as_deref())
        .find(|reason| is_rejection(reason))?;
    Some(format!(
        "GitHub rejected the token for {repository}: {}.",
        github_message(reason)
    ))
}

/// where a repository's token stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenState {
    Set,
    Missing,
    Rejected,
}

fn state_of(context: &Context<'_>, repository: &Repository) -> (TokenState, String) {
    if !repository.has_token {
        let text =
            store_problem(context, &repository.key).unwrap_or_else(|| "No token".to_string());
        return (TokenState::Missing, text);
    }
    match rejection(context, &repository.key) {
        Some(text) => (TokenState::Rejected, text),
        None => (TokenState::Set, "Token set".to_string()),
    }
}

/// the pools one of whose runners is up.
fn runners_up_in(context: &Context<'_>, pools: &[String]) -> bool {
    context
        .pools
        .iter()
        .any(|pool| pools.contains(&plain(&pool.name)) && pool.runners_up > 0)
}

fn removal_question(context: &Context<'_>, repository: &Repository) -> Confirmation {
    let pools = if repository.pools.len() == 1 {
        format!("Pool {} stops", list_of(&repository.pools))
    } else {
        format!("Pools {} stop", list_of(&repository.pools))
    };
    let tail = if runners_up_in(context, &repository.pools) {
        "; runners already up finish their job."
    } else {
        "."
    };
    Confirmation {
        title: format!("Remove the token for {}?", repository.key),
        body: format!("{pools} getting new runners{tail}"),
        confirm: "Remove".to_string(),
        cancel: "Cancel".to_string(),
    }
}

/// the text and the pill a pool's card adds for its repository's token, or `None`
/// where the card is as it was.
pub fn pool_note(
    context: &Context<'_>,
    repositories: &[Repository],
    pool: &PoolInfo,
) -> Option<(String, Control)> {
    let repository = repositories
        .iter()
        .find(|repository| repository.key == plain(&pool.repository))?;
    let key = repository.key.clone();
    match state_of(context, repository) {
        (TokenState::Set, _) => None,
        (TokenState::Rejected, text) => Some((
            text,
            control(
                context,
                Action::ReplaceToken(key),
                "Replace Token",
                Look::Tonal,
                None,
            ),
        )),
        // a pool skipped for another reason keeps the agent's words on its card
        (TokenState::Missing, _) if pool.reason.is_some() && !skipped_for_no_token(pool) => None,
        (TokenState::Missing, _) => {
            let skipped = skipped_for_no_token(pool);
            let text = if skipped {
                format!("No token for {key}. Start All skipped this pool.")
            } else if matches!(context.resident, Resident::Running | Resident::Resuming)
                && matches!(pool.state.as_str(), "running" | "preparing")
            {
                if pool.runners_up > 0 {
                    format!(
                        "No token for {key}. Runners up finish their jobs; no new runner registers."
                    )
                } else {
                    format!("No token for {key}. No new runner registers.")
                }
            } else {
                format!("No token for {key}.")
            };
            Some((
                text,
                control(
                    context,
                    Action::SetToken(key),
                    "Set Token",
                    Look::Tonal,
                    None,
                ),
            ))
        }
    }
}

/// the field that opens in a repository's row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenForm {
    /// the field's label.
    pub label: String,
    pub hint: String,
    pub save: String,
    pub cancel: String,
    /// why Save is unavailable while the field is empty; the page shows it as the
    /// control's reason, since the view never sees what is typed.
    pub save_reason_when_empty: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenRow {
    pub repository: String,
    /// "Used by pool app", "Used by pools docs and docs-arm".
    pub pools: String,
    pub state: TokenState,
    /// "Token set", "No token", or why GitHub refused the token.
    pub state_text: String,
    /// what the latest save or removal came to.
    pub status: Option<String>,
    pub controls: Vec<Control>,
    /// the field, while it is open.
    pub form: Option<TokenForm>,
    /// whether the removal asks its question in the row.
    pub confirming_removal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokensTab {
    pub intro: String,
    pub rows: Vec<TokenRow>,
    /// "3 repositories need attention", on the tab while some repository has no usable
    /// token.
    pub chip: Option<String>,
}

fn intro(words: &HostWords) -> String {
    let store = words.store;
    format!(
        "Each repository has one token, and every pool on that repository uses it. \
Marshal keeps tokens in {store} and never shows one."
    )
}

fn hint(repository: &str, words: &HostWords) -> String {
    let store = words.store;
    format!(
        "A fine-grained personal access token with Administration: read and write for {repository} only. \
Marshal keeps it in {store} and never shows it again."
    )
}

fn row(context: &Context<'_>, repository: &Repository) -> TokenRow {
    let (state, state_text) = state_of(context, repository);
    let key = &repository.key;
    let pools = if repository.pools.len() == 1 {
        format!("Used by pool {}", list_of(&repository.pools))
    } else {
        format!("Used by pools {}", list_of(&repository.pools))
    };
    let controls = match state {
        TokenState::Missing => vec![control(
            context,
            Action::SetToken(key.clone()),
            "Set Token",
            Look::Tonal,
            None,
        )],
        TokenState::Set | TokenState::Rejected => vec![
            control(
                context,
                Action::ReplaceToken(key.clone()),
                "Replace Token",
                Look::Tonal,
                None,
            ),
            control(
                context,
                Action::RemoveToken(key.clone()),
                "Remove Token",
                Look::Destructive,
                Some(removal_question(context, repository)),
            ),
        ],
    };
    let ui = context.input.tokens;
    let editing = ui.is_some_and(|ui| ui.editing.as_deref() == Some(key));
    let result = ui.and_then(|ui| ui.results.get(key));
    let failure = result.and_then(|result| match result {
        TokenResult::NotSaved(failure) => Some(failure.message(&context.input.words)),
        _ => None,
    });
    TokenRow {
        repository: key.clone(),
        pools,
        state,
        state_text,
        status: match result {
            Some(TokenResult::NotSaved(_)) if editing => None,
            Some(result) => Some(result.status_line(key, &context.input.words)),
            None => None,
        },
        controls,
        form: editing.then(|| TokenForm {
            label: format!("Token for {key}"),
            hint: hint(key, &context.input.words),
            save: "Save".to_string(),
            cancel: "Cancel".to_string(),
            save_reason_when_empty: "Unavailable: the field is empty.".to_string(),
            error: failure,
        }),
        confirming_removal: ui.is_some_and(|ui| ui.removing.as_deref() == Some(key)),
    }
}

/// whether no repository has a token, so the window opens on the Tokens tab.
pub fn none_has_a_token(context: &Context<'_>) -> bool {
    let repositories = repositories(context);
    !repositories.is_empty() && repositories.iter().all(|repository| !repository.has_token)
}

pub fn tab(context: &Context<'_>) -> TokensTab {
    let rows: Vec<TokenRow> = repositories(context)
        .iter()
        .map(|repository| row(context, repository))
        .collect();
    let attention = rows
        .iter()
        .filter(|row| row.state != TokenState::Set)
        .count();
    let attention = u32::try_from(attention).unwrap_or(u32::MAX);
    TokensTab {
        intro: intro(&context.input.words),
        chip: (attention > 0).then(|| {
            if attention == 1 {
                "1 repository needs attention".to_string()
            } else {
                format!(
                    "{} need attention",
                    counted(attention, "repository", "repositories")
                )
            }
        }),
        rows,
    }
}

impl NotSaved {
    /// the message the open field shows, which says the token was not saved and to enter
    /// it again.
    #[must_use]
    pub fn message(&self, words: &HostWords) -> String {
        let store = words.store;
        let reason = match self {
            Self::TooLong => TOKEN_MAX.to_string(),
            Self::Store {
                detail: Some(detail),
            } => {
                format!("{store} refused the token ({})", plain(detail))
            }
            Self::Store { detail: None } => format!("{store} refused the token"),
            Self::Refused(words) => plain(words),
            Self::LostContact => "Marshal has lost contact with the agent".to_string(),
        };
        format!("Not saved: {}. {ENTER_AGAIN}", reason.trim_end_matches('.'))
    }

    fn announcement(&self, repository: &str, words: &HostWords) -> String {
        let store = words.store;
        match self {
            Self::LostContact => discarded_announcement(repository, true),
            Self::TooLong => format!(
                "Token for {repository} not saved. {}. {ENTER_AGAIN}",
                capitalised(TOKEN_MAX)
            ),
            Self::Store {
                detail: Some(detail),
            } => format!(
                "Token for {repository} not saved. {store} refused the token: {}. {ENTER_AGAIN}",
                plain(detail)
            ),
            Self::Store { detail: None } => format!(
                "Token for {repository} not saved. {store} refused the token. {ENTER_AGAIN}"
            ),
            Self::Refused(words) => format!(
                "Token for {repository} not saved. {}. {ENTER_AGAIN}",
                plain(words).trim_end_matches('.')
            ),
        }
    }
}

impl TokenResult {
    /// the line the row shows, which is also what the live region says.
    #[must_use]
    pub fn status_line(&self, repository: &str, words: &HostWords) -> String {
        match self {
            Self::Saved { started } if started.is_empty() => {
                format!("Token for {repository} saved.")
            }
            Self::Saved { started } => format!(
                "Token for {repository} saved. Starting {} {}.",
                if started.len() == 1 { "pool" } else { "pools" },
                list_of(started)
            ),
            Self::Removed => format!("Token for {repository} removed."),
            Self::NotSaved(failure) => failure.message(words),
            Self::NotRemoved(why) => format!(
                "Token for {repository} not removed. {}. It is still stored.",
                capitalised(plain(why).trim_end_matches('.'))
            ),
            Self::NotAnswered { removal } => format!(
                "The agent has not answered yet about {} the token for {repository}. It may still do it; this row shows where the token stands.",
                if *removal { "removing" } else { "saving" }
            ),
        }
    }

    /// what the window's polite region says of the result.
    #[must_use]
    pub fn announcement(&self, repository: &str, words: &HostWords) -> String {
        match self {
            Self::NotSaved(failure) => failure.announcement(repository, words),
            other => other.status_line(repository, words),
        }
    }
}

/// what the live region says when an open field closes without a save: another
/// repository's field opened, or contact was lost.
#[must_use]
pub fn discarded_announcement(repository: &str, lost_contact: bool) -> String {
    if lost_contact {
        format!(
            "The token for {repository} was not saved. Marshal has lost contact with the agent."
        )
    } else {
        format!("The token for {repository} was not saved.")
    }
}
