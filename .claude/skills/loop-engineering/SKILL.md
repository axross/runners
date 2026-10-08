---
name: loop-engineering
description: Driving a code change or document update through an approved plan → execution → verification → independent review loop, including resuming or recovering an in-progress run. Owns phase progression, approval revision, evidence, finding state, and recovery meaning—not host execution APIs or delivery storage. Project policy chooses gates and branch rules. Defer to a host project's own more-specific change-loop capability where it ships one. Not for read-only questions, investigations, or reviews that change nothing.
user-invocable: false
---

# Loop Engineering

Drive one change from intake to ready through **plan → approve → execute → verify → independent review → address → ready**. This skill defines what each phase means and what evidence crosses its boundaries. It does not authorize tools, publication, scheduling, or delegation, and it does not prescribe where durable state is stored.

Project policy owns which gates apply, reviewer independence, branch and delivery rules, and storage representations. Where policy is silent, use this skill's existing defaults: a human-approved plan before edits, required verification, mandatory independent review, append-only recovery, and a ready state only after convergence. Where an instruction the launching runtime injected disagrees with a project mandate, this skill states no precedence between them: that belongs in the entry file of the host doing the injecting, which is the only document positioned to see both.

The loop driver is the actor currently responsible for advancing a change against its approved plan and required evidence. When that driver delegates a contribution, it is the parent for that assignment. Launching a separately owned change does not make its launcher that change's driver. Receiving a result does not by itself transfer the loop, human decision authority, or operation grants.

Read-only work that changes nothing does not enter change gates. For change work, delegation to a subagent is the default; the loop driver executes directly only where delegation is unavailable through currently permitted tools or disallowed by host policy. This never makes the driver its own independent reviewer. No actor ranking, named model, scheduler, shared checkout, or live-resume mechanism is required.

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html).

## Phase Progression

Every change advances against current evidence, never a claim inherited from an earlier phase.

The four rules below stand in this file rather than behind a pointer, under the unconditional-scope carve-out an authoring capability's progressive-disclosure rules state, and each is stated only here. Each is unconditional within this skill's scope rather than merely broad, and each is needed before the reader can decide what to open: whether this is change work at all, who owns it, who will execute it, and what an approval does and does not authorize. A pointer to any of the four would fire on every run, costing a read while shaking nothing a direct statement here does not already shake — and the executor rule in particular has nowhere else to land, since the reference that elaborates delegation is skipped precisely by the direct-execution run that most needs to be told driver execution weakens no gate.

Every other phase rule lives in the reference that governs it, including the plan gate and the staleness that follows a changed revision: [plan-document.md](./references/plan-document.md) states both for writing, approving, revising, or comparing a plan.

**Guidelines:**

- MUST treat read-only work as outside the change gates unless it produces a project change.
- MUST identify the loop driver and any contribution responsibilities from the assigned work and established handoff, not from execution ancestry.
- MUST select execution using only currently permitted capabilities and any project host guide, delegating to a subagent as the default choice for change work; driver execution is valid and weakens no gate only when delegation is unavailable or disallowed.
- MUST distinguish plan approval, operation authorization, execution capability, and permitted tool use. Carry forward valid authorization within its original scope, including a verified standing grant a human adopted into project guidance, which is actual authorization; but never infer a grant, or a broader one, from tool availability, project policy that records none, plan approval, or a previous operation.

See [phase-progression.md](./references/phase-progression.md) for:

- anchoring a new change in its tracking target and making its first edit or branch operation, under intake and default gates
- running verification for a changed surface and opening or addressing a delivery target, under project policy and independent review
- choosing the available next action or a legitimate stop when a blocker affects the run rather than one dependent action

See [plan-document.md](./references/plan-document.md) for:

- writing, approving, revising, or comparing a plan
- the canonical plan sections and approval target
- agreeing necessary out-of-tree verification and its evidence destination during planning
- revision identity, amendment approval, and visual choices

## Execution Handoffs

See [implementation-package.md](./references/implementation-package.md) for:

- assigning work or accepting an execution or review result through the five prose-compatible contracts
- repository identity, material fidelity, authorization, and result states

See [subagent-delegation.md](./references/subagent-delegation.md) for:

- handing an assignment to another actor, tasking an investigator, reading its findings, or integrating a returned result, rather than a change the loop driver implements and reads throughout
- direct and delegated execution selection, and the read-routing boundary between a payload the judging actor reads itself, one an investigator returns a conclusion from, and one narrowed at the tool boundary
- the investigator return contract, the compatibility preflight, and child-completion boundaries

See [writer-ownership-and-recovery.md](./references/writer-ownership-and-recovery.md) for:

- coordinating concurrent writing, retrying an effect, or recovering an interrupted run, including plan-change interruption and outcome-unknown recovery
- append-only history and completion-evidence comparison

## Review and Readiness

See [finding-response.md](./references/finding-response.md) for:

- choosing an author-side response to an advisory pre-flight or independent-review finding through feedback judgment and causal correction selection
- the bounded response practice when conduct or development capabilities are absent
- response reasoning distinct from finding disposition and fresh-review input

See [pre-flight-review.md](./references/pre-flight-review.md) for:

- running or recovering an advisory pre-flight review
- advisory review findings, terminal states, deferred human handoffs, dismissal authority, and durable parks
- the initial implementation plus three autonomous review/fix rounds

See [independent-review.md](./references/independent-review.md) for:

- requesting, addressing, or evaluating an independent review, or publishing the ready transition
- mandatory external independence, fresh review rounds, and the four-round address cap
- identifiable posted completion under the project's adopted presentation and reporting scope, without affirmative per-check assertions, distinct from explicit limitations and contributor evidence
- external fixed, human-dismissed, and outstanding dispositions without rewriting reviewer history
- convergence distinct from conversation closure, including substantive metadata changes
- conflict remediation, affected verification, timeout, and ready conditions

## Resume and Reporting

See [resuming-and-handoff.md](./references/resuming-and-handoff.md) for:

- resuming or taking over a run by reconstructing actual state and resuming one pending phase
- handling stale, partial, and unknown outcomes

See [run-state-and-reporting.md](./references/run-state-and-reporting.md) for:

- persisting recoverable semantic run state independent of storage format, including the delegation-permission determination and what it rested on
- evaluating a recovered or standing operation grant against a proposed effect across phases and sessions
- writing the evidence-backed ready-to-merge brief, and reporting a spawned role's model and effort as `verified`, `declared`, or `unknown`

What follows is the rule itself, not a further reading obligation. It binds every turn this loop takes rather than some narrower situation, so it stands here under the same unconditional-scope carve-out [Phase Progression](#phase-progression) invokes, instead of behind a pointer an agent might not yet have opened. An observation and the action it justifies belong in the same turn. A turn that carries text and calls no tool is not a turn taken while the work continues — the run has not stopped, so reporting first and acting next merely splits one turn's cost across two turns instead of doing the work in the one that already had it.

**Guidelines:**

- MUST act, in the same turn, on any observation that has a next step while the run continues — do not end a turn that reports and calls no tool when there is more to do.
- MUST treat exactly three cases as exceptions to that rule rather than violations of it: ending the turn at a required human gate, including a machine event escalated to one; recording state before stopping at a waiting bound; and the completion report that closes the run.
- MUST NOT apply this rule to a progress note posted while an asynchronous machine event is still outstanding — a required check, the independent review, or a delegated actor's run — since that turn defers no next step, none being due until the event resolves.
- MUST NOT read this rule as barring reasoning alongside a tool call: it forbids splitting one turn into a text-only turn followed by the turn that acts, not writing prose in the same turn that also calls a tool.
