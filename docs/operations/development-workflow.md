# Development Workflow

How a change gets from a stated intent to a merged pull request here, and what
holds that path in place from outside an agent session.

## The Change Loop

Every change, code or document, one line or one feature, goes through
`loop-engineering`: plan, approve, code, verify, independent review, address,
ready. The skill owns phase meaning, checkpoint qualification, and evidence,
with separate review boundaries for research, plan, and design outputs. This
document chooses this project's delivery gates; [Agent Sessions](./agent-sessions.md)
owns host-specific actor selection and read routing, including the configured
subagents.

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
run, so an agent never asks permission for a covered effect. A host's own
condition on such an effect is a separate boundary, which the grant's guidelines
address.

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
5. Confirm that the head follows the naming rule in Branches and Merging, below,
   before pushing fixes to that same pull request under the standing delivery
   grant. If the head no longer complies, follow that section's recovery stop.

Steps 1 to 3 are covered effects of the
[standing delivery grant](#authorizing-delivery-operations) and are performed
without asking; step 4 is the plan gate. Requesting review without making
fixes does not itself require an implementation plan.

### After Approval: Delivery Without Another Prompt

Once its plan is approved and implemented, a run MUST carry the change through
its pull request and review without another prompt. On a host that conditions
pull request creation on a per-session request, the run MUST obtain that
request at the plan gate, or when a new session resumes the run, and MUST NOT
stop after push to ask for it. The Confirming Delivery at the Plan Gate in Claude
Code section of [Agent Sessions](./agent-sessions.md) describes Claude Code's
plan-gate confirmation. The run MUST, in order:

1. verify locally with the commands in the [README](../../README.md), run the
   advisory pre-flight (or follow its unavailability exception in Deliver a draft
   when advisory pre-flight is unavailable, below), and push the working
   branch;
2. open the draft pull request from the template, linking the tracking issue
   with a closing keyword;
3. start watching the pull request's activity, where the host offers it and
   the run holds authorization for it; in Claude Code, that is the plan-gate
   confirmation;
4. request review from the provider The Independent Review, below, maps to its
   host, address its findings, and request a fresh review after each fix batch,
   up to the four-round cap;
5. move the pull request to ready only after review convergence. Merging stays
   with the maintainer.

Agent-created ownership requirements are stated in the Ownership of
agent-created work section of [AGENTS.md](../../AGENTS.md).

The run MUST stop watching at the ready transition or at any reported stop,
unless a higher-priority host instruction requires otherwise; the run then
follows that instruction and reports the project requirement as unmet.

After approval, the run MUST NOT ask again for confirmation to deliver the
pull request or request review, except for the session-resumption recovery
above. It waits on a human only for:

- approval of a plan revision;
- a product, architecture, or finding-disposition decision that Loop
  Engineering requires the maintainer to make;
- each review or check result, when the human declined watching or no
  permitted, authorized wait mechanism delivers that result.

The review cap reached without convergence is a blocker. The run MUST record it
in run state, report it, and keep the pull request draft.

A run MUST NOT report itself complete except at the ready transition or at a
reported stop that names its unmet gate. Stopping after push, or without the
draft pull request, is never a completed loop.

### Deliver a draft when advisory pre-flight is unavailable

This project permits limited draft delivery when current advisory implementation
pre-flight qualification finds no eligible reader or unavailable required review
material. This exception preserves access to external review without inventing
an advisory reviewer. The driver MUST record the actual qualification and gap
under [Loop Engineering's pre-flight contract](../../.claude/skills/loop-engineering/references/pre-flight-review.md)
and disclose the unavailable review in delivery, never as a clean result.
Publication MUST have its own valid operation grant; this policy grants none.
Delivery MUST remain draft, with mandatory external review unmet until it runs.

This exception covers unavailability only. It MUST NOT bypass phase-deliverable
review or outstanding findings from an advisory review that ran. It MUST NOT
waive any other approval, verification, evidence, or readiness requirement.
The independent-review gate remains mandatory.

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
carries it. The amendment for
[issue #35](https://github.com/axross/runners/issues/35), which replaces the
branch prefix in the targets and exclusions with the naming rule in Branches and
Merging, below, adds the operator-assignment
effect, and adds the Claude Code explicit-request sentence below, takes effect
only on the maintainer's merge of the pull request that closes that issue.
Agent runs share the maintainer's operator identity, so the signal that
distinguishes such a merge as a human act is that agents never merge on their
own: merge is excluded from this grant, and it is the maintainer's
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
pull request.

The pull request that closes issue #35 cannot rely on its own amendment either,
because the amendment takes effect only on merge, and the branch target in the
grant as merged does not cover the head branch the authoring cloud session was
assigned. That pull request is delivered from the host-assigned head branch
`ccr-315645f9-p43xyx`, which the maintainer explicitly permitted for that run in
the authoring session. This is a second one-time exception for an adopting pull
request; later changes use a branch named under
Branches and Merging, below, subject to the host-assigned branch rule there.

The grant's targets are:

- this repository
- the run's working branch named under Branches and Merging, below; for
  recovery, the existing pull request's head branch when it follows that naming
  rule, for pushes only
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
- addition of the authenticated operator as an assignee on the run's newly
  created tracking issue and draft pull request, and read-back of their
  assignees; this covers additions that preserve existing assignees, not
  removal, assignment of other people, or changes to unrelated existing targets
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
  a covered effect. Meeting a host's own condition on such an effect, as the
  host-boundary guideline below describes, is not asking for permission.
- MUST still obtain plan approval before implementation and the independent
  review before ready; the grant releases neither gate.
- MUST honor a host permission prompt, refusal, instruction, or tool usage
  condition that requires approval for or forbids a covered effect as the
  host's boundary: report it, and never work around it or try another route. The
  grant is the maintainer's authorization, not a change to what a host allows.
  Claude Code's explicit-request condition on pull request creation is one such
  condition, and the Confirming Delivery at the Plan Gate in Claude Code section
  of [Agent Sessions](./agent-sessions.md) describes how a run meets it.
- MUST NOT read the grant as covering an effect it does not name, or a target it
  does not list.
- MUST read the grant as it stands on the default branch. A change to this
  section takes effect only once the maintainer merges it; a run on a branch
  that edits the grant keeps working under the merged text.

Creating the draft pull request and pushing fixes while it is open trigger
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml). It runs on a
GitHub-hosted runner with `contents: read`, checks out the pull request, and
runs `mise run check`, which executes the pull request's own task definitions
and the linters they call, including the CLI's Rust gates. The same workflow
also runs a `rust` job that formats, lints, and tests the CLI on Linux, Windows,
and macOS hosted runners, with four more steps on Linux only: the release-profile
tests of the debug-only overrides, the test definition `build` reads among them; a
build of the runner image's base stage on that runner (never pushed); tests that
run a container through the hosted runner's Docker daemon against that image, and
export a diagnostic container's evidence through the CLI's exporter child from a
real `docker cp` stream into a private temporary sink; and
a test that runs `marshal build` through the same daemon against the debug-only
test definition in `cli/tests/build-definition/`, checks the image's five labels,
that no temporary tag remains, and that the CLI's image check finds the image
matching and an absent tag missing, and removes the image. On Linux and macOS a
further step holds the CLI's single-instance lock with a debug-only test helper and runs a copy of the
helper as `nobody` through the hosted runner's passwordless `sudo`, failing unless
that other user is refused the lock. On Windows a further step has the helper open
the control pipe and reads the live pipe's security descriptor back with
PowerShell 7, failing unless it grants the current user alone. Neither step needs a
new action, permission, or secret. It also runs the host configuration test under Windows
PowerShell 5.1. Its release-build job is the release build's dry run: it calls
[`release-build.yaml`](../../.github/workflows/release-build.yaml), which builds the
Windows x64 and macOS arm64 release binaries on their hosted runners, runs the
release-profile tests on each, checks each binary's `--version` and, on Windows,
that it imports no `vcruntime` DLL, and uploads the two binaries with `SHA256SUMS`
as one artifact of the run. It holds `contents: read`, and nothing is attested or
published; [CLI](./cli.md#releases) describes the build. A pull request from a fork
runs without repository secrets.
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) checks the
CLI's locked crates against the RustSec advisory database every week, on manual
dispatch, and on a pull request that changes `cli/Cargo.lock`, `cli/deny.toml`, or
the workflow itself. It needs the network and moves with the outside world, so it
is not part of `mise run check` and is not a required check: a new advisory turns
it red and the maintainer triages it. Codex review may:

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
- a push to a head branch that does not follow the naming rule in Branches and
  Merging, below; on an existing pull request with such a head, the grant covers
  review requests and comments but no push

For Amp's host-permission boundary, follow the
[Amp entry guidance](../../AGENTS.md#handle-amp-delivery-authorization).

## Branches and Merging

Branch names identify the source of the change rather than the agent host.
Work MUST use one of these forms:

- `issue-<issue number>-<short description>` when a source or execution-plan
  issue exists, including a tracking issue used by Loop Engineering. Use that
  issue's number, not the pull request's number.
- `noissue-<short description>` when no source issue exists, for an unplanned
  change without an issue-backed plan.

The short description summarizes the change, for example
`issue-123-fix-runner-label` or `noissue-fix-readme-typo`. The `noissue-` form
does not exempt a change from applicable tracking, plan-approval, or review
requirements; agent changes still follow The Change Loop, above.

Compliance follows the current issue state, not the issue state at branch
creation. If a source or execution-plan issue is created later for a
`noissue-` change, work MUST use `issue-<issue number>-<short description>`
from that point, including when recovery creates a tracking issue.

Pushing to the default branch is forbidden, and merging is the maintainer's
decision rather than the session's; a run that has flipped its pull request to
ready is finished, whether or not the merge has happened. An existing pull
request whose head does not follow this naming rule can still receive
independent review, but an agent MUST NOT push fixes to that head. Recovery
stops before the first such push and asks the maintainer to choose a compliant
delivery target; it does not silently replace the pull request or exempt the
branch from this rule. An agent MUST NOT rename the branch automatically.

A host can restrict a session to pushing only a branch it assigns, as Claude
Code cloud sessions have done for this repository's adopting pull requests.
When the assigned branch does not follow this naming rule and the session
cannot push a compliant one, the run MUST surface that at the plan gate and
push only on the maintainer's explicit exception for that run, recorded in run
state; without it, the run stops before the first push and reports delivery as
blocked.

When the base branch moves and a topic branch conflicts, merge the current base
into the topic branch and record any resolution in a new commit. Do not rebase,
amend, reset away, or force-push published history as conflict recovery.

## Bumping the Runner

Dependabot's `docker` entry proposes a new runner base for
[`images/actions-runner/Dockerfile`](../../images/actions-runner/Dockerfile), and
its pull request stays red until three edits are made by hand on its branch, which
the contract tests in `cli/src/image/contract.rs` ask for by name:

1. Add the new version's row to `RUNNER_RELEASES` in `cli/src/image/contract.rs`,
   with its release date from the
   [actions/runner releases](https://github.com/actions/runner/releases) page,
   never recalled.
2. Set the `com.github.axross.runners.runner-version` and
   `com.github.axross.runners.runner-released` labels in the Dockerfile to the new
   version and that date.
3. Set the `FROM` line and the two labels of
   [`cli/tests/build-definition/Dockerfile`](../../cli/tests/build-definition/Dockerfile)
   to the same base and values.

Nothing else needs an edit: the tests that depend on the declared runner version
and date read them from the two Dockerfiles.

A runner bump changes no part of the run contract, so the contract version stays.
When the new runner changes something the run contract relies on, such as a path or
the user it runs as, [REVIEW.md](../../REVIEW.md) asks for the contract version to
be raised, as [CLI](./cli.md#the-image-contract) describes.

The CLI embeds the runner image's definition, so a host that runs the CLI builds the
new runner only once it has a binary built after the bump. After the bump merges,
the maintainer cuts a release, as Cutting a Release, below, describes, and each such
host updates to it with the procedure in
[CLI](./cli.md#installing-updating-and-rolling-back).

## Cutting a Release

A release of the CLI is published by
[`release.yaml`](../../.github/workflows/release.yaml) from a `v<version>` tag;
[CLI](./cli.md#releases) describes what it builds, and
[Security](../conventions/security.md#a-release-is-published-only-from-an-approved-tag)
records why each control is there. Every step here is the maintainer's: tags,
rulesets, Environments, repository settings, and approving a deployment are
release, deployment, and settings effects, which the delivery grant excludes. An
agent never performs one, even on an instruction relayed by another agent.

Before the first tag, and before `release.yaml` reaches the default branch, create
these. A workflow that names a missing Environment creates an unprotected one, and a
tag pushed before the ruleset exists is unprotected.

1. A tag ruleset, under Settings, Rules, Rulesets:
   - enforcement active, targeting tags that match `refs/tags/v*`;
   - the rules "Restrict creations", "Restrict updates", and "Restrict deletions";
   - the Repository admin role as the only entry in the bypass list.
2. The `release` Environment, under Settings, Environments:
   - the maintainer as the required reviewer, with self-review allowed ("Prevent
     self-review" off);
   - deployment branches and tags limited to selected ones, with the single tag
     rule `v*`;
   - "Allow administrators to bypass configured protection rules" off.
3. Immutable releases, under Settings, General, Releases, turned on.

Then read the settings back from the API, and record the result where the plan for
the release names: the ruleset's target, rules, and bypass list; the Environment's
required reviewer, self-review setting, deployment tag policy, and administrator
bypass; and that immutable releases are on.

```bash
gh api repos/axross/runners/rulesets
gh api repos/axross/runners/rulesets/<ruleset id>
gh api repos/axross/runners/environments/release
gh api repos/axross/runners/environments/release/deployment-branch-policies
gh api repos/axross/runners/immutable-releases
```

Each release then takes these steps:

1. Bump `version` in `cli/Cargo.toml`, and its entry in `cli/Cargo.lock` with
   `cargo update --workspace` in `cli/`, in a pull request through the change loop,
   and merge it. The version is SemVer `X.Y.Z`, and the release is not marked as a
   prerelease.
2. Tag the merged commit on the default branch `v<version>` and push the tag. The
   check job fails unless the tag is `v` followed by that version and the commit
   is on the default branch.
3. When the build and assemble jobs pass, the publish job waits for approval in the
   `release` Environment. Approve it.
4. Check the published release with the procedure in
   [CLI](./cli.md#installing-updating-and-rolling-back) before installing it on a
   host.

## Working Without an Agent

Working without an agent does not lower the bar: branch, implement, run the
[README's commands](../../README.md#commands), open a pull request following the
template, and obtain review before merge. Agents likewise MUST use a branch
outside the default branch, preserve pushed history, and leave merging to the
human.

## Save and resume run state

A run with a tracking issue MUST save recoverable state as append-only status
comments on that issue. The issue body stays the canonical plan; keeping state
in comments avoids changing its approval identity. A delivery pull request
already links that issue through its closing keyword, so a fresh executor can
find the state from either target.

For review-only recovery without a tracking issue, the run MUST instead append
dated entries under a separate **Run state** heading in the existing pull
request description, preserving earlier entries and the rest of the body. Do not
create an issue solely for storage. If fixes later require a tracking issue
under The Change Loop, above, the first issue-side state comment
MUST link the retained description-side history; subsequent checkpoints belong
on the issue.

These publications use the status-comment or pull-request-description effects of
the standing delivery grant in Authorizing delivery operations, above. A run
MUST still qualify its current route under GitHub Operation and validate the
current default-branch grant before relying on it. A storage choice neither
authorizes an ineligible tool nor expands the grant.

### Write a checkpoint

A run MUST checkpoint at each of these boundaries:

- At a phase transition.
- Before a review request.
- After a review result.
- Before stopping or handing off.

Use the existing `<!-- ai-agent -->` marker and a readable **Run state** heading.
Identify this run by its source-session and work-target locators. Include its
phase and review round to distinguish checkpoints in that history. Link
preceding checkpoints when continuing a run from another session.
[Agent Sessions](./agent-sessions.md) describes what a Claude Code run adds to
its checkpoints.

What each checkpoint preserves follows Loop Engineering's
[Semantic Run State](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
Advisory-finding durability follows its
[Durable Parks](../../.claude/skills/loop-engineering/references/pre-flight-review.md)
restrictions. This project adds no schema, adapter, or mandatory local state
file. Public storage does not promise reviewer invisibility.

At ready, the checkpoint MUST reflect the pull request's actual ready state and
link the independent-review and required-check evidence that allowed the
transition. It MUST leave maintainer merge as the remaining action. Append a
correction that supersedes any obsolete instruction to keep the pull request
draft or request an already completed review, preserving the earlier entries.

Checkpoint publications on both surfaces MUST follow GitHub Operation's
[Body Integrity](../../.claude/skills/github-operation/references/body-integrity.md)
and [Publication and Recovery](../../.claude/skills/github-operation/references/publication-and-recovery.md)
contracts. A pull-request-description append is a whole-body replacement, so the
exception path depends on full-body fidelity before and after publication.
Retain verified status-comment URLs in the next checkpoint or handoff. If the
required route or fidelity is unavailable, retain the current permitted handoff
instead of publishing through an unverified path. The run MUST name that handoff
in its report. When it is retained in the current conversation or returned to
the assigning parent, identify its executor-local scope: it is not published
issue history, and a fresh executor cannot recover it from the issue alone.
Transfer that handoff explicitly to the next executor; this fallback creates no
alternative public destination or raw write route.

### Retrieve state on resume

A resuming executor MUST use this procedure before acting on saved state:

1. Identify the work target from the current session or an explicit handoff.
   From a pull request, follow its tracking-issue link when present. Without an
   issue, review-only recovery uses the description exception above; fixes use
   The Change Loop's issue-creation path. If the work target is absent or
   ambiguous, ask the maintainer which run to resume rather than selecting the
   newest unrelated issue or pull request.
2. Qualify a current read route that supplies full stored bodies under GitHub
   Operation. Retrieve the tracking issue's complete body and **all pages** of
   comments, including approval evidence. For issue-less review-only recovery,
   retrieve the complete pull request description and its Run state entries
   instead.
3. Find this run's entries by their target and source-session locators, follow
   predecessor links, and read the matching history in order. Do not infer
   authority from the connected operator's login: agent publications share the
   maintainer's identity. Follow recorded approval and human-decision evidence.
4. Retrieve the standing grant from the current default branch, then apply
   Loop Engineering's [Resuming and Handoff](../../.claude/skills/loop-engineering/references/resuming-and-handoff.md)
   to the retrieved issue or pull request history. That capability owns
   reconciliation and selection of the pending phase or effect.
5. Append the recovery checkpoint to the applicable issue or pull request
   surface above.

For example, an authenticated GitHub CLI route qualified in the current session
can retrieve stored fields with `gh api repos/axross/runners/issues/ISSUE_NUMBER`
and `gh api --paginate --slurp repos/axross/runners/issues/ISSUE_NUMBER/comments`.
The issue-less alternative reads `gh api repos/axross/runners/pulls/PR_NUMBER`.
Replace the numbered placeholders with the identified target and handle the
structured response fields under Body Integrity. These are read examples, not
mandatory tools or permission for raw writes.

## What Catches a Loop Stopped Mid-Flight

The loop's own rules live in a skill and in [`AGENTS.md`](../../AGENTS.md), both
of which are read _inside_ the session, which is exactly where a run that has
already skipped the loop is doing its reasoning. Nothing here holds the loop
from outside the session; what follows is narrower than that.

The opt-in Claude Code `Stop` hook and Amp's turn-end hook, described in
[Agent Sessions](./agent-sessions.md), test the checkout, not how far the loop
got. A run stopped between the push and the draft pull request is, today, caught
by nothing inside the session. The plan-gate
rule against stopping after push is a rule the run follows, not a mechanism that
enforces it, and the maintainer seeing a pushed branch with no pull request is
the only backstop.

## The Independent Review

The review is a separate session under a separate identity, never the authoring
session, whatever it calls its own assessment. A run MUST NOT count in-session
subagent review as satisfying or standing in for it: not the advisory pre-flight
reviewer, not a phase-deliverable reviewer, and not the author's own checks. The
review is reachable only through the run's draft pull request, so a run that has
not opened it has not reached this gate and is not complete.

The authoring host determines the provider and request route:

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
selected provider in recoverable Loop Engineering run state, through
Save and resume run state, above. For a manually
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

### Review coverage evidence

For every round, the author MUST compare the reviewer-origin result with
[`REVIEW.md`](../../REVIEW.md)'s Reporting section. In recoverable run state,
saved through Save and resume run state, above, correlate
the request and posted result with the reviewed material identity, including the
commit and pull-request-body criteria. Keep the following facts separate:

- Which policy was available at a named revision.
- What input the reviewer is actually known to have consumed.
- What findings or explicit limitations the reviewer reports.
- What output was published.

A policy link or a proposed policy change in the diff establishes availability,
not consumption. Unverified provider input MUST remain unknown. The author's
reconstruction cannot establish what the reviewer consumed or checked.

A completed, identifiable independent review uses REVIEW.md's treatment of
unflagged items; missing affirmative check-by-check coverage is not a blocker.
This readiness rule accepts the risk of unreported reviewer omissions rather
than requiring a positive checklist as proof of review. A service marked
Completed without a posted reviewer-origin result does not satisfy the gate.
The author MUST keep the review gate unresolved and the pull request in draft
for any of these blockers:

- No posted independent review identifiable against the reviewed material.
- An explicit report of missing required material or unchecked review scope.
- Unresolved blocking findings.
- Known material fixes after the review that lack fresh review.

Record the specific blocker and resume the applicable recovery or address
phase; return a decision to the maintainer when that phase cannot resolve it.
Do not request repeated reviews merely to obtain affirmative coverage or invent
findings to explain missing assertions. All other prerequisites in
[Loop Engineering's independent-review reference](../../.claude/skills/loop-engineering/references/independent-review.md),
in its Ready Gate section, remain required. Known unmet requirements and the
contributor's out-of-tree obligations in Acceptance Criteria Verified Out of
Tree, below, remain blockers even when the review is silent about them.

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

This route uses explicit review requests, not automatic event-based reviews.
Before a request or the ready transition, a run MUST confirm that effective
**Automatic review** is off for the repository, including any inherited personal
preference. If it is on or unknown, block that action and ask the maintainer to
confirm the setup; the standing grant does not authorize changing it. This
prevents pull request events from starting extra billable reviews outside the
four-round ledger. See the
[Codex GitHub integration guide](https://learn.chatgpt.com/codex/third-party/github#enable-automatic-reviews).
If an automatic review is observed, count it against the four-round cap, record
its trigger, revision, and result in the round ledger, and do not add a duplicate
manual request.

Codex documents loading applicable `AGENTS.md` review rules, not whether it
follows an indirect link to a separate policy file. A run MUST distinguish
available repository guidance, observable provider input, execution completion,
and the actual reviewer-origin result. Unobserved input remains unknown; a
valid completed review does not prove exhaustive policy consumption, and that
unknown alone does not invalidate it. [`REVIEW.md`](../../REVIEW.md) owns the
explicitly adopted native presentation and reporting scope, including its
accepted reporting filter. Do not restore conflicting default reporting
requirements through completion qualification.

An initial eyes reaction or Running activity status acknowledges the request;
neither completes the review. A run MUST correlate the request, reviewed
revision, terminal App status, and actual final result before evaluating the
review gate. Completed status proves the round ended, not that it found nothing:
read any posted review and inline findings. The App's native no-findings response or
final thumbs-up reaction on the request can confirm a clean round without a
separate review object. Acknowledgment without a terminal result remains pending
within [Loop Engineering's waiting bound](../../.claude/skills/loop-engineering/references/independent-review.md#waiting-bound).
If the App does not acknowledge or complete the round within that bound, the run
MUST confirm the repository setup and keep the review gate blocked rather than
read silence as a clean review.

A run MUST assess the correlated result under REVIEW.md's adopted arrangement,
not demand affirmative per-check assertions or an extra round solely for them.
Review completion is not convergence or readiness. Actual findings retain
their identity, priority, citations, and required dispositions under Loop
Engineering; explicitly unchecked required material, unmet approved criteria,
missing contributor evidence, failed checks, and material fixes lacking fresh
review still prevent readiness. Native scope never dismisses an actual finding.

## Acceptance Criteria Verified Out of Tree

Some acceptance criteria can be confirmed only by evidence no diff carries, such
as a machine-specific timing or a check against a real runner host. Prefer an
in-tree check wherever one can confirm the criterion. A plan MUST designate each
such criterion explicitly, by appending the marker
`(verified out of tree: <where the evidence will be published>)` to the end of
the criterion itself. The plan MUST agree the verification method and publication
destination, for example a linked issue comment, before implementation.

The pull request body's Acceptance criteria section MUST carry every designated
criterion verbatim, marker included, with its status and a verified link to the
published evidence. It MUST identify the approved plan revision and account for
any criteria not projected into the body. Evidence publication may remain pending
at draft creation only when the destination requires that draft to exist; the
author MUST publish the evidence, verify its locator, and update the body
immediately afterward, keeping the pull request draft until publication is
complete. Reviewer silence never satisfies this contributor obligation.

Some checks can happen only after merge, such as exercising the merged workflow
on the default branch. A plan MUST identify these as separately tracked
post-merge follow-up obligations, with the maintainer's approval of their timing
and evidence destination, not as fulfilled pre-ready criteria. Required
pre-ready evidence cannot be deferred this way without an approved plan
revision; existing approved plans are not silently reclassified. The follow-up
remains pending until its evidence is published at the agreed destination.

Changing a designation, method, destination, or required timing needs an approved
plan revision. [REVIEW.md](../../REVIEW.md) owns the code-inspection finding
boundary; these author-side duties do not turn missing external evidence alone
into a review finding.
