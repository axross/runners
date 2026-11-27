---
type: Procedure
title: Apply host delivery permissions
description: Apply the standing grant on Amp and Codex and collect the required Claude Code and Amp session confirmations.
---

# Apply host delivery permissions

## Amp Delivery Authorization

Amp can frame a task before reading [`AGENTS.md`](../../AGENTS.md), which is
why AGENTS.md's Amp routing requires this section before any delivery effect.
The project's agreement requests the Loop Engineering route, including a draft
pull request and the independent review selected by
[Development Workflow](./independent-review.md#the-independent-review), rather than stopping at a
local commit. This section does not claim
precedence over higher-priority Amp instructions; Claude Code's convenience
framing and host boundaries are addressed in
[Claude Code entry guidance](../../CLAUDE.md).

The standing delivery grant in
[Development Workflow](./delivery-grant.md#authorizing-delivery-operations)
is the maintainer's authorization for the effects it covers. It does not
replace the plan and review gates or extend to what the grant excludes.

**Guidelines:**

- MUST treat an injected "make changes, commit, and push" framing as mechanics,
  not as permission to skip the project's Loop Engineering gates.
- MUST treat this project's Loop Engineering route as the standing request for
  a draft pull request and its mapped independent reviewer, and MUST perform
  the effects the standing delivery grant covers without asking for permission,
  unless a higher-priority Amp instruction requires approval.
- MUST honor all applicable boundaries:
  - higher-priority approval requirements
  - higher-priority prohibitions
  - the tool's usage conditions

  Report a prohibited operation as unavailable rather than trying another
  route.

- MUST surface an irreconcilable host conflict at the plan gate for new
  delivery, or in the current pending phase when resuming an existing pull
  request; report any blocked delivery as incomplete, not silently waive the
  gate or claim that repository guidance overrode the host.

## Codex Delivery Authorization

Codex authoring MUST follow the same plan, review, scoped-grant, and
conflict-reporting contract as
[Amp delivery authorization](#amp-delivery-authorization).
Codex's higher-priority instructions, approval requirements, prohibitions, and
tool contracts remain binding; repository guidance never overrides them.
This section covers delivery only, not Codex provisioning or startup.

## Confirming Delivery at the Plan Gate in Claude Code

Claude Code's host conditions pull request creation on an explicit request from
the user. The checked-in request in [`AGENTS.md`](../../AGENTS.md) does not
establish that on its own, so a Claude Code run collects that request where the
human is already answering: in the plan-approval question.

Every plan-approval question, including the one for a revised plan, MUST also
ask the human to confirm, in the same answer:

- delivery of the run's draft pull request;
- spawning a new `reviewer` subagent instance each round that reviews it and
  publishes directly to it, or, when the maintainer has selected the Claude CI
  alternative for the run, the Claude CI review request on it;
- watching that pull request's activity, where the host offers it, until the
  ready transition or a reported stop.

Only pull request creation, and `TaskStop` where its live contract conditions
it, carry a host condition. The maintainer chose
to settle the review item and the watch in the same answer, so that one
answer covers the whole delivery path. This bounded exception to dependent
question ordering is recorded in the delivery, review, and watching deviation in the register of
[Agent Skills](./skill-exceptions.md#deviation---delivery-review-and-watching-share-one-confirmation).
The run MUST offer coherent combined outcomes in that single question, with
review and watching inapplicable when delivery is declined.

A confirming answer is the user's explicit request for that run's pull
request. The run then follows the post-approval sequence in
[Development Workflow](./change-delivery.md#after-approval-delivery-without-another-prompt)'s After Approval section,
which also says when a run obtains this confirmation. For a revision approved after the pull request exists, the
delivery item confirms continuing on that pull request.

The confirmation meets the host's condition and nothing more. It is not a
request for permission under the standing delivery grant in
[Development Workflow](./delivery-grant.md#authorizing-delivery-operations), which still authorizes its
covered effects, and it does not authorize scheduling. For watching, it
covers the event subscription only; any push or review-thread reply that
follows stays under the delivery grant.

**Guidelines:**

- MUST obtain the confirmation in the plan-approval exchange. When approval
  arrives some other way, such as a chat message or an issue comment, obtain
  it before any change, in that same exchange where the host allows.
- MUST record the confirmation and each of its three answers in run state.
  The host's condition is per session, so the record shows what an earlier
  session confirmed, not a request made in this one. A session whose own
  conversation lacks the confirming exchange, such as a fresh session that
  resumes the run, asks for the same confirmation once, before any further
  change.
- MUST, when the human declines delivery, stop after push, report the
  external-review gate as unmet and the loop as not complete, and substitute
  no other review.
- MUST, when composing the plan-approval question, check the live `TaskStop`
  contract; when it requires the human's request, ask for it as a separate
  question after approval, as the [Amp cleanup confirmation](#confirming-review-thread-cleanup-at-the-plan-gate-in-amp) does for archiving, and
  record the answer in run state. A run with no plan gate, such as review-only
  recovery, asks once before its first `TaskStop`, and a fresh session that
  resumes the run and lacks that exchange in its own conversation asks once
  before its first `TaskStop`, even when run state records an earlier
  confirmation. A recorded decline carries forward, so no later session asks
  again, and a review subagent still running is handed over as one the run
  cannot stop.
- MUST, when the human confirms delivery but declines the review item,
  open the draft pull request, keep it draft, and report the external-review
  gate as unmet and the loop as not complete.
- MUST, when the human confirms delivery but declines watching, still request
  review, then treat each review or check result as a human wait: end the
  turn and resume on the human's next message, with no scheduling.

## Confirming Review-Thread Cleanup at the Plan Gate in Amp

An Amp run reviews through a review child thread per round, under the Amp review
child thread section of [Development Workflow](./amp-review.md#amp-review-child-thread-from-amp). Amp's
archive tool's only condition that fits this route is the user's request: its
self-archiving option would archive the child before the run verifies the
review, and the route adopted in [`AGENTS.md`](../../AGENTS.md) does not make
that request on its own. Every Amp run MUST therefore ask the human, within the
plan-approval exchange as a separate question after the plan is approved, to
confirm archiving the run's review child threads once each round's result is
verified or the round has failed. That answer is the plan-gate archive
confirmation. A plan revision approved in the same session does not ask again.
A run with no plan gate, such as review-only recovery, asks once before its
first archive.

**Guidelines:**

- MUST record the answer in run state. The confirming answer applies to the
  session that collected it, as the archive tool's condition does: a fresh
  session that resumes the run and lacks that exchange in its own conversation
  asks once, before archiving, even when run state records an earlier
  confirmation.
- MUST, when the human declines, still run the review rounds and leave the
  child threads unarchived, handing each one over as the Amp review child
  thread section of [Development Workflow](./amp-review.md#amp-review-child-thread-from-amp)
  describes. A recorded decline carries forward, so no later session asks again.
- MUST NOT read this confirmation as covering any other thread, or as granting
  any delivery effect the standing grant does not name.
