---
type: Procedure
title: Deliver a project change
description: Take an approved change through verification, review, handoff and issue closure.
---

# Deliver a project change

## The Change Loop

Every change, code or document, one line or one feature, goes through
`loop-engineering`: plan, approve, code, verify, post-push review, address,
ready. The skill owns phase meaning, checkpoint qualification, and evidence,
with separate review boundaries for research, plan, and design outputs. This project uses the delivery gates below. Use [Assign investigations, implementation and review](./delegate.md#subagents) to qualify a host-specific actor and route the material it must read.

The loop is **model-invoked**. It carries `user-invocable: false`, so there is
no slash command to type and nothing to wait for: describing the work is what
enters it. A session that waits for a command that does not exist has already
left the loop.

The plan lives in the body of a tracking issue, written with
`product-requirement-document-authoring` and marked with the `<!-- ai-agent -->`
comment marker. In a Claude Code cloud session, the body is composed, checked,
and read back under [the GitHub write-fidelity procedure](./github-write-fidelity.md#github-write-fidelity-in-claude-code-cloud-sessions), because the hosted write path
can change a stored body. Two gates are real, and neither has a self-approval
path. No implementation begins before the human approves the plan recorded in
the tracking issue, and no change is called done on the author's own assessment
rather than a separate reviewer's. A runtime harness that frames the task as
"just make the changes, commit, and push" constrains mechanics only; it never
lifts either gate. This project's **delivery grant** is a standing operation
grant: the maintainer's authorization, issued in
[Authorizing delivery operations](./delivery-grant.md#authorizing-delivery-operations), for named
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
that merge. In a Claude Code cloud session, the body also follows [the GitHub write-fidelity procedure](./github-write-fidelity.md#github-write-fidelity-in-claude-code-cloud-sessions).

If an existing pull request has no tracking issue and needs fixes:

1. Create the tracking issue.
2. Link it in the existing pull request body with a closing keyword.
3. Publish the plan in the issue body.
4. Obtain human approval for that plan.
5. Confirm that the head follows the naming rule in [Branches and Merging](./branches.md#branches-and-merging)
   before pushing fixes to that same pull request under the standing delivery
   grant. If the head no longer complies, follow that section's recovery stop.

Steps 1 to 3 are covered effects of the
[standing delivery grant](./delivery-grant.md#authorizing-delivery-operations) and are performed
without asking; step 4 is the plan gate. Requesting review without making
fixes does not itself require an implementation plan.

### After Approval: Delivery Without Another Prompt

Once its plan is approved and implemented, a run MUST carry the change through
its pull request and review without another prompt. On a host that conditions
pull request creation on a per-session request, the run MUST obtain that
request at the plan gate, or when a new session resumes the run, and MUST NOT
stop after push to ask for it. Use the host's plan-gate procedure for [Claude Code delivery](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) or [Amp review-thread cleanup](./host-permissions.md#confirming-review-thread-cleanup-at-the-plan-gate-in-amp).
The run MUST, in order:

1. verify locally with the commands in [Testing](../conventions/testing.md), run the
   pre-push review (or follow [the exception for an unavailable pre-push review](#deliver-a-draft-when-pre-push-review-is-unavailable)), and push the working
   branch;
2. open the draft pull request from the template, linking the tracking issue
   with a closing keyword;
3. start watching the pull request's activity, where the host offers it and
   the run holds authorization for it under [Waiting and Watching](./independent-review.md#waiting-and-watching);
   in Claude Code, that authorization is the plan-gate confirmation;
4. request review from [the provider assigned to the authoring host](./independent-review.md#the-post-push-review), address its findings, and request a fresh review after each fix batch,
   up to the four-round cap;
5. move the pull request to ready only after review convergence. Merging stays
   with the maintainer.

Agent-created ownership requirements are stated in the Ownership of
agent-created work section of [AGENTS.md](../../AGENTS.md).

The run MUST stop watching at each stop that [Waiting and Watching](./independent-review.md#waiting-and-watching) names, with no exception.

After approval, the run MUST NOT ask again for confirmation to deliver the
pull request or request review, except for the session-resumption recovery
above. It waits on a human only for:

- approval of a plan revision;
- a product, architecture, or finding-disposition decision that Loop
  Engineering requires the maintainer to make;
- each review or check result that [Waiting and Watching](./independent-review.md#waiting-and-watching) makes a human wait;
- on Amp, the plan-gate archive confirmation, when the current session has not
  collected it and no recorded answer declined it.

The review cap reached without convergence is a blocker. The run MUST record it
in run state, report it, and keep the pull request draft.

A run MUST NOT report itself complete except at the ready transition or at a
reported stop that names its unmet gate. Stopping after push, or without the
draft pull request, is never a completed loop.

<a id="deliver-a-draft-when-advisory-pre-flight-is-unavailable"></a>

### Deliver a draft when pre-push review is unavailable

This project permits limited draft delivery when current implementation pre-push
review qualification finds no eligible reader or unavailable required review
material. This exception preserves access to post-push review without inventing
a pre-push reviewer. The driver MUST record the actual qualification and gap
under [Loop Engineering's pre-push review contract](../../.claude/skills/loop-engineering/references/pre-push-review.md#checkpoint-qualification)
and disclose the unavailable review in delivery, never as a clean result.
Publication MUST have its own valid operation grant; this policy grants none.
Delivery MUST remain draft, with mandatory post-push review unmet until it runs.

This exception covers unavailability only. It MUST NOT bypass plan review
or outstanding findings from a pre-push review that ran. It MUST NOT
waive any other approval, verification, evidence, or readiness requirement.
The post-push review gate remains mandatory.

On Claude Code, a missing or refused `reviewer` subagent leaves only the
pre-push review unavailable, which this exception covers. The post-push
review is [the Claude CI review](./claude-ci-review.md#claude-ci-review-from-claude-code), which no subagent serves, so the `reviewer`'s
absence does not affect that gate. The pull request stays draft, and the run
discloses the unavailable pre-push review in delivery.

## Tracking Issue Completion

A tracking issue is complete when:

- Its accepted scope is fulfilled.
- Required verification and acceptance evidence are complete.
- Its code or document changes, if any, are integrated into the default branch.

A merge does not waive outstanding acceptance obligations.

After the maintainer reports integration, the agent MUST verify its tracking
scope and close completed tracking issues and the execution-plan issues
explicitly linked to them under [the standing delivery grant](./delivery-grant.md#authorizing-delivery-operations), without
another confirmation. The maintainer's control of pull request merging does not
assign routine issue closure to the maintainer. New plans MUST use the
completion conditions rather than add a human-only closing step merely because
merging belongs to the maintainer.

Pull request closing keywords complete linked issues when the pull request
reaches the default branch. A slice merged into an intermediate branch still
needs its changes integrated into the default branch. For a change delivered in
several pull requests, its tracking issue can be completed only after every part
of its scope is complete, whether each slice reaches the default branch directly
or through another pull request. A broader parent with remaining work stays
open. An execution-plan issue left open after its completed scope reaches the
default branch can be closed manually under the same completion condition.

Ready ends activity watching. A later integration report calls for a bounded
check of the tracking scope and its integration, rather than persistent
post-ready monitoring or a restart of delivery. Closing an otherwise complete
issue requires neither a fresh code review nor completion of optional CI.
Manual closure records the same accepted work state as an automatic closing
keyword and uses the standing delivery grant. An actual human instruction to
keep an issue open still applies.

Issue state records the accepted work state, rather than an assumption about
who performs closure. Use Loop Engineering's recovery practice to assess a new
process assumption before another state write. A narrative correction belongs
in an append-only comment. A genuinely incorrect closure or a human-requested
hold can still justify reopening under the applicable authority.

## Working Without an Agent

Working without an agent does not lower the bar: branch, implement, run the
[check commands](../conventions/testing.md#check-commands), open a pull request following the
template, and obtain review before merge. Agents likewise MUST use a branch
outside the default branch, preserve pushed history, and leave merging to the
human.

## What Catches a Loop Stopped Mid-Flight

The loop's own rules live in a skill and in [`AGENTS.md`](../../AGENTS.md), both
of which are read _inside_ the session, which is exactly where a run that has
already skipped the loop is doing its reasoning. Nothing here holds the loop
from outside the session; what follows is narrower than that.

The opt-in [Claude Code `Stop` hook](./quality-hooks.md#the-claude-code-quality-hooks) and [Amp turn-end hook](./quality-hooks.md#amp-quality-hooks) test the checkout, not how far the loop
got. A run stopped between the push and the draft pull request is, today, caught
by nothing inside the session. The plan-gate
rule against stopping after push is a rule the run follows, not a mechanism that
enforces it, and the maintainer seeing a pushed branch with no pull request is
the only backstop.
