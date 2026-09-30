# Independent Review and Readiness

Apply this reference after verified implementation and whenever external review or required checks return. Project policy chooses the reviewer and delivery mechanism; where it is silent, independent review remains mandatory.

## Independence

The author cannot certify its own work. Independence must come from the reviewer arrangement project policy requires, not from a label applied inside the authoring context.

**Guidelines:**

- MUST NOT self-certify independent review.
- MUST treat an unavailable advisory review as no waiver of mandatory external review.
- MUST request a fresh independent review after every fix batch using the project's independent-review input policy; the advisory stage's private ledger restrictions do not redefine the external review's policy.
- MUST treat missing required review material as a non-complete review result, never as a clean verdict.

## Addressing Findings

Address every blocking finding and unmet acceptance criterion, preserving finding identity and evidence between rounds. A finding that changes the approved plan returns the run to plan revision and fresh approval.

**Guidelines:**

- MUST preserve finding IDs, severities, citations, dispositions, and their fixing commits under the posted-review policy.
- MUST rerun affected verification after fixes and obtain a fresh review of the resulting content.
- MUST surface ambiguous product or architecture findings to the human rather than guessing.

## Mergeability and Conflict Remediation

Mergeability is a ready-gate prerequisite and a transition to remediation when
it fails. The project chooses how its branch incorporates the base; Loop keeps
the distinction between mechanical repair and a decision that changes intent.

**Guidelines:**

- MUST restore the delivery target to a mergeable state under project branch policy before treating the ready gate as satisfied.
- MUST resolve mechanical conflicts within the approved scope, including independent or adjacent edits and reproducible generated artifacts, but return intentional competing changes to the human when reconciling them requires product or architecture judgment; a resulting plan change follows plan revision and reapproval.
- MUST rerun the verification required by every surface touched while incorporating the base or resolving conflicts, and record the resulting evidence.
- MUST request fresh independent review after known conflict-resolution or other material-fix changes, record the material the reviewer actually covered, and never claim an earlier review covered those edits.
- MUST NOT turn that operational re-review rule into universal exact-head equality, automatic invalidation on every concurrent update, or a material-bound operation grant when the selected provider and project policy explicitly accept a completion race.

## External Round Cap

The post-delivery address/review loop allows **4** rounds. After non-convergence, record unresolved findings and checks and return a decision-waiting result.

**Guidelines:**

- MUST stop autonomous addressing after four rounds and report the unresolved state.
- MUST NOT read the cap as authorization to publish, schedule, or trigger another review.

## Waiting Bound

Waiting is host execution, not loop semantics. The semantic bound remains the awaited work's declared timeout plus a margin, or **2 hours** where no timeout is declared. The budget resets when a new result arrives and a new push or equivalent revision starts fresh checks.

**Guidelines:**

- MUST report a still-pending result as partial or unavailable when the bound expires.
- MUST NOT require polling, subscriptions, schedulers, cache-TTL measurements, or a particular resume mechanism.
- MUST NOT infer scheduling permission from the existence of this bound.
- MUST stop autonomous waiting at that bound, preserving recovery evidence; human waits end the turn and are never polled.
- MUST resolve which wait mechanism the session actually has before the first wake, rather than tuning an interval against a mechanism it turns out not to expose, and record when only one of delivery and a scheduled wake is available — a success transition missed by delivery alone strands the run rather than merely delaying it.
- MUST scope any permitted wait mechanism to this tail, and MUST tear down every mechanism it armed in the same turn as the stop, at each of the three stops: the ready transition, non-convergence at the round cap, and the waiting bound. This holds with no exception, including where follow-up work on the same change is already anticipated — a mechanism left armed past the tail wakes the run on unrelated activity it can do nothing with, and never on the later human comment a resume carries instead.

## Ready Gate

A change is ready only when all required checks are green, mandatory independent review is clean, the implemented plan revision is still approved, all required evidence is present, and the delivery target is mergeable under project policy.

**Guidelines:**

- MUST publish the ready transition as soon as every condition above is satisfied; the gate itself is sufficient warrant for that one effect and needs no separate operation grant. This authority does not extend to merge, release, deployment, scheduling, or any other unnamed effect, which stay with the human under project policy.
- MUST keep the change not ready while any condition above is unknown, unavailable, stale, or failed.
- MUST re-enter review when later human feedback changes delivered content.
