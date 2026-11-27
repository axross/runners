---
type: Policy
title: Out-of-tree acceptance evidence
description: Decide when external evidence is required and who can approve changes to its method, destination or timing.
---

# Out-of-tree acceptance evidence

## Acceptance Criteria Verified Out of Tree

Some acceptance criteria can be confirmed only by evidence no diff carries, such
as a machine-specific timing or a check against a real runner host. Prefer an
in-tree check wherever one can confirm the criterion. A plan MUST designate each
such criterion explicitly, by appending the marker
`(verified out of tree: <where the evidence will be published>)` to the end of
the criterion itself. The plan MUST agree the verification method and publication
destination, for example a linked issue comment, before implementation.

The pull request body's Acceptance criteria section MUST carry every designated
criterion verbatim, marker included, with its status and a verified link to the
published evidence. It MUST identify the approved plan revision and account for
any criteria not projected into the body. Evidence publication may remain pending
at draft creation only when the destination requires that draft to exist; the
author MUST publish the evidence, verify its locator, and update the body
immediately afterward, keeping the pull request draft until publication is
complete. Reviewer silence never satisfies this contributor obligation.

Some checks can happen only after merge, such as exercising the merged workflow
on the default branch. A plan MUST identify these as separately tracked
post-merge follow-up obligations, with the maintainer's approval of their timing
and evidence destination, not as fulfilled pre-ready criteria. Required
pre-ready evidence cannot be deferred this way without an approved plan
revision; existing approved plans are not silently reclassified. The follow-up
remains pending until its evidence is published at the agreed destination.

Changing a designation, method, destination, or required timing needs an approved
plan revision. [REVIEW.md](../../REVIEW.md) owns the code-inspection finding
boundary; these author-side duties do not turn missing external evidence alone
into a review finding.
