---
type: Procedure
title: Wait for review and CI
description: Identify the host mechanism that delivers a check or review result and verify what arrived.
---

# Wait for review and CI

## Waiting on CI and Review

[Waiting and Watching](./independent-review.md#waiting-and-watching) governs what each host waits on, when it may schedule a check-in,
and which results are human waits.
[Loop Engineering's waiting bound](../../.claude/skills/loop-engineering/references/independent-review.md#waiting-bound) governs deadlines, budget resets, human waits, and teardown at each stop. A cloud environment's lifetime is not that deadline.
Authorization and recovery state belong to
[Loop Engineering's grant and run-state reference](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
The [standing delivery grant](./delivery-grant.md#authorizing-delivery-operations) covers neither scheduling nor subscriptions.
On Claude Code, check the [delivery confirmation](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) for authorization to watch this pull request.

For a Codex-authored pull request, the awaited results are its required GitHub
checks and the [independent Codex review](./codex-review.md).
Qualify the session's tools under [GitHub Operation](../../.claude/skills/github-operation/SKILL.md).

For an Amp-authored pull request, the awaited results are its required GitHub
checks and the review its Amp review child thread posts. The
[Amp review child thread from Amp](./amp-review.md#amp-review-child-thread-from-amp)
procedure covers how to wait for the child, read back its posted review, and
verify the result.

For a Claude Code-authored pull request, the awaited results are its required
GitHub checks and the [Claude CI review](./claude-ci-review.md#claude-ci-review-from-claude-code). Claude Code's
[subagent documentation](https://code.claude.com/docs/en/sub-agents) says
subagents run in the background by default and that, while fork mode is on, an
interactive session cannot block on one, so a worker's completion arrives as a
notification. The subscription is the Remote `subscribe_pr_activity` tool, and
`unsubscribe_pr_activity` ends it. Under the tools' live contracts, a subscribed
pull request's comments, reviews, and check results reach the conversation as
events in one session at a time. If another session already watches the pull
request, the subscribe result says so and this session receives no events. In this
repository, Claude CI rounds have delivered their inline findings and tally
comment this way, and a notice that the head's check suites completed without a
failure has arrived as an event.

Use the current pull request head when reading check results. A connector read
can return only a first page of workflow runs, so it cannot establish complete
required-check coverage by itself. Read the selected review's posted result and
its findings through the tools the session exposes, and respect each read's
pagination and completeness limits. Missing output leaves the check or review
unverified. [The Independent Review](./independent-review.md#the-independent-review) defines review-completion evidence.

Claude Code on the web also documents
[PR auto-fix](https://code.claude.com/docs/en/claude-code-on-the-web), which
delivers review comments and CI failures when enabled. It also investigates
events, pushes clear fixes, and can publish review-thread replies under the
user's GitHub identity. The plan-gate confirmation covers the Remote event
subscription. Enabling Auto-fix requires a scoped grant for its subscription,
pushes, and replies. Its documented stop path
is clearing **Auto-fix** in the session's CI status bar or asking Claude to stop
watching the pull request. This documentation does not establish that Auto-fix
delivers successful-check transitions, and the consulted official pages give no
verified procedure for invoking the Remote tools. The events observed above are
this repository's only live evidence of their behavior. Use the definitions
exposed by the host.

[Claude Code's entry guidance](../../CLAUDE.md) owns how the plan-gate watch
meets an injected or harness instruction to subscribe, and how an injected or
harness instruction to keep watching or to schedule a check-in is handled.
