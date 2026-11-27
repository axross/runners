---
type: Procedure
title: Wait for review and CI
description: Qualify a host wait mechanism and determine when the run must report an unmet wait instead.
---

# Wait for review and CI

## Waiting on CI and Review

Use the [Claude Code delivery confirmation](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) to determine whether watching this pull request was requested. The [standing grant](./delivery-grant.md#authorizing-delivery-operations) defines the subsequent effects available to the run.

The host supplies the wait mechanism. The portable contract lives in
[Loop Engineering's independent-review reference](../../.claude/skills/loop-engineering/references/independent-review.md),
whose Waiting Bound section owns capability resolution, deadlines, budget
resets, human waits, and teardown at each stop. A scheduler's expiry or cloud
environment's lifetime is not that deadline. Authorization and recovery state
belong to [Loop Engineering's grant and run-state reference](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
This project's delivery grant, in [Development Workflow](./delivery-grant.md#authorizing-delivery-operations)
does not name scheduling or subscriptions.

For a Codex-authored pull request, the awaited results are its required GitHub
checks and the independent Codex review selected by Development Workflow.
A permitted in-session wait followed by sanctioned GitHub reads can retrieve
those results; a missing scheduler alone does not establish inability.
Qualify the actual session's tools under GitHub Operation rather than assume
all Codex surfaces expose the same mechanisms.

For an Amp-authored pull request, the awaited results are its required GitHub
checks and the review its Amp review child thread posts. Wait for the child with
Amp's thread-wait tool, then read the posted review back on the pull request;
Development Workflow owns what counts as the result. Wait for checks with an in-session
wait followed by sanctioned GitHub reads, or with a subscription only where the
host and the run already permit it.

For a Claude Code-authored pull request, the awaited results are its required
GitHub checks and the review its Claude Code review subagent posts, or, when the
maintainer selected the Claude CI alternative for the run, the Claude CI review.
Claude Code's [subagent documentation](https://code.claude.com/docs/en/sub-agents)
says [subagents](./delegate.md#subagents) run in the background by default and that, while fork mode is
on, an interactive session cannot block on one, so the instance's completion
arrives as a notification rather than as the result of a blocking wait; this
has not yet been exercised for the review subagent, and
[#155](https://github.com/axross/runners/issues/155) tracks its first live
round. After it arrives, read the posted review back on the pull request. A
review subagent's permission prompt appears in the
authoring session, and one the run cannot answer for the human is a human wait,
as the After Approval section of [Development Workflow](./change-delivery.md#after-approval-delivery-without-another-prompt)
lists.

Use the current pull request head when reading check results. A connector read
can return only a first page of workflow runs, so it cannot establish complete
required-check coverage by itself. Read the selected review's posted result and
its findings through the tools the session exposes, and respect each read's
pagination and completeness limits; missing output is not a successful check or
completed review. Development Workflow owns review-completion evidence.

Claude's documented mechanisms have different scopes. The summary below follows
the linked pages and has not been exercised in this repository; the tools the
actual host exposes decide what a run can use:

- **Local CLI session:** [session scheduling](https://code.claude.com/docs/en/scheduled-tasks)
  uses `/loop` or `CronCreate`, with `CronList` to inspect tasks and
  `CronDelete` to cancel by ID. A self-paced `/loop` can stop with `Esc` or
  `ScheduleWakeup` with `stop: true`; `Esc` does not delete separately scheduled
  tasks. Tasks fire only while Claude Code is running and idle. These are not
  the Remote MCP tools `send_later` and `delete_trigger` that the example
  settings permit. If these are the only available wait mechanisms and no
  separate scheduling authorization exists, the run MUST treat each review or
  check result as a human wait.
- **Cloud session:** [PR auto-fix](https://code.claude.com/docs/en/claude-code-on-the-web)
  delivers review comments and CI failures when enabled. It also investigates
  events, pushes clear fixes, and can publish review-thread replies under the
  user's GitHub identity. Before enabling Auto-fix, its scoped grant must cover
  the subscription and these downstream effects, not event delivery alone.
  Its documented stop path is clearing **Auto-fix** in the session's CI status
  bar or asking Claude to stop watching the pull request. This documentation
  does not establish delivery of successful-check transitions, or specify the
  `send_later` / `delete_trigger` schemas. The consulted official pages provide
  no verified Remote invocation procedure; live Remote behavior remains
  unverified here. Use the actual host's exposed definitions, not guessed
  arguments or a CLI-tool substitution.

In Claude Code, watching the run's own pull request is what the plan-gate
[Claude Code delivery confirmation](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) covers, and that watch ends at the ready transition or at any
reported stop. [Claude Code's entry guidance](../../CLAUDE.md) states how an
injected instruction to watch a pull request until it merges or closes is
evaluated against this project's run, which ends at ready.
