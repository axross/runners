---
type: Procedure
title: Assign investigations, implementation and review
description: Qualify the actor and assignment for the host, route evidence to it and record an unavailable role.
---

# Assign investigations, implementation and review

## Subagents

Role selection depends on the actual host and assignment.
[Loop Engineering](../../.claude/skills/loop-engineering/SKILL.md) defines the
loop driver and owns checkpoint qualification, handoff and result contracts.
This repository checks in subagent definitions for Claude Code only: no
`.codex/` role definitions exist, and the Amp configuration is the quality-hooks
[quality-hooks plugin](./quality-hooks.md#amp-quality-hooks), which defines no roles. A Codex or Amp session qualifies whatever
its host exposes under
[Loop Engineering's delegation contract](../../.claude/skills/loop-engineering/references/subagent-delegation.md)
and records an unavailable role as below.

### Use Claude Code subagents

[`.claude/agents/`](../../.claude/agents/) holds this repository's three Claude
Code definitions, and it is the only home for any of them: an agent definition
is not a skill, so the skills CLI never carries it, and it never appears in
`skills-lock.json`. Their frontmatter configures Claude Code, not Amp.

`implementer.md` is the implementation-capable actor `loop-engineering`
delegates Code and Verify to. It pins `model: sonnet`, because a worker
inheriting the session's model runs at the main actor's cost and defeats the
point of delegating. Local results return to the parent; publication is not
delegated merely by choosing this actor. It states its delivery boundary, that
commits stay local and pushing and publishing belong to whoever asked, in its
own prose rather than by withdrawing a tool.

`reviewer.md` is the reader for the advisory pre-flight review and, under an
assignment that names it so, the Claude Code independent review. It denies
exactly two things, editing and spawning, which are the Edit, Write, and
NotebookEdit tools and the Agent tool, and nothing else. Widening that deny-list
is the tempting mistake and MUST be resisted: judging a change means confirming
what was asked and not only what was written, which reaches the issue, the
plan's artifacts, and the documentation behind a factual claim. A reviewer that
cannot reach one of those does not fail to start; it returns a report short by
exactly those checks, and an under-equipped review reads exactly like a clean
one. The denial covers editing tools and nested spawning, not every possible
shell write. Without an assignment that names the instance as a round's
independent review and carries that round's token, it returns its report
privately and posts nothing; with one, it posts exactly one COMMENT-type review.
Of these limits, only that denial is one Claude Code documents enforcing; no
live round has confirmed it yet, and
[#155](https://github.com/axross/runners/issues/155) tracks the first. Every
other limit is instruction-only for every instance: shell writes and commits,
push and merge, approve or request-changes verdicts, GitHub writes beyond the
round's one review, starting or messaging actors through non-Agent routes, and
the posting condition itself.
The standing delivery grant in [Development Workflow](./delivery-grant.md#authorizing-delivery-operations)
owns that list. An instance used for
the pre-flight is advisory and is never the independent review; only a new
instance the run assigns that role counts.

`investigator.md` is the reader for a payload the main actor needs only one
conclusion from: a log, a long thread, a wide search across files or history, a
file tree. It returns a conclusion and a locator precise enough to go back to the
source, never the payload itself. Like the reviewer, it denies editing and
spawning, and it decides nothing the material does not itself settle, sending an
unresolved judgment call back to whoever asked. It is not required for an exact
local lookup or when the host prohibits that delegation purpose.

Each of these exists to keep some context out of the main actor's own: the
implementer so it does not inherit the planning phase's accumulated context, the
reviewer so it does not inherit the implementer's reasoning state or, as the
independent review, the author's conversation; and the investigator so a large
payload never enters the main actor's context at all.
All three run Sonnet at `effort: high`. The investigator runs at `high` rather
than a lower level because its output is a judgment the main actor cannot check
without re-reading the payload it delegated away; the maintainer approved that
default in the plan for [issue #1](https://github.com/axross/runners/issues/1),
and it is reported as declared, not measured. A declaration alone does not verify
what a worker ran; report declared settings separately from evidence of it under
[Loop Engineering's Model and Effort Certainty contract](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).

These are configured candidates, not a permission grant. A session MUST check
the capabilities its host actually permits before using one. Delegating to the
matching configured actor is the default; parent implementation is valid only
when delegation is unavailable or disallowed, and mandatory verification and
external review remain unchanged. Deleting a file never lets the loop skip the
independent-review gate. Without the implementer, the loop delegates to a
generic agent or runs single-agent. Without the reviewer, the pre-flight stage
is skipped rather than performed by the main actor, which is what keeps it from
collapsing into self-review, and the Claude Code independent-review gate stays
unmet, as [Record an unavailable role](#record-an-unavailable-role), below,
states. Without the investigator, the main actor reads the payload itself.

The advisory pre-flight review applies after every verified initial
implementation, before the first branch push and draft pull request, whether the
parent or a child implemented it. A compatible fresh reader must be permitted
and available; its findings and round limits follow
[the pre-flight contract](../../.claude/skills/loop-engineering/references/pre-flight-review.md).
If no reader qualifies, record the exact unavailable or prohibited reason and the
resulting delivery restriction. That outcome is not a clean review; the Deliver a
draft when advisory pre-flight is unavailable section of
[Development Workflow](./change-delivery.md#deliver-a-draft-when-advisory-pre-flight-is-unavailable) states what it permits,
including that on Claude Code a missing `reviewer` leaves both gates unmet.

The independent review's Claude Code review subagent is one more use of
`reviewer.md`, not a fourth definition. Each round spawns a new instance with
`isolation: worktree` on the call itself, never as a fork, because a fork
inherits the author's conversation, and never by resuming an earlier instance.
The pinned model and effort are not overridden at spawn time. Claude Code's
[subagent documentation](https://code.claude.com/docs/en/sub-agents) says a
subagent loads the project's CLAUDE.md, which here imports AGENTS.md; this has
not yet been exercised for `reviewer`, and
[#155](https://github.com/axross/runners/issues/155) tracks it. AGENTS.md's Code
Review Rules exempt the instance from the delegation rule. The Claude Code
review subagent procedure in [Development Workflow](./claude-subagent-review.md#claude-code-review-subagent-from-claude-code)
owns its assignment, isolation check, checkpoints, and ending, and states why a
review session started by another session is not a route; the register in
[Agent Skills](./skill-exceptions.md#deviation---the-claude-code-review-subagents-limits-and-closure) records where the route departs from Loop
Engineering's review-child defaults.

For Claude Code reads, keep the exact object of a judgment and the rules governing
that judgment in the judging actor's context. Route other large material wanted
only for one conclusion to the investigator when its invocation is permitted;
otherwise read it directly. When only part of a payload is needed at exact
fidelity, narrow the read at the tool call rather than asking an intermediary
to summarize it.

An investigator assignment MUST name a bounded target that may widen when the
answer requires it, one question answerable as an enumeration or yes/no, and the
required answer shape. Require a conclusion and precise source locator; include a
quotation only when the conclusion needs exact wording. Do not ask for a source
dump, a full summary, further-investigation proposals, or exploration outside
what the question requires. The reader MUST read as much as settling the answer
takes. Callers MUST NOT impose arbitrary read budgets, token caps, or tool-call
caps; unresolved material is reported, not guessed at to fit a budget.

### Record an unavailable role

Report role or tool unavailability and its evidence using
[Loop Engineering's assignment result contract](../../.claude/skills/loop-engineering/references/implementation-package.md).
For an active change loop, also use its
[recoverable run-state contract](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
Distinguish these cases:

- The host cannot discover or invoke the definition.
- The host's tool is absent or ineligible for the assignment.
- The host refuses the spawn.
- No role matches the assignment.
- Assignment material is missing.

A generic worker can qualify only through the same tool and role checks, with an
explicit assignment and boundaries. Otherwise the caller reads or implements
directly where permitted; an unavailable advisory reviewer is recorded rather
than replaced with self-approval. On Claude Code, a `reviewer` that is missing,
or whose spawn the host refuses, is also an unavailable independent-review
route: that gate stays unmet, and no other subagent, provider, or session the
run chooses replaces it; the Claude CI alternative, or a review session the
maintainer instructs, applies only when the maintainer selects it for the run. A
CLI agent or persistent thread MUST NOT be used to bypass a missing role or a
prohibited purpose. For unavailable research, plan, or design review, the driver
MUST use
[Loop Engineering's non-complete human handoff](../../.claude/skills/loop-engineering/references/reviewing-phase-deliverables.md).
The parent retains delivery responsibility, and the independent review follows
the host mapping in the Independent Review section of
[Development Workflow](./independent-review.md#the-independent-review).
