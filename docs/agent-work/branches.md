---
type: Convention
title: Branch naming and merging
description: Choose a working branch, preserve published history and handle a nonconforming pull request head.
---

# Branch naming and merging

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
requirements; agent changes still follow [The Change Loop](./change-delivery.md#the-change-loop).

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
