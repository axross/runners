//! checks a group's admission boundary at registration time, independently of the
//! window's display queue; only an authorised fresh page or 304 can supply its id.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::github_status::answer::Next;
use crate::github_status::collector::MAX_PAGES;
use crate::github_status::http::{Fetch, Get, GitHubReader, next_page, page_id};
use crate::runtime::secret::Secret;

use super::registration::{HttpLimits, SetupError};

#[derive(Clone, Deserialize)]
struct Group {
    id: u64,
    name: String,
    default: bool,
    inherited: bool,
    visibility: String,
    allows_public_repositories: bool,
}

#[derive(Deserialize)]
struct Groups {
    runner_groups: Vec<Group>,
}

#[derive(Clone)]
struct Page {
    etag: Option<String>,
    groups: Vec<Group>,
    link: Option<String>,
}

/// registration's conditional cache, separate from the optional display queue;
/// cached flags alone never authorise a registration.
pub(super) struct GroupCheck {
    reader: GitHubReader,
    pages: RefCell<BTreeMap<String, Page>>,
}

impl GroupCheck {
    /// shares registration's HTTP limits without sharing the display collector's
    /// subscription lifetime or retry schedule.
    pub(super) fn new(base: Option<&str>, limits: HttpLimits) -> Result<Self, SetupError> {
        Ok(Self {
            reader: GitHubReader::new(base, limits)?,
            pages: RefCell::new(BTreeMap::new()),
        })
    }

    /// checks every required page before selecting a group, so a failed later page
    /// cannot turn a partial answer into permission to register.
    pub(super) async fn checked_id(
        &self,
        organization: &str,
        name: &str,
        token: &Secret,
    ) -> Result<u64, String> {
        let first = format!("/orgs/{organization}/actions/runner-groups?per_page=100");
        let mut path = first.clone();
        let mut seen = BTreeSet::new();
        let mut selected = None;
        // the `/organizations/<id>` id of the read, held from the first page that names one
        let mut held_id: Option<String> = None;
        for _ in 0..MAX_PAGES {
            if !seen.insert(path.clone()) {
                return Err("the runner group pages repeat".to_string());
            }
            let page = self.page(&path, token).await?;
            for group in &page.groups {
                if group.name == name {
                    if selected.is_some() {
                        return Err("the runner group name is ambiguous".to_string());
                    }
                    selected = Some(group.clone());
                }
            }
            if held_id.is_none() {
                held_id = page_id(&path).map(str::to_string);
            }
            let next = page.link.as_deref().map_or(Next::None, |link| {
                next_page(link, self.reader.base_url(), &first, held_id.as_deref())
            });
            match next {
                Next::None => return selected.ok_or("runner group not found".to_string())?.id(),
                Next::Page(next) => path = next,
                Next::Refused => return Err("the runner group next page is refused".to_string()),
            }
        }
        Err("the runner group list has too many pages".to_string())
    }

    /// a cached page is usable only after this credential gets an authorised 304;
    /// a failed read never falls back to the last successful flags.
    async fn page(&self, path: &str, token: &Secret) -> Result<Page, String> {
        let cached = self.pages.borrow().get(path).cloned();
        let answer = self
            .reader
            .get(&Get {
                path,
                token,
                etag: cached.as_ref().and_then(|page| page.etag.as_deref()),
            })
            .await
            .map_err(|cause| format!("the runner group read failed: {}", cause.word()))?;
        match answer.status {
            200 => {
                let groups: Groups = serde_json::from_slice(&answer.body)
                    .map_err(|_| "GitHub returned an invalid runner group list".to_string())?;
                let page = Page {
                    etag: answer.etag,
                    groups: groups.runner_groups,
                    link: answer.link,
                };
                self.pages
                    .borrow_mut()
                    .insert(path.to_string(), page.clone());
                Ok(page)
            }
            304 => cached.ok_or_else(|| "the runner group page has no cached answer".to_string()),
            404 => Err("organisation not found with this token".to_string()),
            status => Err(format!(
                "Response status code does not indicate success: {status}. {}",
                answer.message.as_deref().unwrap_or_default()
            )),
        }
    }
}

impl Group {
    /// returns an id only for the selected-private boundary; unknown flags or ids
    /// must not widen which repositories can reach a host.
    fn id(&self) -> Result<u64, String> {
        let cause = if self.default {
            "the default group is refused"
        } else if self.inherited {
            "an inherited group is refused"
        } else if self.visibility == "all" {
            "the group admits every repository"
        } else if self.visibility == "private" {
            "the group admits every private repository"
        } else if self.visibility != "selected" {
            "the runner group visibility is not recognised"
        } else if self.allows_public_repositories {
            "the group allows public repositories"
        } else if self.id == 0 {
            "the runner group id is invalid"
        } else {
            return Ok(self.id);
        };
        Err(cause.to_string())
    }
}
