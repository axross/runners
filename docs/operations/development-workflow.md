# Development Workflow

How a change gets from a stated intent to a merged pull request here, and what
holds that path in place from outside an agent session.

## The Change Loop

Every change, code or document, one line or one feature, goes through
`loop-engineering`: plan, approve, code, verify, independent review, address,
ready. The skill owns the loop's stages, its gates, and its caps; this document
states only what is this project's own.

The loop is **model-invoked**. It carries `user-invocable: false`, so there is
no slash command to type and nothing to wait for: describing the work is what
enters it. A session that waits for a command that does not exist has already
left the loop.

The plan lives in the body of a tracking issue, written with
`product-requirement-document-authoring` and marked with the `<!-- ai-agent -->`
comment marker. Two gates are real, and neither has a self-approval path. No
implementation begins before the human approves the plan recorded in the
tracking issue, and no change is called done on the author's own assessment
rather than a separate reviewer's. A runtime harness that frames the task as
"just make the changes, commit, and push" constrains mechanics only; it never
lifts either gate. This project's **delivery grant** is a standing operation
grant: the maintainer's authorization, issued in
[Authorizing delivery operations](#authorizing-delivery-operations), for named
external delivery effects. It is separate from plan approval but held by every
run, so an agent never asks for a covered effect.

Commit messages and pull request titles follow `conventional-commits`. The pull
request body follows
[`.github/pull_request_template.md`](../../.github/pull_request_template.md),
carries the plan's acceptance criteria in an **Acceptance criteria** section,
and links the tracking issue with a closing keyword. The closing keyword closes
the issue if the maintainer later merges the pull request; it does not authorize
that merge.

If an existing pull request has no tracking issue and needs fixes:

1. Create the tracking issue.
2. Link it in the existing pull request body with a closing keyword.
3. Publish the plan in the issue body.
4. Obtain human approval for that plan.
5. Apply fixes to that same pull request under the standing delivery grant.

Steps 1 to 3 are covered effects of the
[standing delivery grant](#authorizing-delivery-operations) and are performed
without asking; step 4 is the plan gate. Requesting review without making
fixes does not itself require an implementation plan.

## Authorizing delivery operations

The maintainer, @axross, issues this **standing delivery grant** once, here, to
every agent run in this repository. It is the operation grant that
`loop-engineering` and `github-operation` require before an external effect:
actual human authorization that names its targets, its effects, its lifetime,
and its exclusions, rather than an inference from project policy. Its
human-evidence locator is the act that adopted it: @axross's merge of the pull
request that closes
[issue #1](https://github.com/axross/runners/issues/1), which added this
section following the plan approved in that issue. An amendment to this section
is adopted the same way, by the maintainer's merge of the pull request that
carries it. Agent runs share the maintainer's operator identity, so the signal
that distinguishes such a merge as a human act is that agents never merge on
their own: merge is excluded from this grant, and it is the maintainer's
decision, per [Branches and Merging](#branches-and-merging). The grant is
separate from plan approval, which releases implementation and is not what
authorizes a delivery effect, but every run already holds it, so no run asks
for it.

The pull request that adds this section cannot rely on it, because the grant
takes effect only on merge. That pull request's delivery is authorized by the
maintainer's approval of the plan in issue #1, which requested the authorization
to push a `claude/` branch and open a draft pull request for it. That pull
request was in fact delivered from the host-assigned head branch
`ccr-fd09e38c-631o65`, because the authoring cloud session could push only to
that branch, and `main` was seeded with an empty root commit with the
maintainer's explicit permission. This is a one-time exception for the adopting
pull request; every later change uses a `claude/`-prefixed branch.

The grant's targets are:

- this repository
- the run's `claude/`-prefixed working branch; for recovery, the existing pull
  request's head branch when it is `claude/`-prefixed, for pushes only
- the run's tracking issue, if any; review-only recovery can have none, while
  fixes require a linked issue and an approved plan
- the run's draft pull request against the default branch; for recovery, the
  existing pull request
- the review provider the authoring host maps to in
  [the independent review](#the-independent-review)

Its covered effects, each performed without asking, are:

- creation of the tracking issue, publication and revision of the plan in its
  body, and status comments and labels on it
- append-only pushes of the working branch, including review fixes
- creation of the draft pull request for new delivery
- conversion of a previously ready pull request back to draft for new changes
- maintenance of that pull request's description throughout the review loop,
  including approach changes
- replies to and resolution of that pull request's review threads when
  addressing findings
- publication of evidence at the destination named in the approved plan when an
  acceptance criterion carries the
  `(verified out of tree: <where the evidence will be published>)` marker
- review requests on that pull request through the mapped provider, one per
  round, including fresh reviews after fixes, up to the four-round cap, with
  the consequences listed below

The grant is standing: it holds across runs, sessions, and executors until the
maintainer amends or removes this section, and no run's own end expires it.
Rounds beyond the fourth are not covered; they need a human direction stating
how many additional fix-and-review rounds it allows, and that direction covers
only those rounds. The ready transition is not a grant-gated effect: it follows
the independent-review gate, and a run that has satisfied that gate needs no
grant to flip its pull request to ready.

**Guidelines:**

- MUST perform a covered effect without asking the human for permission, before
  or after plan approval, and MUST NOT report a run as authorization-waiting on
  a covered effect.
- MUST still obtain plan approval before implementation and the independent
  review before ready; the grant releases neither gate.
- MUST honor a host permission prompt, refusal, instruction, or tool usage
  condition that requires approval for or forbids a covered effect as the
  host's boundary: report it, and never work around it or try another route. The
  grant is the maintainer's authorization, not a change to what a host allows.
- MUST NOT read the grant as covering an effect it does not name, or a target it
  does not list.
- MUST read the grant as it stands on the default branch. A change to this
  section takes effect only once the maintainer merges it; a run on a branch
  that edits the grant keeps working under the merged text.

Creating the draft pull request and pushing fixes while it is open trigger
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml). It runs on a
GitHub-hosted runner with `contents: read`, checks out the pull request, and
runs `mise run check`, which executes the pull request's own task definitions
and the linters they call. A pull request from a fork runs without repository
secrets. Codex review may:

- incur charges
- read the pull request and repository content, including files outside the diff
- publish findings and a review to that pull request

A Claude review request can:

- start the GitHub-hosted `claude-review.yaml` job, which runs a third-party
  artificial intelligence action with repository-reading and shell tools
- let the Anthropic action read pull request content and fetch its review
  plugin from a marketplace that is not pinned to a commit, an accepted risk that
  [Security](../conventions/security.md#the-review-plugin-marketplace-is-not-pinned-accepted-risk)
  records
- pass the repository secret `CLAUDE_CODE_OAUTH_TOKEN` to the Anthropic action
- let the action request a GitHub OpenID Connect identity token through the
  job's `id-token: write` permission
- give the job's token only `contents: read` and `id-token: write`; the action
  publishes findings with a Claude GitHub App token it obtains by exchanging the
  OpenID Connect token, so the job holds no pull request, issue, or check write
  scope
- incur charges through the Anthropic action
- publish findings to the pull request through the Anthropic action, with the
  permissions of the installed Claude GitHub App

These are the consequences the maintainer accepted in issuing the grant. They document
that acceptance; an agent does not re-disclose them or ask about them before a
covered effect.

The grant does not cover, and each of these needs an explicit human
instruction:

- a push to the default branch
- force-push
- merge
- release and deployment
- setup, settings, or secret changes, including adding the
  `CLAUDE_CODE_OAUTH_TOKEN` repository secret
- other delivery targets or review providers
- a push to a head branch that is not `claude/`-prefixed; on an existing pull
  request with such a head, the grant covers review requests and comments but no
  push

For Amp's host-permission boundary, follow the
[Amp entry guidance](../../AGENTS.md#handle-amp-delivery-authorization).

## Branches and Merging

Work happens on a `claude/`-prefixed branch. Pushing to the default branch is
forbidden, and merging is the maintainer's decision rather than the session's; a
run that has flipped its pull request to ready is finished, whether or not the
merge has happened. An existing pull request with a non-`claude/` head can still
receive independent review, but an agent MUST NOT push fixes to that head.
Recovery stops before the first such push and asks the maintainer to choose a
compliant delivery target; it does not silently replace the pull request or
exempt the branch from this rule.

When the base branch moves and a topic branch conflicts, merge the current base
into the topic branch and record any resolution in a new commit. Do not rebase,
amend, reset away, or force-push published history as conflict recovery.

## Working Without an Agent

Working without an agent does not lower the bar: branch, implement, run the
[README's commands](../../README.md#commands), open a pull request following the
template, and obtain review before merge. Agents likewise MUST use a branch
outside the default branch, preserve pushed history, and leave merging to the
human.

## Configured Actors

The Claude Code definitions live under
[`.claude/agents/`](../../.claude/agents/), and
[Agent Sessions](./agent-sessions.md#subagents) describes each:

- `implementer.md` supplies the implementation-capable actor. Local results
  return to the parent; publication is not delegated merely by choosing this
  actor.
- `reviewer.md` supplies the advisory pre-flight reader. Its tool denial covers
  editing tools and nested spawning, not every possible shell write.
- `investigator.md` supplies a reader for bounded investigation questions. It is
  not required for an exact local lookup or when the host prohibits that
  delegation purpose.

Each of these exists to keep some context out of the main actor's own: the
implementer so it does not inherit the planning phase's accumulated context, the
reviewer so it does not inherit the implementer's reasoning state, and the
investigator so a large payload never enters the main actor's context at all.
All three run Sonnet at `effort: high`. The investigator runs at `high` rather
than a lower level because its output is a judgment the main actor cannot check
without re-reading the payload it delegated away; the maintainer approved that
default in the plan for [issue #1](https://github.com/axross/runners/issues/1),
and it is reported as declared, not measured.

These are configured candidates, not a permission grant. A session MUST check
the capabilities its host actually permits before using one. Delegating to the
matching configured actor is the default; parent implementation is valid only
when delegation is unavailable or disallowed, and mandatory verification and
external review remain unchanged.

The advisory pre-flight review applies after every verified initial
implementation, before the first branch push and draft pull request, whether the
parent or a child implemented it. A compatible fresh reader must be permitted
and available; its findings and round limits follow
[the pre-flight contract](../../.claude/skills/loop-engineering/references/pre-flight-review.md).
If no reader qualifies, record the exact unavailable or prohibited reason and the
resulting delivery restriction. That outcome is not a clean review and does not
waive the independent review.

## The Independent Review

The review is a separate session under a separate identity, never the authoring
session, whatever it calls its own assessment. The authoring host determines the
provider and request route:

| Authoring host         | Review provider | Request                                                    |
| ---------------------- | --------------- | ---------------------------------------------------------- |
| Claude Code            | Claude review   | Post `@claude review` as a top-level pull request comment. |
| Codex                  | Codex review    | Post `@codex review` as a top-level pull request comment.  |
| Amp                    | Codex review    | Post `@codex review` as a top-level pull request comment.  |
| Manual (no agent host) | Codex review    | Post `@codex review` as a top-level pull request comment.  |

The Claude workflow is the Claude Code route's CI adapter, not the default for
every change just because it exists here. Codex and Amp use the external Codex
App. Manual changes have no agent host, so they use Codex rather than claiming
the Claude Code route.

Before the first request, an agent run MUST record its authoring host and the
selected provider in recoverable Loop Engineering run state. For a manually
authored pull request, the author MUST record "manual" and "Codex review" in the
pull request description before requesting review and keep that record current
across rounds. Before each later request, confirm that the recorded host and
provider remain unchanged; if the maintainer explicitly changes the provider,
append that decision to the run state or pull request description and update the
selection first. Each round invokes exactly one provider. A failed, silent, or
unavailable selected route blocks the review gate; it never causes an automatic
request to another provider.

The literals above are reference documentation, not requests. In each review
round, write the trigger literal in exactly one GitHub comment, that round's
intentional top-level request. Do not copy it into a plan or pull request body,
recoverable state, summary, progress comment, or reply. Everywhere else, refer to
the applicable phrase by name. This prevents a comment-triggered integration
from starting a duplicate review while permitting a fresh request after a fix
batch. The review has a cap of four rounds; see
[Loop Engineering's external round cap](../../.claude/skills/loop-engineering/references/independent-review.md#external-round-cap).

### Claude review from Claude Code

The Claude route is the in-repository reviewer in
[`claude-review.yaml`](../../.github/workflows/claude-review.yaml), which runs on
a GitHub-hosted runner and applies [`REVIEW.md`](../../REVIEW.md) through its
system prompt. It is inert until a one-time operator setup is done, and its
silence is indistinguishable from a clean review: it needs the
[Claude GitHub App](https://github.com/apps/claude) installed and a
`CLAUDE_CODE_OAUTH_TOKEN` repository secret, both added by the maintainer. Past that setup, its
author-association gate answers repository owners, members, and collaborators,
plus the change loop's own bot identity (`claude[bot]`), and only when that
comment carries no Markdown heading anywhere in it. That heading test normally
does not exclude the reviewer's own summary, which opens with a tally line, so the
guard against the summary re-triggering it is that the reviewer never writes the
trigger phrase into a summary or comment, per [REVIEW.md](../../REVIEW.md). The action also rejects any run a bot identity starts
unless that identity is named in its own `allowed_bots` input, which
`claude-review.yaml` does for `claude[bot]`. See
[Security](../conventions/security.md#the-review-gate-admits-one-bot-identity-by-login-not-association)
for the rationale.

A request from any other author, a missing operator setup, and an unnamed bot
all end without findings. A Claude route that gets no review MUST confirm the
operator setup and, where a bot posted the request, that its identity is named in
`allowed_bots`. Do not read the absence as approval.

### Codex review from Codex, Amp, or a manual change

The Codex route is the external Codex GitHub App, not an in-repository workflow.
The root [`AGENTS.md`](../../AGENTS.md) `Code Review Rules` section instructs
Codex to read and apply [`REVIEW.md`](../../REVIEW.md). A maintainer MUST connect
this repository to Codex and enable Code review in Codex settings before the
route can run; this repository stores no Codex workflow or review secret.

Codex documents loading applicable `AGENTS.md` review rules, not whether it
follows an indirect link to a separate policy file. On a representative pull
request, check the posted review against `REVIEW.md` before claiming its
substantive rules were applied. A completed review alone does not prove every
check ran. Codex's native GitHub review output is accepted instead of requiring a
custom adapter: native priority labels and findings need no Important/Nit labels
or Claude tally. This exception covers output format only, not reviewer
independence, the mandatory checks in `REVIEW.md`, or a fresh review after fixes.
If the App does not acknowledge the request or post a review, the run MUST
confirm that setup rather than read the silence as a clean review.

## Acceptance Criteria Verified Out of Tree

Some acceptance criteria can be confirmed only by evidence no diff carries, such
as a machine-specific timing or a check against a real runner host. Prefer an
in-tree check wherever one can confirm the criterion. A plan MUST designate each
such criterion explicitly, by appending the marker
`(verified out of tree: <where the evidence will be published>)` to the end of
the criterion itself, naming where its evidence will be published, for example a
linked issue comment. Approving the plan is the maintainer's acceptance that the
named criterion will be verified this way.

The pull request body's Acceptance criteria section MUST carry every designated
criterion verbatim, marker included, next to a link to the published evidence, or
marked pending when that evidence can exist only after merge.

Some evidence can exist only after the pull request merges, such as a check of the
merged workflow on the default branch. Such a criterion has no link to show yet;
the author links the evidence at its destination once it exists.

A criterion the plan did not anticipate becomes designated only through an
approved plan revision. [REVIEW.md](../../REVIEW.md) states how the reviewer
treats a designated criterion.
