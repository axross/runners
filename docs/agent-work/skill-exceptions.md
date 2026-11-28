---
type: Policy
title: Skill gaps and adopted deviations
description: Identify the authority, scope, evidence and reassessment conditions for project-specific exceptions.
---

# Skill gaps and adopted deviations

## Deviations and Gaps

Two different things route here, and they resolve the same way. A **deviation**
is a collision: an installed capability requires one thing, this project
proposes or has adopted a bounded alternative. A **gap** is an installed capability being wrong,
outdated, or simply silent on a case that comes up here, or a capability the
library does not have at all. Either way the installed skill is left exactly as
it is, and the resolution is written down in this document.

That matters because an unrecorded deviation reads to the next agent, and to a
reviewer, as a plain violation of a MUST rule, and an unrecorded gap gets
rediscovered from scratch by whoever hits it next.

A suspected gap MUST be verified against the actual host-selected skill's text
and source before being routed anywhere; a rule that already covers the case is
not a defect to file. When that source cannot be established, report routing as
blocked rather than infer it from this project's lockfile. A real gap is then
resolved by one or both of two routes: an issue opened on
[`axross/skills`](https://github.com/axross/skills) when the gap generalizes
beyond this project, and a written note in the register below saying what the
capability states, what this project does instead, and how to handle the case
meanwhile. The human's go-ahead MUST be obtained before opening an upstream
issue or posting feedback on an existing one, since it is a public write beyond
this repository's delivery grant, and
the gap MUST be recorded locally in the meantime rather than leaving the finding
to depend on that issue landing.

An additive, nonconflicting local convention is not a deviation. A conflicting
choice is only valid after human adoption under
[Agent Skill Management](../../.claude/skills/agent-skill-management/SKILL.md)'s
installed-rule change contract. This project's register MUST retain that
contract's required exception and adoption evidence. For an entry adopted by
the maintainer's merge, GitHub records both the merge and the plan approval on
the tracking issue, as the [standing delivery grant](./delivery-grant.md#authorizing-delivery-operations) explains. The contract owns validity
and reassessment; this register owns their local record, not permission to
override a host restriction or publish upstream.

The exposing task continues under applicable installed rules and any separately
valid deviation. A pending upstream proposal never puts its proposed rule into
force; if applicable boundaries prevent continuing, report the blocker. Any
upstream request filed or left pending SHOULD be named in the completion report.

## The Register

### Gap - the library has no Docker, shell, or GitHub Actions workflow skill

This repository's main artifacts are Dockerfiles, shell and PowerShell scripts,
and GitHub Actions workflows. `axross/skills` has no skill for any of them, so no
installed capability states how to write or review one. What governs them
instead:

- the pinned linters in [`mise.toml`](../../mise.toml), which enforce
  well-formedness (hadolint, shellcheck, PSScriptAnalyzer, actionlint);
- [Security](../security/index.md), which states the supply-chain and
  runner trust-boundary rules;
- the language lenses in [REVIEW.md](../../REVIEW.md), which the post-push
  reviewer applies;
- `application-security` and `code-maintainability` for the generic rules they
  already state.

A session that meets a question none of these answers takes it to the maintainer
rather than inventing a convention. **Upstream status:** not filed. The
maintainer has not yet given the go-ahead for an issue on `axross/skills`; the
completion report of any change that touches one of these surfaces names the gap
as pending.

### Gap - github-operation names no rewrite by the hosted write path

The project installation of `github-operation`, whose computed hash in
`skills-lock.json` is
`138a5a473adf9feafc7d4e6c20c1c7c372634bd0d2e0a46557f79aa43443a7e7`, has all six
of its files byte-identical to `axross/skills` at commit
`99e6407460d4e477a1a0690e40bee8f7079acbf2`, the upstream commit the lockfile
records for the refresh in #180: `SKILL.md`,
`references/body-integrity.md`, `references/channel-selection.md`,
`references/identity-and-targets.md`, `references/publication-and-recovery.md`,
and `scripts/decode-sanitized-read.mjs`. They were compared on 2026-11-27 (PST).
The upstream hash was not recomputed.

The skill requires reading back each write, comparing the exact stored fields
with the intended text, and reporting an unverified result when they cannot be
shown to match. The only write-side changes it names are `--attach`'s intended
ones, the rewrite of relative references and the appended attachments, which
Body Integrity accepts after such a write; it names none made by the hosted
write path. In Claude Code cloud sessions, the hosted write path was observed
wrapping long URLs in code spans, appending the comment footer, replacing a pull
request's attribution with that footer, and removing U+200B, so a write can be
stored differently from the text sent.

This project handles the case in the GitHub Write Fidelity in Claude Code Cloud
Sessions section of [Agent Sessions](./github-write-fidelity.md#github-write-fidelity-in-claude-code-cloud-sessions). An agent composes the
text in a local file, checks it by command before writing, and compares a stored
issue or pull request body with that file by command. It compares a comment by
command where the REST read is allowed, and otherwise checks it as MCP returns
it, a check whose result stays unverified. It corrects a difference only within
the recovery limits that section states. The installed skill is left as it is.

**Upstream status:** not filed. The maintainer has not yet given the go-ahead
for an issue on `axross/skills`, so the gap is pending and a completion report
names it.

### Deviation - delivery, review, and watching share one confirmation

At the [refresh for #96](https://github.com/axross/runners/blob/10e3071db7e7571581c8a1d10baf6d71cdbd134b/skills-lock.json) (upstream `cce7ecf`), the project's lockfile recorded `professional-behavior` with hash
`e970d2315042396152616b29559545e115b622c7b524f7e8e75416db2fc51789`. The refresh
in #180 (upstream `99e6407460d4e477a1a0690e40bee8f7079acbf2`) recorded it with
hash `d632abe2e2239feb51b1bc611094af2e4c433ae43b71cdc8e8c0ad1e69265865`. The
installed rule still reads, in the Decisions in One Prompt section of
[Asking the Human](../../.claude/skills/professional-behavior/references/asking-the-human.md):
"MUST NOT put two decisions in one prompt when the answer to one would change,
prune, or reframe the other; hold the dependent one for a later round, in
dependency order."

Claude Code's plan-gate confirmation is the bounded counterexample: declining
delivery removes the pull request on which review and watching depend, but
splitting those confirmations would replace the single delivery answer the
maintainer chose to prevent repeated delivery prompts. The maintainer adopts this entry and each amendment by merging the pull
request that carries it, under the same adoption rule as the standing delivery
grant.

For this confirmation only, the maintainer retains the combined interaction
governed by the Confirming Delivery at the Plan Gate in Claude Code section of
[Agent Sessions](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code), including its coherent outcomes. The scope includes the same confirmation at
session resumption; other interviews retain the installed dependency rule. This
exception changes no installed skill, host restriction, operation grant, or plan
and review gate, and authorizes no upstream publication. It records the chosen
project interaction rather than a general defect in the skill. Reassess it if
the installed ordering rule or the delivery confirmation's dependencies change,
or the maintainer withdraws this choice. A host rule requiring sequential
questions takes precedence over this project exception.

The review item is the [Claude CI review request](./claude-ci-review.md#claude-ci-review-from-claude-code) on the pull request for
each round. Declining delivery still removes the pull request on which that
review and the watch depend.

### Deviation - the Claude Code review subagent's limits and closure

The maintainer retired this deviation when [pull request #178](https://github.com/axross/runners/pull/178)
made [Claude CI](./claude-ci-review.md#claude-ci-review-from-claude-code) the post-push review route for Claude Code. The
[`reviewer` subagent](./delegate.md#use-claude-code-subagents) serves the pre-push review.
The [previous register](https://github.com/axross/runners/blob/a070ff3488b3df526ef5083e77c908ecc383c49c/docs/operations/agent-skills.md#deviation---the-claude-code-review-subagents-limits-and-closure)
records the former exception, its adoption, and its limits.
