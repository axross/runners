# Phase Progression

Apply this reference to a change, not a read-only question, investigation, or review. Project policy chooses gates; execution and delivery mechanisms realize them without changing their meaning. A missing adapter is not a missing contract: use the host's published, permitted tools directly, or identify the particular capability or authorization that prevents the next transition.

## Intake and Defaults

A named tracking target enters planning; an existing delivery target enters recovery and addressing. A free-form change first needs a durable tracking target. The tracking target is the project's own—an issue in the repository's forge by default, or whichever tracker project policy names instead—and the loop needs to know only that it is durable and holds the plan. These defaults preserve the loop's tracking target → plan → draft delivery path when a project supplies no more-specific change policy. They are not permission to make an external write.

**Guidelines:**

- MUST identify the change target and applicable project policy before planning; by default anchor a new change in its durable tracking target before planning or edits, and reconstruct an existing delivery target rather than restarting it.
- MUST record the plan in that durable target before requesting approval; a private local plan-mode artifact alone does not satisfy this gate.
- MUST surface an authorization or capability blocker when a required delivery operation cannot be performed; never silently replace a mandatory target or gate with an easier one.
- MUST defer to a host project's own more-specific change-loop capability wherever it ships one: that capability owns the loop there and this one does not run beside it, which is a different thing from the project policy above that merely chooses this loop's gates.
- MUST use the default independent-review arrangement where project policy is silent: a separate session on separate infrastructure, under a bot identity distinct from the connected operator. A review produced inside the authoring session is self-review, regardless of its name.

## Advance or Stop

A completed phase is evidence for the next transition, not a reason to return
control to the human. Loop owns whether work advances; the host owns the tool,
wait, or return mechanism that realizes that outcome.

**Guidelines:**

- MUST enter the next required phase or perform its next action without asking for another instruction when current evidence establishes its prerequisites, a permitted route can perform it, and every effect that requires human authorization is covered by a matching operation grant. A matching standing grant counts as a matching operation grant under [Scoped Operation Grants](./run-state-and-reporting.md#scoped-operation-grants).
- MUST continue independent available work when another action is blocked by a required human decision, an unmatched operation grant, an unavailable capability or required input, an unsafe failed or outcome-unknown effect, or an outstanding machine result; stop only the work that depends on that blocker.
- MUST stop the run only when no required action remains available because of one of those blockers, or when an applicable execution or review bound, non-convergence, or completion applies; record the specific result state and resumable next action.
- MUST end the turn for a human wait; for an actual pending machine result, use only a permitted wait mechanism within the applicable bound. The turn-boundary rule separating a progress note from a deferred available action is stated in [SKILL.md](../SKILL.md) under its carve-out, and is not restated here.
- MUST preserve the phase, attempts, review round, grants, material revision, and pending or unknown effects across interruption so recovery resumes one established transition rather than restarting or skipping ahead.
- MUST NOT treat evidence that the ready gate is satisfied as authorization to merge, schedule, or perform any other unnamed effect. Publishing the ready transition itself is governed by [independent-review.md](./independent-review.md)'s ready gate, not by this operation-grant rule: once that gate is satisfied, publishing is the required next action rather than a blocked one.

## Execute and Verify

The approval binds the outcome; the assignment binds the execution. Parent and child implementations carry the same verification burden. The project's software-development and specialist capabilities own how to implement and test, and its code-review and QA capabilities own diff judgment and evidence adequacy.

**Guidelines:**

- MUST implement only the current approved plan, preserve unrelated changes, and resolve the execution arrangement before the first edit.
- MUST run the required formatting, lint, type, test, and surface-specific checks using documented project commands; disclose every failed or skipped check and its residual risk.
- MUST perform a reviewer-mode self-check against the request and actual diff before delivery, fixing obvious Critical or Major problems; a child's self-check is checked against its receipt and actual state, not treated as independent review.
- MUST perform the advisory pre-flight stage by default after verified initial implementation, regardless of the implementation actor, when a compatible reader is permitted and available; when host conditions or missing material rule it out, record the exact unavailable reason, never report a clean result, and preserve the external gate.
- MUST preserve branch history, work outside the default branch, and refrain from pushing to the default branch or merging the change under the default policy. A child may not publish merely because it owns implementation.

## Deliver and Address

Delivery defaults to a draft target linked to its tracking target. Delivery owns its template, assignment, comment routing, trigger and persistence representation; the loop owns whether the evidence permits progress.

**Guidelines:**

- MUST open delivery in draft by default and include the approved scope, the acceptance-criteria projection below, every skipped required check with its residual risk, and recovery information; request the project's required independent review through a permitted route.
- MUST carry, in the delivery target's own body, every approved acceptance criterion a reviewer can confirm or refute from the change itself—quoted verbatim, each with its status—together with the number of criteria not carried, a locator for the tracking target holding them, and the plan revision the projection was taken from, and MUST NOT require a reviewer to read the tracking target in order to review. Quote rather than restate: a reviewer reading only the delivery target cannot tell a softened criterion from the original. A reviewer that cannot reach the tracking target reviews against the projection alone, and an absent or empty projection is the author's defect rather than that reviewer's limitation. The tracking target stays canonical; the projection never becomes what approval binds to.
- MUST address blocking findings and unmet acceptance criteria on the same delivery target, correlate every fixed finding with its fixing commit, rerun affected checks, and obtain fresh review after fixes.
- MUST preserve human decisions already settled in the plan rather than re-offering them as open choices to a reviewer.
- MUST keep mandatory review marked unmet when it cannot be obtained; no self-check, advisory review, or unavailable reviewer certifies readiness.
- MUST move to ready as soon as [independent-review.md](./independent-review.md)'s ready gate is satisfied, report the evidence, and leave merging to the human under the default policy.
- MUST return to addressing after later human feedback, restoring draft status when necessary and requiring fresh review of changed content.
