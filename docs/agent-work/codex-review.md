---
type: Procedure
title: Request the Codex review
description: Request the review for a Codex or manual change and qualify the outcome of the external service.
---

# Request the Codex review

## Codex review from Codex or a manual change

Start from [the provider selection and review gate](./independent-review.md#the-post-push-review). For an agent-authored change, verify that [the standing grant](./delivery-grant.md#authorizing-delivery-operations) covers the request and that the host permits the write. For a manual change, record the authoring route in the pull request description.

The Codex route is the external Codex GitHub App, not an in-repository workflow.
The root [`AGENTS.md`](../../AGENTS.md) `Code Review Rules` section instructs
Codex to read and apply [`REVIEW.md`](../../REVIEW.md). A maintainer MUST connect
this repository to Codex and enable Code review in Codex settings before the
route can run; this repository stores no Codex workflow or review secret.

This route uses explicit review requests, not automatic event-based reviews.
Before a request or the ready transition, a run MUST confirm that effective
**Automatic review** is off for the repository, including any inherited personal
preference. If it is on or unknown, block that action and ask the maintainer to
confirm the setup; the standing grant does not authorize changing it. This
prevents pull request events from starting extra billable reviews outside the
four-round ledger. See the
[Codex GitHub integration guide](https://learn.chatgpt.com/codex/third-party/github#enable-automatic-reviews).
If an automatic review is observed, count it against the four-round cap, record
its trigger, revision, and result in the round ledger, and do not add a duplicate
manual request.

Codex documents loading applicable `AGENTS.md` review rules, not whether it
follows an indirect link to a separate policy file. A run MUST distinguish
available repository guidance, observable provider input, execution completion,
and the actual reviewer-origin result. Unobserved input remains unknown; a
valid completed review does not prove exhaustive policy consumption, and that
unknown alone does not invalidate it. [`REVIEW.md`](../../REVIEW.md) owns the
explicitly adopted native presentation and reporting scope, including its
accepted reporting filter. Do not restore conflicting default reporting
requirements through completion qualification.

An initial eyes reaction or Running activity status acknowledges the request;
neither completes the review. A run MUST correlate the request, reviewed
revision, terminal App status, and actual final result before evaluating the
review gate. Completed status proves the round ended, not that it found nothing:
read any posted review and inline findings. The App's native no-findings
response or final thumbs-up reaction on the request can confirm a clean round
without a separate review object. Acknowledgment without a terminal result
remains pending, and the App's result is a human wait under [Waiting and Watching](./independent-review.md#waiting-and-watching). On resume the run reads the App's status and any posted result.
If the round has no acknowledgment or terminal result once
[Loop Engineering's waiting bound](../../.claude/skills/loop-engineering/references/post-push-review.md#waiting-bound)
has passed, the run MUST confirm the repository setup and keep the review gate
blocked rather than read silence as a clean review.

A run MUST assess the correlated result under REVIEW.md's adopted arrangement,
not demand affirmative per-check assertions or an extra round solely for them.
Review completion is not convergence or readiness. Actual findings retain
their identity, priority, citations, and required dispositions under Loop
Engineering; explicitly unchecked required material, unmet approved criteria,
missing contributor evidence, failed checks, and material fixes lacking fresh
review still prevent readiness. Native scope never dismisses an actual finding.
