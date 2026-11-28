---
type: Procedure
title: Request and verify the Claude CI review
description: Request one Claude CI review round and verify its workflow, reviewer output and head commit.
---

<a id="request-the-alternative-claude-ci-review"></a>

# Request and verify the Claude CI review

<a id="claude-review-from-claude-code"></a>

<a id="alternative-claude-ci-review"></a>

## Claude CI review from Claude Code

Check [the selected provider](./independent-review.md#the-post-push-review), [the standing grant](./delivery-grant.md#authorizing-delivery-operations) and the session's [delivery confirmation](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) before posting a review trigger.

The Claude CI review is the in-repository reviewer in
[`claude-review.yaml`](../../.github/workflows/claude-review.yaml), which runs on
a GitHub-hosted runner and applies [`REVIEW.md`](../../REVIEW.md) through its
system prompt. It is inert until a one-time operator setup is done, and its
silence is indistinguishable from a clean review: it needs the
[Claude GitHub App](https://github.com/apps/claude) installed and a
`CLAUDE_CODE_OAUTH_TOKEN` repository secret, both added by the maintainer. Past that setup, its
author-association gate answers repository owners, members, and collaborators,
plus the change loop's own bot identity (`claude[bot]`), and only when that
comment carries no Markdown heading anywhere in it. That heading test normally
does not exclude the reviewer's own summary, which opens with a tally line, so the
guard against the summary re-triggering it is that the reviewer never writes the
trigger phrase into a summary or comment, per [REVIEW.md](../../REVIEW.md). The action also rejects any run a bot identity starts
unless that identity is named in its own `allowed_bots` input, which
`claude-review.yaml` does for `claude[bot]`. See
[Security](../security/workflow-permissions.md#the-review-gate-admits-one-bot-identity-by-login-not-association)
for the rationale. A request from any other author, a missing operator setup,
and an unnamed bot all end without findings.

Each round follows these rules:

- **Request.** The round's request is the Claude review trigger phrase,
  `@claude review`, written in exactly one top-level pull request comment for
  that round and nowhere else. When `claude[bot]` posts it, the comment carries
  no Markdown heading and nowhere the sequence of two number signs and a space,
  because the workflow's `!contains(body, '## ')` gate skips a bot comment that
  contains it.
- **No overlap.** The workflow's concurrency group cancels an in-flight review
  when a new request arrives on the same pull request. A run MUST NOT post a new
  request while its previous round is pending.
- **Reviewer.** The reviewer is always `claude[bot]`. It shares an identity with
  the requester only when `claude[bot]` posted the request.
- **Result.** A round's result is a `claude[bot]` comment posted after the
  request that contains the tally line [REVIEW.md](../../REVIEW.md) requires,
  together with any `claude[bot]` inline findings. The summary names neither
  commit nor round, so the tally comment and the inline findings are tied to the
  round by the Head and timing rule, below.
- **Failed rounds.** A cancelled, failed, or unfinished workflow run, a run the
  workflow's gate skipped, and inline findings with no tally summary are failed
  rounds. A workflow run without a conclusion is pending until the waiting bound
  has passed since the request, which is the job's `timeout-minutes` in
  `claude-review.yaml` plus [Loop Engineering's margin](../../.claude/skills/loop-engineering/references/post-push-review.md#waiting-bound); after that it is
  unfinished. A workflow run that concluded as a success does not by itself show
  that a review was posted.
- **Detection.** On each wake or resume, read the workflow run the request
  comment triggered and the comments posted after the request, then classify the
  round from both. That run is the `issue_comment` run created at or just after
  the request comment's timestamp, not the latest run in the repository. Runs
  that other comments start, including the reviewer's tally comment, are not the
  round's run and are ignored.
- **Gate.** A failed, silent, or unavailable round blocks the review gate, and
  no other provider substitutes for it. A request that gets no review MUST lead
  the run to confirm the operator setup and, where `claude[bot]` posted the
  request, that its identity is named in `allowed_bots`. Do not read the absence
  as approval.
- **Head and timing.** A round covers the pull request head that was current
  when its request was posted, which the run records with the round. Its inline
  findings MUST carry that head, or a head pushed while the round was pending, as
  their `original_commit_id`. A summary-only result is tied to its round by being
  posted after the round's request and before any later request. This project
  accepts a completion race for this route: a head move while a round is pending
  does not invalidate that round. A later change needs a new request only where
  Loop Engineering's
  [Start and Rounds](../../.claude/skills/loop-engineering/references/post-push-review.md#start-and-rounds),
  [Mergeability and Conflict Remediation](../../.claude/skills/loop-engineering/references/post-push-review.md#mergeability-and-conflict-remediation),
  or [Ready Gate](../../.claude/skills/loop-engineering/references/post-push-review.md#ready-gate) requires a fresh
  post-push round for it, whether it was pushed while a round was pending or
  after, and no earlier round is claimed to cover it.
- **Cap.** Every request counts against the four-round cap, including a failed
  round. After a failed round, the run may post a new request once the cause is
  addressed, and that request counts toward the cap.

## What a Claude CI round reads

A round reads three things:

- the pull request's diff, through `gh pr diff`;
- the default branch's checkout, which shows files as they were before the
  pull request;
- head files, through [`read-head.sh`](../../.github/scripts/read-head.sh), which the
  workflow installs where the reviewer cannot change it. It prints one file, a line
  range, or a directory listing at the head commit the reviewer names. The
  [Security](../security/workflow-permissions.md#the-review-job-reads-head-files-only-through-a-fixed-command)
  page says how it is protected.

There is no head-wide search, so a question across the whole head stays a long
scan of single reads or an unchecked item. The workflow runs from the default
branch, so a pull request that changes it does not change its own round.

A round's report of unchecked scope sorts under
[Review Completion Evidence](../../.claude/skills/loop-engineering/references/post-push-review.md#review-completion-evidence)
as follows:

- **A head file.** Reachable when the command can read it, so a report that
  names one as unchecked is a finding about the round, not a limit of the route.
  A path with a character outside ASCII letters, digits, `.`, `_`, `-` and `/`,
  or over 1,024 characters, is refused, and a fork pull request's head may not
  be served; both are unreachable through this route, and the reviewer reports
  them as unchecked.
- **An earlier round's threads.** Outside required scope, unless the assignment
  asks the reviewer to read them.
- **Material outside the repository.** Unreachable, unless a plan designates an
  out-of-tree check for it.
