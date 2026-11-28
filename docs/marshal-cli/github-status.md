---
type: Specification
title: "GitHub status polling"
description: "Which runner and job states can polling establish, and when does it back off?"
---

# GitHub status polling

## GitHub Status

GitHub status is what GitHub says about this host's runners, for the window only:
whether each is offline, idle or busy, the job a busy runner runs, and the request
budget. It lives in the library's `github_status` module beside registration, and the
agent runs it and publishes it as the `github` topic. It owns the busy flag, the job
lookup, the request budget and the back-off. Repositories and organisations are
separate scopes in the same queue; organisation status adds fields without
changing the repository fields' meaning.

The `github` value's `organizations` lists `organization`, `runner_group`,
`checked_at`, `status` and `runners`; each runner has only `name` and `github`
(`offline`, `idle` or `busy`), never `job` or `job_names`. An organisation is read
only while a configured pool has a registered runner on this host and its token
is present, through `GET /orgs/<org>/actions/runners?per_page=100`. Only this host's
runner names are shown. Pools and groups of one organisation share a conditional,
paged read and check time, with the same 15 s minimum, budget, subscription and
backoff rules as repositories. Group rows reflect configured pools, not detection
of a runner moved in GitHub. Organisation tokens never trigger repository job
lookups. Message cutting drops, from the end of each list, organisation runners, then
repository runners, then `scale_sets` entries, then organisation group rows, until the
value fits: `omitted_runners` counts dropped runners, `omitted_scale_sets` dropped scale
sets and `omitted_organizations` dropped group rows. A scale set holds only counts, while
a group row says whether its organisation's token and runner list work.

**Display collection follows the subscription.** The agent reads GitHub for display only while at least
one connection holds a `github` subscription that was subscribed or renewed in the
last 90 s. When the last one ends, no new request starts after 1 s; one in flight
finishes or times out and its answer is kept. The app holds that subscription on a
connection of its own while its window is shown and not minimized, and renews it every
30 s, so an app that hangs stops collection within 91 s. After collection stops the
agent keeps each repository's last pages and their `ETag`s.

An on-demand listener is independent of display collection and continues while the
window is hidden. Its `scale_sets` counts come from the session's initial or latest
message statistics, with no extra display request. A `202` retains the last counts.
The counts are published only while the `github` subscription is held, including
for a pool whose repository has no entry in `repositories`, and the entries are
cleared when the subscription ends, so the next subscriber's first value carries no
earlier counts. A listener that is not the pool's own, by pool, scale-set name and
repository, gives null counts. The existing
repository and budget fields keep their meanings.

**The checks.** A repository is checked while one of its pools has a runner with a
`workload` and the token store held its token at the last look; the token is read
from the store at each check by the repository's key, as a registration reads it.
A check reads the runner list in full, then, while a busy runner's job is not known,
sends up to two lookup requests: the repository's in-progress runs, newest first, then
the jobs (`filter=latest`) of those runs, continuing where it stopped at the next
check. A job is matched to its runner by name, kept for that runner's life, and
looked for in at most 20 checks. Every page is requested with `If-None-Match` and that
page's last `ETag`, so a `304` keeps it at no cost. Pages hold 100 runners, 10 runs
and 30 jobs; a read of more than 10 pages, a page over 1 MiB, or a `200` whose body
is not the list asked for reads as `error`. Only the jobs pages of runs still in
progress are kept. Requests go out one at a time across the host, in the order repositories
fall due. A repository is checked at most once per 15 s at the `normal` level and
once per 60 s at `slowed`, and never sooner than the latest `x-poll-interval` GitHub
sent for any of the repository's reads.
Registrations stay outside this queue, and their responses are not read for the
budget.

**Next pages.** For every paged read, each address in a `link` header's `next` is
compared with the read's first request. It is followed only when it has that request's
origin and its path is either that request's path, or `/repositories/<id>` followed by
exactly the rest of that path after `/repos/<owner>/<repo>`, or, for the runner list
of an organisation, `/organizations/<id>/actions/runners`, where `<id>` is one or
more ASCII digits and the same on every page of the read, and only its query differs.
GitHub names next pages in those id forms. A repository read never follows the
`/organizations/` form, and an organisation read never follows the `/repositories/`
form. Any other address reads as `error` and is not requested, so no response can
send the token to another origin or another endpoint path. The agent does not check
that `<id>` is the configured repository's or organisation's, which would cost
another request; the address comes only from GitHub's response over TLS, with no
redirect followed.

The requests use the registration client: `https://api.github.com` only in a release
build, no redirect, 10 s to connect, 30 s a request, 1 MiB a response, the token in a
sensitive header and scrubbed error text. GitHub suggests following 301, 302 and 307;
the agent follows none, so a redirect cannot carry the token elsewhere.

**The budget** is read from the `x-ratelimit-limit`, `x-ratelimit-remaining` and
`x-ratelimit-reset` headers of each display response. Within one reset window the
lowest remaining count is kept; a response with a later reset, received after the
current window's reset has passed, starts a new window at `normal`. The level is
`normal` above 25% left, `slowed` at or below 25%, and `paused` at or below 10% or
when GitHub says the limit is reached; it only moves down within a window, and a pause
sends no display request until the reset.

**What each answer means:**

