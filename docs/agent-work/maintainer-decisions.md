---
type: Procedure
title: Ask for a maintainer decision
description: Choose the available question mechanism, including decisions from launched sessions and subagents.
---

# Ask for a maintainer decision

## Asking the Maintainer

[`AGENTS.md`](../../AGENTS.md) states, for every host, the rule for asking the
maintainer. It states the rule itself, not a pointer to this section, because
AGENTS.md is the one file every session loads, and a session can miss a route it
must first follow. This section names Claude Code's mechanism, says how launched
and launching sessions on any host apply that rule, and covers Claude Code
[subagents](./delegate.md#subagents) and forks. The installed
[Asking the Human](../../.claude/skills/professional-behavior/references/asking-the-human.md)
practice owns how a question is framed, recovered, and made to fall back, and
this section does not restate it.

In Claude Code the mechanism is `AskUserQuestion`. The maintainer owns every
decision that [`AGENTS.md`](../../AGENTS.md),
[Development Workflow](./change-delivery.md#the-change-loop), and
[Loop Engineering](../../.claude/skills/loop-engineering/SKILL.md) leave to the
maintainer, among them scope, product behavior, an architecture choice, a
finding's disposition such as dismissing a Critical or Major pre-push finding,
an additional review round, and a plan approval with its delivery confirmation.
In plan mode, `ExitPlanMode` carries plan approval instead, as that tool's live
contract states. Plan-mode approval, like approval that arrives some other way,
MUST follow the plan-gate confirmation's rule for collecting the delivery
confirmation.

**Guidelines:**

- MUST ask each such decision with `AskUserQuestion` whenever the session
  exposes it and its live contract qualifies it for that question, however the
  session was started. MUST NOT list options as prose in the turn output in place
  of the question unless no mechanism is eligible.
- MUST begin a wait for a maintainer decision with the question, and handle each
  of its three outcomes as the Choosing the Route section of the practice
  states: answered by the maintainer; accepted and still awaiting a reply; or
  closed, cancelled, errored, reported missing, or ended without an answer. For
  the last outcome, MUST NOT end the turn with the dependent work blocked before
  that section's turn-output fallback has been used.
- MUST NOT ask a question for a wait on review or check results. Those waits are
  governed by [Waiting and Watching](./independent-review.md#waiting-and-watching).

### Judge eligibility from the session

**Guidelines:**

- MUST judge eligibility from the session's tool list and the tool's live
  contract.
- MUST NOT read any of these as making the tool ineligible: a seeded prompt's
  framing as an automated or "not from the user" event, which says who sent that
  message and not whether the maintainer can answer a question; cloud or
  headless hosting; or the absence of earlier human messages.
- MUST treat only a human-authored answer to the question, not the tool's
  acceptance or an error, as showing that the maintainer answered it, and MUST
  NOT conclude from an error alone that no one can answer.
- MUST let the host's own statement that the tool is unavailable or prohibited
  for the session govern, and report the conflict.

### Ask from a launched session

A launched session is a separate session that another session started, with
`create_session` or otherwise. A subagent is not one.

When a launched session has an eligible mechanism, the maintainer can answer in
that session's own conversation. Options sent to the launcher, or left as prose,
reach the maintainer only if someone finds them and carries the answer back. A
launcher's wording about channels says how the launcher wants to hear about a
decision, not who decides it or where the maintainer answers, so it adds a duty
to report rather than replacing the question.

**Guidelines:**

- MUST ask the maintainer directly with its own eligible mechanism, and report to
  its launcher at each outcome:
  - while the question is with the maintainer and still pending, saying so, so
    the launcher does not ask it again;
  - when the maintainer answers, with the answer;
  - when the question ends without an answer, with what stays blocked.
- MUST send the pending report in the same turn as the question, alongside it or
  just before it. A report MUST NOT take the question's place or end the turn
  before the question is asked.
- MUST read a launch prompt's statement that the launcher handles communication
  with the maintainer, or its instruction to stop and report or to send the
  options to the launcher, as the duty to report. A session MUST NOT read such
  wording as making its mechanism ineligible or the launcher its only channel.
- MUST take only the maintainer's own message in the launched session's
  conversation as routing a decision some other way. A launch prompt that quotes
  or paraphrases the maintainer does not, and a session that is unsure MUST ask.
- MAY treat a maintainer answer its launcher passes on, quoted with where and
  when the maintainer gave it, as settling a decision it is waiting on. MUST NOT
  treat it as a rerouting, a plan approval with its delivery confirmation, or a
  tool request the host conditions on the user's own request, and MUST ask when
  unsure whether it settles a decision.
- MUST return the question and its partial results to the launcher only when no
  mechanism is eligible and the session cannot reach the maintainer, as the
  Execution Result section of
  [Loop Engineering's assignment contracts](../../.claude/skills/loop-engineering/references/implementation-package.md)
  states.

### Launch a session

A session that writes another session's starting prompt for separately owned
work, with `create_session` or otherwise, shapes the route the launched session
takes.

**Guidelines:**

- MUST say in the prompt that maintainer decisions are asked directly with the
  launched session's own question tool and reported back, and MUST NOT present
  the launcher as the launched session's only channel.
- MUST NOT start the launched session in a permission mode that denies its
  question tool, such as Claude Code's `dontAsk`, which denies `AskUserQuestion`
  even when an allow rule matches it.
- MUST pass on maintainer answers that affect the launched session's work,
  quoting the answer and saying where and when the maintainer gave it, and MUST
  NOT present one as settling an answer the maintainer must give in the launched
  session itself: a plan approval with its delivery confirmation, or a tool
  request the host conditions on the user's own request.
- MUST NOT put to the maintainer a decision that a launched session reports as
  asked and still pending, since the maintainer answers it in that session.
- This rule does not cover subagent assignments or the [Amp review child thread's assignment](./amp-review.md#amp-review-child-thread-from-amp).

### Return a decision from a subagent

Claude Code's [subagent documentation](https://code.claude.com/docs/en/sub-agents)
lists `AskUserQuestion` among the tools it removes from every subagent "even when
listed in the `tools` field" (Available tools). That is documented, not
exercised in this repository. The configured implementer, reviewer, and
investigator therefore MUST return an unresolved maintainer decision and their
partial results to their caller, which asks it, as the practice's Returning a
Question You Cannot Ask section states.

The same page says a fork receives its parent conversation's exact tool pool, so
a fork is inferred to keep `AskUserQuestion`, which is also not exercised here. A
fork MUST follow the first guideline of Asking the Maintainer, and MUST return
any question it asked, and the answer, to its caller with its result.
