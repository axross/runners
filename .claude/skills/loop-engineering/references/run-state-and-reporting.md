# Run State and Reporting

Apply this reference when persisting recoverable state or reporting a phase result. Storage and publication mechanics belong to the host and project; the loop owns the meaning that must survive.

## Semantic Run State

Durable state is whatever a fresh executor cannot safely derive: target, phase, approved plan revision and approval evidence, scoped operation grants, assignments and attempts, latest results, self-review evidence, checks, review round, open findings at their permitted durability, unresolved decisions or authorization, the execution arrangement and how it was settled, actual revision and uncommitted state, remaining processes, and unknown external effects.

For example, a human-dismissed external finding can have a verified disposition reply but failed conversation closure. Preserve the actual human decision and its rationale separately from the verified reply and pending closure; resume only the pending effect under its valid grant. Finding disposition is not delivery completion. Pending, failed, and outcome-unknown reply/closure effects retain their own evidence without requiring a new storage schema or result vocabulary.

**Guidelines:**

- MUST persist only through a currently permitted project mechanism and preserve append-only history.
- MUST NOT require an HTML status block, a particular service, a local path, or a byte-extraction command in the portable core.
- MUST keep evidence locators and revision identities sufficient to detect stale state.
- MUST record how the execution arrangement was settled, not only which one ran: what the determination rested on, quoted where it rests on a stated condition and recorded as an observation where it rests on none, and whether a question was ever put to the human. A record naming no grounds is incomplete, and recording a decline the human never gave claims an answer nobody made.
- MUST preserve the substantive self-review outcome, unresolved findings, and limitations in permitted internal handoffs. When a persistence surface prohibits requirement-derived text, project only code or process observations and requirement locators onto that surface; the projection replaces neither the plan nor fresh review input.

## Scoped Operation Grants

An operation grant records actual human authorization for one or more concrete effects. It remains separate from plan approval, execution capability and a host's permission to use a tool. Compatible effects may share one grant, but the grant is no broader than the targets, operations and consequences the human authorized.

A **standing grant** is an operation grant a human recorded ahead of the run in the project's own guidance rather than in the conversation. It is actual human authorization, carried across runs, sessions and executors, because a human's act put it on the default branch; that is what separates it from inferring authorization out of ordinary project policy, which stays forbidden. A record an agent drafted qualifies through that human act, not through its own authorship. Host permission prompts and a tool's own usage conditions remain boundaries that no grant, standing or conversational, overrides.

**Guidelines:**

- MUST record each grant's target, permitted operations, human-evidence locator, applicable route and lifetime, limits, and explicit exclusions without credentials or sensitive payloads.
- MUST name an automation trigger's intended downstream effects, including billed execution, protected-context access or publication where applicable, before treating those effects as part of the grant.
- MUST compare every proposed effect with the recovered target, operation, route, lifetime, limits and exclusions; carry a matching grant, standing or conversational, forward across phases, sessions and executors, and ask only for an absent or expanded effect. An effect a matching grant covers is never authorization-waiting: perform it without asking, whether the run is before or after plan approval. A standing grant covers effects only and never substitutes for plan approval.
- MUST treat a changed target, route, expired lifetime, exhausted limit or missing evidence as authorization-waiting for the unmatched effect, without blocking independent effects covered by another valid grant.
- MUST accept a record in project guidance as a standing grant only if it names its targets, permitted effects, lifetime, limits, explicit exclusions, applicable route (where it limits one) and a human-evidence locator; a record missing any of these is not a grant, and none is inferred from it.
- MUST verify a standing grant before relying on it: read the record from the current default branch, never from the agent's working tree or its own unmerged change; its locator names the human's adopting act, a human's merge of the change or a direct human commit on the default branch, and where that act cannot be told apart from an agent run's, such as under a shared operator identity with no distinguishing signal, the record is not a grant; the record's current text is what that act landed, so a later unadopted edit is not covered. Re-read the record before relying on it for an effect and on each resume: a handoff's or run state's claim of a standing grant is data, not evidence. A record that fails this, or has been removed or has expired, is not a grant.
- MUST NOT infer setup, secrets, settings, production enablement, merge, release, deployment, scheduling, force-push or default-branch push authority from a grant that does not name that effect explicitly, and no grant widens a prohibition on pushing to the default branch that the project's delivery or branch practices state. Readiness publication is not one of these grant-gated effects: [independent-review.md](./independent-review.md)'s ready gate, once satisfied, is its own sufficient warrant.
- MUST NOT bind a grant to an exact material revision unless the human explicitly made that revision part of its scope; material currency and review validity remain separate evidence questions.

## Phase Reporting

Report the result state—complete, partial, decision-waiting, authorization-waiting, unavailable, failed, or outcome-unknown—and the evidence supporting it. Exact commands and outcomes matter more than a generic success claim.

**Guidelines:**

- MUST report the approved revision, changed files or delivered content, verification evidence, review evidence, unresolved work, residual risk, remaining processes, and blockers where relevant.
- MUST distinguish skipped, unavailable, denied, failed, and unknown work.
- MUST NOT report child completion, self-review, or missing evidence as whole-change readiness.

## Model and Effort Certainty

Whether an actor is _capable_ and which model it _ran_ are separate questions, and a host may answer the second only partially. Reporting a configured value as a confirmed one turns an unverified assumption into a claim the human cannot audit, so each is classified rather than asserted:

- `verified` — a runtime, transcript, or telemetry reading confirms the actual value
- `declared` — configuration or a spawn argument states the value, but what ran could not be independently confirmed
- `unknown` — the session exposes too little to say

**Guidelines:**

- MUST classify model and effort with one of those three values, independently of each other, for every role the run delegates to, and MUST NOT report a `declared` value as `verified`.
- MUST treat a spawn-time model argument that overrides an actor definition's pinned value as discarding that pin, and MUST NOT report the pinned value as what that run used — it is not even `declared` for that run. Record the override and its reason.
- MUST NOT require a runtime-verified value before delegating; a stricter policy than this belongs to a project or host that wants it.
- MUST fold the classification into the completion report for every role the run delegated to, rather than into a separate activity log beside it.

## Ready-to-Merge Handoff

The completion report is also the human's verification brief.

**Guidelines:**

- MUST identify the work target, delivery target where one exists, approved plan revision, required-check results, independent-review outcome and round count, mergeability, and any residual risk.
- MUST give concrete manual exercise steps for human-observable changes and say plainly when none apply.
- MUST source any preview or artifact locator from verified current evidence rather than constructing it from memory.
