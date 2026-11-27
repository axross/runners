---
type: Procedure
title: Checkpoint and resume a run
description: Publish recoverable state without altering the approved plan and identify the correct checkpoint when resuming.
---

# Checkpoint and resume a run

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
under [The Change Loop](./change-delivery.md#the-change-loop), the first issue-side state comment
MUST link the retained description-side history; subsequent checkpoints belong
on the issue.

These publications use the status-comment or pull-request-description effects of
the standing delivery grant in [Authorizing delivery operations](./delivery-grant.md#authorizing-delivery-operations). A run
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
Record the host-specific confirmation for [Claude Code delivery](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) or [Amp review-thread cleanup](./host-permissions.md#confirming-review-thread-cleanup-at-the-plan-gate-in-amp).

What each checkpoint preserves follows Loop Engineering's
[Semantic Run State](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
Pre-push finding durability follows its
[Durable Parks](../../.claude/skills/loop-engineering/references/pre-push-review.md#durable-parks)
restrictions. This project adds no schema, adapter, or mandatory local state
file. Public storage does not promise reviewer invisibility.

At ready, the checkpoint MUST reflect the pull request's actual ready state and
link the post-push review and required-check evidence that allowed the
transition. It MUST leave maintainer merge as the remaining action. Append a
correction that supersedes any obsolete instruction to keep the pull request
draft or request an already completed review, preserving the earlier entries.

Checkpoint publications on both surfaces MUST follow GitHub Operation's
[Body Integrity](../../.claude/skills/github-operation/references/body-integrity.md)
and [Publication and Recovery](../../.claude/skills/github-operation/references/publication-and-recovery.md)
contracts and, in a Claude Code cloud session, [the GitHub write-fidelity procedure](./github-write-fidelity.md#github-write-fidelity-in-claude-code-cloud-sessions). A
pull-request-description append is a whole-body replacement, so the exception
path depends on full-body fidelity before and after publication. Retain verified
status-comment URLs in the next checkpoint or handoff. If the required route or
fidelity is unavailable, retain the current permitted handoff instead of
publishing through an unverified path. The run MUST name that handoff in its
report. When it is retained in the current conversation or returned to the
assigning parent, identify its executor-local scope: it is not published issue
history, and a fresh executor cannot recover it from the issue alone. Transfer
that handoff explicitly to the next executor; this fallback creates no
alternative public destination or raw write route.

### Retrieve state on resume

A resuming executor MUST use this procedure before acting on saved state:

1. Identify the work target from the current session or an explicit handoff.
   From a pull request, follow its tracking-issue link when present. Without an
   issue, review-only recovery uses the description exception above; fixes use
   [The Change Loop](./change-delivery.md#the-change-loop)'s issue-creation path. If the work target is absent or
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
