---
type: Procedure
title: Run an Amp review round
description: Start a fresh child thread, verify its review and end the thread under the host conditions.
---

# Run an Amp review round

## Amp review child thread from Amp

Select the provider under [the independent review gate](./independent-review.md#the-independent-review). Before creating the thread, check [the scope of the standing grant](./delivery-grant.md#authorizing-delivery-operations) and the session's [archive confirmation or recorded refusal](./host-permissions.md#confirming-review-thread-cleanup-at-the-plan-gate-in-amp).

The Amp route follows
[Loop Engineering's review workflow](../../.claude/skills/loop-engineering/references/review-workflow.md)
with this project's choices. Each round runs in a new Amp thread that the run
starts and owns; no thread is reused across rounds, and the child is not a
handoff of the authoring thread.

1. Start the child with a neutral assignment that names the repository, the
   pull request, the reviewed head commit, [`REVIEW.md`](../../REVIEW.md), and
   the pull request body's acceptance criteria. Beyond what Loop Engineering's
   review workflow excludes, carry none of the author's claimed verdicts. Tell
   the child to read the actual change and surrounding code, not to edit, push,
   or delegate, to post its review itself, and to report the review's URL, the
   reviewed commit, which of those limits its host enforced, and any
   limitations.
2. Right after the child starts, checkpoint its thread URL and round in run
   state, through [Save and resume run state](./run-state.md#save-and-resume-run-state).
3. The child posts its review on the pull request in the form REVIEW.md's Amp
   arrangement sets.
4. Wait for the child as Agent Sessions' Waiting on CI and Review describes,
   within
   [Loop Engineering's waiting bound](../../.claude/skills/loop-engineering/references/independent-review.md#waiting-bound).
   Then read the posted review back, confirm that it is submitted rather than
   pending, and correlate it with the round's child thread and the reviewed
   commit. Record the child's report of which limits its host enforced in run
   state. [Review coverage evidence](./independent-review.md#review-coverage-evidence) applies to the result.
5. Archive the child thread once its result is verified, preserving its
   history, and confirm the archive, under the plan-gate archive confirmation
   Agent Sessions describes. Record the review's URL and the archive status in
   run state.

A child that cannot post, a review that does not appear, and a review that does
not correlate are failures of the selected route, which block the review gate as
[The Independent Review](./independent-review.md#the-independent-review) states. Record the incomplete evidence, then,
once the child has settled or been stopped, archive that round's child too when
the plan-gate archive confirmation allows it.

A child left unarchived does not invalidate a verified review. When the
maintainer declined the plan-gate archive confirmation, the archive fails, or a
failed round's child is still working when the run stops, give the maintainer
the thread's URL, the reason it remains, and the steps: open it on ampcode.com
and choose **Archive** in its More Actions menu, or run `amp threads archive`
with its thread ID. Report the remaining thread. On
resume, check what each recorded child posted and whether it is archived before
starting another child or counting another round.
