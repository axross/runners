---
type: Procedure
title: Run a Claude Code review round
description: Qualify an independent reviewer, isolate its worktree, verify its posted result and account for its ending.
---

# Run a Claude Code review round

## Claude Code review subagent from Claude Code

Choose this provider through [the independent review gate](./independent-review.md#the-independent-review). Check [the grant's targets and effects](./delivery-grant.md#authorizing-delivery-operations) and the current session's [delivery confirmation](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code).

The Claude Code route follows
[Loop Engineering's review workflow](../../.claude/skills/loop-engineering/references/review-workflow.md)
with this project's choices and the deviation registered in
[Agent Skills](./skill-exceptions.md#deviation---the-claude-code-review-subagents-limits-and-closure). It has not yet been exercised in this
repository; [#155](https://github.com/axross/runners/issues/155) tracks the host
facts only its first live round can confirm. Each round runs in a new instance
of the project's `reviewer` subagent that the run spawns and owns; no instance
is reused or resumed across rounds or from any other use, and an instance is
never a fork of the authoring conversation.

1. Before spawning, confirm that the session can discover and spawn the
   `reviewer` subagent and exposes a GitHub publication route the instance can
   use, `gh` through `Bash` or a GitHub MCP tool; otherwise the route is
   unavailable and the review gate stays blocked. When the live `TaskStop`
   contract requires the human's request, use the answer this session collected
   as Agent Sessions' plan-gate confirmation section describes.
2. Spawn the instance with `isolation: worktree` on the call, never as a fork
   and never by resuming an earlier instance, and without overriding the
   definition's pinned model or effort. Give it a neutral assignment that names
   the repository, the pull request, the reviewed head commit, the author's
   checkout path, [`REVIEW.md`](../../REVIEW.md), the pull request body's
   acceptance criteria, and the round token: the tracking issue number, or the pull request number
   when there is no issue, plus the round number. The assignment names the
   instance as that round's independent review. Beyond what Loop Engineering's
   review workflow excludes, which includes the author's conversation, the
   implementation receipt, and the pre-flight ledger and findings, carry none of
   the author's claimed verdicts. Tell the instance to stop and post nothing if
   its working directory is the author's checkout; otherwise to fetch the
   reviewed commit and check it out inside its confirmed worktree, since the
   worktree branches from the default branch; to read the actual change and
   surrounding code; not to edit, push, or delegate, and to make no write other
   than that fetch and checkout and the writes that compose its review; to post
   one COMMENT-type review itself; and to report the review's URL, the commit it
   read, which of those limits its host enforced and which it followed by
   instruction, which GitHub identity it posted under, and any limitations.
3. Right after the spawn, checkpoint the round token, the agent ID the spawn
   returned, and the instance's worktree in run state, through [Save and resume run state](./run-state.md#save-and-resume-run-state). The round token, the posted review, and the worktree are
   the instance's durable identity; Claude Code's subagent documentation says
   subagents work within a single session, so the agent ID is a handle only the
   spawning session can use.
4. Before relying on the round, confirm from the spawn result that the
   instance's worktree path differs from the author's checkout. When the spawn
   result does not report the worktree path, confirm it from the instance's
   returned report, which is not yet exercised
   ([#155](https://github.com/axross/runners/issues/155)); the round counts only
   once the path is confirmed. A spawn result or report that gives the author's
   checkout, and an instance that reports its working directory is the author's
   checkout, each fail the isolation check: record the failure, stop the
   instance if it is still running, and keep the gate unmet. A review that an
   instance posted despite failing the check does not count; report it.
5. The instance posts its review on the pull request in the form REVIEW.md's
   Claude Code review subagent arrangement sets.
6. Wait for the instance as Agent Sessions' Waiting on CI and Review describes,
   within
   [Loop Engineering's waiting bound](../../.claude/skills/loop-engineering/references/independent-review.md#waiting-bound).
   Then read the posted review back, confirm that it is submitted rather than
   pending, and correlate it with the round token and the reviewed commit.
   Record the instance's report of the commit it read, the limits its host
   enforced and the limits it followed by instruction, and its posting identity
   in run state. [Review coverage evidence](./independent-review.md#review-coverage-evidence) applies to the result.
7. After each round, and before any next commit or the ready transition,
   confirm that the author's checkout is unchanged and that the pull request
   head is still the reviewed commit, and inspect whether the instance's
   worktree remains. Claude Code's documentation says a worktree is removed
   automatically only when the subagent made no changes, which this repository
   has not verified, so record what the inspection found and report a remaining
   worktree; never assert that it was cleaned up.
8. Record a completed instance as completed, with the review's URL, in run
   state. Never resume or message a completed or stopped instance. The
   documented host operations offer no way to close a completed or stopped
   instance, so that record ends the round without a handoff, as the deviation
   in Agent Skills registers.
9. Report the reviewer's pinned model and effort as declared, not measured,
   under
   [Loop Engineering's Model and Effort Certainty contract](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).

An instance that cannot post, a review that does not appear, a review that does
not correlate, and a failed isolation check are failures of the selected route,
which block the review gate as [The Independent Review](./independent-review.md#the-independent-review) states. A missing
or refused `reviewer` is one too, and no other subagent, provider, or session
the run chooses substitutes for it; the Claude CI alternative, or a review
session the maintainer instructs, applies only when the maintainer selects it
for the run. Record the incomplete evidence. Stop any round's instance
that is still running after its result is verified or its round has failed with
`TaskStop`, and record it as stopped.

When the run cannot stop a running instance, give the maintainer the agent ID,
the reason it remains, and the step: in a local interactive session, press `x`
on its row in `/tasks`. In a cloud session, where `/tasks` is not documented,
say that no human step is verified there. Report the remaining instance. On
resume, check what each recorded instance posted, by its round token, and its
worktree. A session other than the one that spawned the instance cannot see its
state: record whether it completed or was stopped as unknown and hand it over
with its agent ID as an instance the run cannot stop, before starting another
instance or counting another round.

One `reviewer.md` serves both the advisory pre-flight and this route, with an
assignment-keyed posting exception, and its `sonnet` and `high` pin is kept and
not overridden at spawn time. Both are the maintainer's decisions of 2026-11-20,
[recorded in issue #151](https://github.com/axross/runners/issues/151#issuecomment-7047417917),
and no further rationale was given for them. The cost of the shared definition
is the accepted instruction-only exposure listed with the [standing delivery grant](./delivery-grant.md#authorizing-delivery-operations).

A review session that another session starts for a run is not a route. Earlier
runs used one on a decision relayed for a single run, but no document described
it and the grant's targets cover only the actors the run itself creates, so it
adds a second actor outside the grant between the author and the review.
Reviews already completed that way are unaffected. A review session the
maintainer instructs for a run counts as that run's independent review only
when it meets the qualification in [The Independent Review](./independent-review.md#the-independent-review): a fresh
context without the author's conversation, given a neutral assignment that
carries no implementation receipt, pre-flight findings, or claimed verdicts,
started after the draft pull request exists, and posting its own COMMENT-type
review that the run correlates with a round token and the reviewed commit. The
run records the maintainer's instruction, with its locator, in run state. A
decision another session relays without that instruction is not one.
