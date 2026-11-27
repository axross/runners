---
type: Procedure
title: Select and verify an independent review
description: Select the provider for the authoring host and decide whether its posted result meets the review gate.
---

# Select and verify an independent review

## The Independent Review

The review runs in a separate context, never the authoring session, whatever
that session calls its own assessment. The Codex route also runs under a
separate reviewer identity, and so does the explicitly selected Claude CI
alternative, except when a run posts its request through the Claude GitHub App,
whose `claude[bot]` identity the reviewer shares (see
[Security](../security/index.md)). The Amp and Claude Code routes do not:
their reviewer, an Amp review child thread or a Claude Code review subagent,
may post under the same GitHub operator account and runs on
the same infrastructure as its author. The maintainer accepts context
independence for those routes, a fresh Amp thread or Claude Code review
subagent that carries none of the author's conversation, as
[Loop Engineering's review-workflow default](../../.claude/skills/loop-engineering/references/review-workflow.md#default-independent-review-arrangement)
allows. A fresh Amp review child thread, or on Claude Code a new, non-fork
`reviewer` instance assigned as that round's independent review, that the run
starts after the draft pull request exists and that posts its own review, is
the independent review for its route. Qualification is judged per instance and
per assignment: sharing the `reviewer` definition with advisory or
phase-deliverable instances does not disqualify an instance, and sharing their
context does. A run MUST NOT count any other review as satisfying or
substituting for it: not a fork, not a resumed or reused instance of any use,
not an instance that is not so assigned or whose assignment carries pre-flight
findings, not a phase-deliverable reviewer, not a review session started by
another session unless the maintainer instructs it for the run, and not the
author's own checks. The review is reachable only
through the run's draft pull request, so a run that has not opened it has not
reached this gate and is not complete.

The authoring host determines the provider and request route:

| Authoring host         | Review provider                                                                                         | Request                                                                                  |
| ---------------------- | ------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Claude Code            | [Claude Code review subagent](./claude-subagent-review.md#claude-code-review-subagent-from-claude-code) | Spawn a new `reviewer` instance in a worktree that posts its review on the pull request. |
| Codex                  | [Codex review](./codex-review.md#codex-review-from-codex-or-a-manual-change)                            | Post `@codex review` as a top-level pull request comment.                                |
| Amp                    | [Amp review child thread](./amp-review.md#amp-review-child-thread-from-amp)                             | Start a fresh Amp thread that posts its review on the pull request.                      |
| Manual (no agent host) | [Codex review](./codex-review.md#codex-review-from-codex-or-a-manual-change)                            | Post `@codex review` as a top-level pull request comment.                                |

The Claude workflow is an alternative a Claude Code run uses only when the
maintainer explicitly selects it for that run, not the default for every change
just because it exists here. Codex uses the external Codex App. Amp and Claude
Code use a fresh reviewer, an Amp review child thread and a Claude Code review
subagent, and request no comment-triggered review; a Claude
Code run requests the Claude CI review only when the maintainer selects that
alternative. Manual changes
have no agent host, so they use Codex rather than claiming a Claude Code
route.

Before the first request, an agent run MUST record its authoring host and the
selected provider in recoverable Loop Engineering run state, through
[Save and resume run state](./run-state.md#save-and-resume-run-state). For a manually
authored pull request, the author MUST record "manual" and "Codex review" in the
pull request description before requesting review and keep that record current
across rounds. Before each later request, confirm that the recorded host and
provider remain unchanged; if the maintainer explicitly changes the provider,
append that decision to the run state or pull request description and update the
selection first. Each round invokes exactly one provider. A failed, silent, or
unavailable selected route blocks the review gate; it never causes an automatic
request to another provider.

The literals above and in [Alternative Claude CI review](./claude-ci-review.md#alternative-claude-ci-review) are reference
documentation, not requests. In each review
round of a comment-triggered route, write the trigger literal in exactly one
GitHub comment, that round's intentional top-level request. Do not copy it into
a plan or pull request body, recoverable state, summary, progress comment, or
reply. Everywhere else, refer to the applicable phrase by name. This prevents a
comment-triggered integration from starting a duplicate review while permitting
a fresh request after a fix batch. The review has a cap of four rounds; see
[Loop Engineering's external round cap](../../.claude/skills/loop-engineering/references/independent-review.md#external-round-cap).

### Review coverage evidence

For every round, the author MUST compare the reviewer-origin result with
[`REVIEW.md`](../../REVIEW.md)'s Reporting section. In recoverable run state,
saved through [Save and resume run state](./run-state.md#save-and-resume-run-state), correlate
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
contributor's out-of-tree obligations in [Acceptance Criteria Verified Out of Tree](./acceptance-evidence.md#acceptance-criteria-verified-out-of-tree), remain blockers even when the review is silent about them.
