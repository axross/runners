---
name: loop-engineering
description: Driving a code change or document update through an approved plan → execution → verification → independent review loop, including resuming or recovering an in-progress run. Owns phase progression, approval revision, evidence, finding state, and recovery meaning—not host execution APIs or delivery storage. Project policy chooses gates and branch rules. Defer to a host project's own more-specific change-loop capability where it ships one. Not for read-only questions, investigations, or reviews that change nothing.
user-invocable: false
---

# Loop Engineering

Drive one change from intake to ready through **plan → approve → execute → verify → independent review → address → ready**. This skill defines what each phase means and what evidence crosses its boundaries. It does not authorize tools, publication, scheduling, or delegation, and it does not prescribe where durable state is stored.

Project policy owns which gates apply, reviewer independence, branch and delivery rules, and storage representations. Where policy is silent, use this skill's existing defaults: a human-approved plan before edits, required verification, mandatory independent review, append-only recovery, and a ready state only after convergence. Where an instruction the launching runtime injected disagrees with a project mandate, this skill states no precedence between them: that belongs in the entry file of the host doing the injecting, which is the only document positioned to see both.

Read-only work that changes nothing does not enter change gates. For change work, delegation to a subagent is the default; the parent executes directly only where delegation is unavailable through currently permitted tools or disallowed by host policy. This never makes the parent its own independent reviewer. No actor ranking, named model, scheduler, shared checkout, or live-resume mechanism is required.

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html).

## Phase Progression

Every change advances against current evidence, never a claim inherited from an earlier phase.

The three rules below stand in this file rather than behind a pointer, under the unconditional-scope carve-out an authoring capability's progressive-disclosure rules state, and each is stated only here. Each is unconditional within this skill's scope rather than merely broad, and each is needed before the reader can decide what to open: whether this is change work at all, who will execute it, and what an approval does and does not authorize. A pointer to any of the three would fire on every run, costing a read while shaking nothing a direct statement here does not already shake — and the executor rule in particular has nowhere else to land, since the reference that elaborates delegation is skipped precisely by the parent-only run that most needs to be told parent execution weakens no gate.

Every other phase rule lives in the reference that governs it, including the plan gate and the staleness that follows a changed revision: [plan-document.md](./references/plan-document.md) states both, and the obligation below to read it fires whenever a plan is written, approved, revised, or compared.

**Guidelines:**

- MUST treat read-only work as outside the change gates unless it produces a project change.
- MUST select execution using only currently permitted capabilities and any project host guide, delegating to a subagent as the default choice for change work; parent execution is valid and weakens no gate only when delegation is unavailable or disallowed.
- MUST distinguish plan approval, operation authorization, execution capability, and permitted tool use. Carry forward valid authorization within its original scope, including a verified standing grant a human adopted into project guidance, which is actual authorization; but never infer a grant, or a broader one, from tool availability, project policy that records none, plan approval, or a previous operation.

See [phase-progression.md](./references/phase-progression.md) for:

- intake, default gates, available-next-action progression, and legitimate stops
- project policy, independent review, and blocked delivery

**Guidelines:**

- MUST read [phase-progression.md](./references/phase-progression.md) before anchoring a new change in its tracking target, making the first edit or branch operation for it, running the verification a changed surface requires, opening or addressing a delivery target, or judging that a blocker stops the run rather than one dependent action.

See [plan-document.md](./references/plan-document.md) for:

- the canonical plan sections and approval target
- revision identity, amendment approval, and visual choices

**Guidelines:**

- MUST read [plan-document.md](./references/plan-document.md) before writing, approving, revising, or comparing a plan.

## Execution Handoffs

See [implementation-package.md](./references/implementation-package.md) for:

- the five prose-compatible contracts
- repository identity, material fidelity, authorization, and result states

**Guidelines:**

- MUST read [implementation-package.md](./references/implementation-package.md) before assigning work or accepting an execution or review result.

See [subagent-delegation.md](./references/subagent-delegation.md) for:

- direct and delegated execution selection, and the read-routing boundary between a payload the parent reads itself, one an investigator returns a conclusion from, and one narrowed at the tool boundary
- the investigator return contract, the compatibility preflight, and child-completion boundaries

**Guidelines:**

- MUST read [subagent-delegation.md](./references/subagent-delegation.md) before handing an assignment to another actor, before tasking an investigator or reading back what one returns, and before integrating a returned result; a change the parent implements and reads throughout does not need it.

See [writer-ownership-and-recovery.md](./references/writer-ownership-and-recovery.md) for:

- writing coordination, plan-change interruption, retries, and outcome-unknown recovery
- append-only history and completion-evidence comparison

**Guidelines:**

- MUST read [writer-ownership-and-recovery.md](./references/writer-ownership-and-recovery.md) before concurrent writing, retrying an effect, or recovering an interrupted run.

## Review and Readiness

See [pre-flight-review.md](./references/pre-flight-review.md) for:

- advisory review findings, terminal states, deferred human handoffs, dismissal authority, and durable parks
- the initial implementation plus three autonomous review/fix rounds

**Guidelines:**

- MUST read [pre-flight-review.md](./references/pre-flight-review.md) before running or recovering an advisory pre-flight review.

See [independent-review.md](./references/independent-review.md) for:

- mandatory external independence, fresh review rounds, and the four-round address cap
- conflict remediation, affected verification, timeout, and ready conditions

**Guidelines:**

- MUST read [independent-review.md](./references/independent-review.md) before requesting, addressing, or evaluating an independent review, or publishing the ready transition.

## Resume and Reporting

See [resuming-and-handoff.md](./references/resuming-and-handoff.md) for:

- reconstructing actual state and resuming one pending phase
- handling stale, partial, and unknown outcomes

**Guidelines:**

- MUST read [resuming-and-handoff.md](./references/resuming-and-handoff.md) before resuming or taking over a run.

See [run-state-and-reporting.md](./references/run-state-and-reporting.md) for:

- semantic run state independent of storage format, including the delegation-permission determination and what it rested on
- scoped operation grants across phases and sessions
- evidence and ready-to-merge reporting, and classifying a spawned role's model and effort as `verified`, `declared`, or `unknown`

**Guidelines:**

- MUST read [run-state-and-reporting.md](./references/run-state-and-reporting.md) before persisting recoverable state, evaluating a recovered or standing operation grant against a proposed effect, writing the ready-to-merge brief, or reporting what model and effort a spawned role ran at.

What follows is the rule itself, not a further reading obligation. It binds every turn this loop takes rather than some narrower situation, so it stands here under the same unconditional-scope carve-out [Phase Progression](#phase-progression) invokes, instead of behind a pointer an agent might not yet have opened. An observation and the action it justifies belong in the same turn. A turn that carries text and calls no tool is not a turn taken while the work continues — the run has not stopped, so reporting first and acting next merely splits one turn's cost across two turns instead of doing the work in the one that already had it.

**Guidelines:**

- MUST act, in the same turn, on any observation that has a next step while the run continues — do not end a turn that reports and calls no tool when there is more to do.
- MUST treat exactly three cases as exceptions to that rule rather than violations of it: ending the turn at a required human gate, including a machine event escalated to one; recording state before stopping at a waiting bound; and the completion report that closes the run.
- MUST NOT apply this rule to a progress note posted while an asynchronous machine event is still outstanding — a required check, the independent review, or a delegated actor's run — since that turn defers no next step, none being due until the event resolves.
- MUST NOT read this rule as barring reasoning alongside a tool call: it forbids splitting one turn into a text-only turn followed by the turn that acts, not writing prose in the same turn that also calls a tool.
