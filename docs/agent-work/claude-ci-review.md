---
type: Procedure
title: Request the alternative Claude CI review
description: Request the workflow review only when selected by the maintainer and identify its eligible output.
---

# Request the alternative Claude CI review

<a id="claude-review-from-claude-code"></a>

## Alternative Claude CI review

Check [the selected provider](./independent-review.md#the-independent-review), [the standing grant](./delivery-grant.md#authorizing-delivery-operations) and the session's [delivery confirmation](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) before posting a review trigger.

A Claude Code run uses this route only when the maintainer explicitly selects it
for that run, under the provider-change rule in [The Independent Review](./independent-review.md#the-independent-review);
the run requests it by posting `@claude review` as a top-level pull request
comment, once per round. The Claude CI review is the in-repository reviewer in
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
for the rationale.

A request from any other author, a missing operator setup, and an unnamed bot
all end without findings. A Claude CI review request that gets no review MUST confirm the
operator setup and, where a bot posted the request, that its identity is named in
`allowed_bots`. Do not read the absence as approval.