| Answer                                           | Runner list                                                                   | Lookup                                               |
| ------------------------------------------------ | ----------------------------------------------------------------------------- | ---------------------------------------------------- |
| `200`                                            | Read                                                                          | Read; `job_names` becomes `shown`                    |
| `304`                                            | Keep that page                                                                | Keep that page                                       |
| 403 or 429 with `x-ratelimit-remaining: 0`       | Pause until the reset, or, with no reset known, wait as for a secondary limit | The same                                             |
| 403 or 429 with GitHub's secondary-limit message | Wait `retry-after`, else 60 s doubling to 15 minutes                          | The same                                             |
| Another 403 or 429 with `retry-after`            | Wait `retry-after`                                                            | Wait `retry-after`                                   |
| `401`                                            | `token_rejected`                                                              | `job_names` `unreadable`, `token_rejected`           |
| Any other 403                                    | `token_rejected`                                                              | `job_names` `hidden`: the token has no Actions: read |
| `404`                                            | `not_found`                                                                   | `job_names` `unreadable`, `not_found`                |
| `3xx`                                            | `moved`                                                                       | `job_names` `unreadable`, `moved`                    |
| `5xx`, anything else, or a next page refused     | `error`                                                                       | `job_names` `unreadable`, `error`                    |
| No connection or a timeout                       | `unreachable`                                                                 | `job_names` `unreadable`, `unreachable`              |
| The store cannot give the token                  | `token_unreadable`                                                            | No request                                           |

A rate-limit wait holds every display request on the host. A `retry-after` or
`x-poll-interval` counts for at most an hour, and a reset is never taken as more than
an hour away, which is the rate limit's own window. A repository whose runner
list fails keeps its last runners and `checked_at`, and is tried again after 15 s,
doubling to 5 minutes, never sooner than the level allows, with `retry_at`. A failed
lookup leaves the runner list alone: job names become `unreadable`, and lookups wait
15 s, doubling to 5 minutes, never less than the level's interval, with
`job_names_retry_at`; `unreadable` stays until a later lookup is answered. While job
names are hidden the agent sends one in-progress-runs request again when one of the
repository's runners becomes busy, at most once a minute: a runner that becomes busy
within the minute gets it at the first check after the minute, so a token that gains
Actions: read shows the next job's name without a restart.

**The `github` value:**

| Field                                                 | Meaning                                                                                                                                                                                   |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `budget`                                              | `null` until a display request is answered, then `limit`, `remaining`, `resets_at`, `seen_at` and `level`                                                                                 |
| `repositories`                                        | Each repository with a registered runner and a stored token, by its key, with what the agent last learned of it                                                                           |
| `repositories[]`                                      | `repository`, `checked_at`, `status`, `retry_at`, `job_names`, `job_names_cause`, `job_names_retry_at`, `runners`                                                                         |
| `repositories[].runners`                              | This host's runners GitHub lists: `name` (the snapshot's `workload`), `github` (`offline`, `idle` or `busy`) and `job`                                                                    |
| `repositories[].runners[].job`                        | `workflow`, `name`, `run_id` and `job_id`, once a busy runner's job is known                                                                                                              |
| `organizations`                                       | One entry per configured organisation group, as the text above describes: `organization`, `runner_group`, `checked_at`, `status` and `runners`, each runner with `name` and `github` only |
| `scale_sets`                                          | One entry per on-demand pool, independent of the repository display scope                                                                                                                 |
| `scale_sets[]`                                        | `pool`, `repository`, `name`, `assigned`, `running`, `registered_idle`                                                                                                                    |
| `scale_sets[].pool`, `repository`, `name`             | Logical pool name, repository key and configured scale-set name                                                                                                                           |
| `scale_sets[].assigned`, `running`, `registered_idle` | GitHub's assigned-job, running-job and registered-idle-runner counts from the existing session statistics, not local slot counts; null before statistics are available                    |
| `omitted_runners`                                     | Runner entries left out from the end to keep the value within 64 KiB less the envelope                                                                                                    |
| `omitted_scale_sets`                                  | Scale-set entries left out from the end after all runner entries, before any organisation group row, when needed to keep the same message bound                                           |
| `omitted_organizations`                               | Organisation group rows left out from the end after all runner and scale-set entries, when needed to keep the same message bound                                                          |

Times are milliseconds since the Unix epoch. `status` is `ok` or the cause in the table
above; `job_names` is `unknown` (no in-progress-runs request answered yet), `shown`,
`hidden` or `unreadable`, with `job_names_cause` in the words of `status`. `level`,
`status`, `job_names` and `github` are open sets: a reader treats a word it does not
know as unknown. A runner is not listed when its repository's `checked_at` is later
than the runner's `since` in the snapshot and no entry names it; until then it is
still being checked. The workflow and job names come from GitHub and are untrusted:
the agent removes control characters and Unicode bidirectional formatting characters
and cuts each to 120 code points with an ellipsis. The value never carries a token, a
URL from GitHub or a response body. A job's page is
`https://github.com/<owner>/<repo>/actions/runs/<run_id>/job/<job_id>`, built by the
reader from its own configured repository.

The agent logs each display request's method, path, status code and requests left,
and for a 403 or 404 from a lookup also GitHub's message, cleaned as names are and cut
to 200 characters, and its `X-Accepted-GitHub-Permissions` header. It logs each change of
a runner's GitHub state, of a repository's job names and of the level, each failure
and pause with when it tries again, and never a token.
