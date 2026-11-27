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
contract's required exception and adoption evidence. The contract owns validity
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
- the language lenses in [REVIEW.md](../../REVIEW.md), which the independent
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
`cbe366fe59843dd47925eae36d341bec28397e25835884d303b23e8e52037d40`, has all six
of its files byte-identical to `axross/skills` at commit
`69a254dd6b368732a6f8a153f5826784b7df6746`: `SKILL.md`,
`references/body-integrity.md`, `references/channel-selection.md`,
`references/identity-and-targets.md`, `references/publication-and-recovery.md`,
and `scripts/decode-sanitized-read.mjs`. They were compared on 2026-11-20 (PST).
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
`e970d2315042396152616b29559545e115b622c7b524f7e8e75416db2fc51789`. The installed rule still reads, in
[Asking the Human](../../.claude/skills/professional-behavior/references/asking-the-human.md):
"MUST NOT put two decisions in one prompt when the answer to one would change,
prune, or reframe the other; hold the dependent one for a later round, in
dependency order."

Claude Code's plan-gate confirmation is the bounded counterexample: declining
delivery removes the pull request on which review and watching depend, but
splitting those confirmations would replace the single delivery answer the
maintainer chose to prevent repeated delivery prompts. This entry is adopted
the same way as the standing delivery grant: by the maintainer's merge of the
pull request that closes
[issue #35](https://github.com/axross/runners/issues/35), which adds it together
with the plan-gate confirmation it serves.

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

Since the merge of the pull request that closes
[issue #127](https://github.com/axross/runners/issues/127), the confirmation's
review item is the creation of a Claude Code review child session for the pull
request, or the Claude CI review request when the maintainer has selected that
alternative for the run. Since the merge of the pull request that closes
[issue #151](https://github.com/axross/runners/issues/151), the first of those
is the spawning of a new `reviewer` subagent instance for each round; the
dependency this entry records is unchanged.

### Deviation - the Claude Code review subagent's limits and closure

At the [refresh for #96](https://github.com/axross/runners/blob/10e3071db7e7571581c8a1d10baf6d71cdbd134b/skills-lock.json) (upstream `cce7ecf`), the project's lockfile recorded `loop-engineering` with hash
`6a7dc5702473917c880c2f783265d099d555fa5e4e063ac411a2fed599118806`. The installed rules still read, in its
[review workflow](../../.claude/skills/loop-engineering/references/review-workflow.md):

- "MUST use available, permitted tool restrictions for a review child while
  retaining required material access and direct publication through the
  target's review mechanism."
- "MUST close each review child the run created after verifying its posted
  result, using an available supported end/close/archive operation and
  preserving review history and posted evidence."
- For Claude Code, "end the review child session using its supported lifecycle
  operation. Automatic one-shot termination counts when observed; an idle
  resumable session is not proof of closure."
- "MUST tell the human when cleanup cannot be completed for another reason, or
  permission remains unavailable, identify the exact session/thread and reason,
  and ask them to close it with concrete host-specific steps."

The Claude Code review subagent in
[Development Workflow](./claude-subagent-review.md#claude-code-review-subagent-from-claude-code) departs from these in four
bounded ways, each the maintainer's choice:

- The instance is the project's `reviewer` subagent, spawned with
  `isolation: worktree`. Its definition declares, and Claude Code documents
  enforcing, the denial of the Edit, Write, NotebookEdit, and Agent tools; no
  live round has confirmed it yet, and
  [#155](https://github.com/axross/runners/issues/155) tracks the first. The
  other limits are instruction-only, as the accepted-consequences list in
  Development Workflow's grant section gives them. The definition leaves
  available restrictions on GitHub write tools and on session-starting and
  messaging tools unused, for the reasons the counterexample below gives. The
  same `reviewer.md` still serves as the advisory pre-flight reader, and its
  prose permits posting only under an independent-review assignment that
  carries a round token.
- The host documentation describes no operation that makes a completed subagent
  non-resumable, so a completed instance is recorded as completed in run state
  rather than closed, and that record ends the round without a per-round
  handoff. The run never resumes a completed instance.
- An instance still running after its result is verified or its round has
  failed is stopped with `TaskStop`. The host documentation says an instance
  stopped this way resumes when messaged, so, like a completed instance, it is
  recorded as stopped rather than closed, and the run never messages or resumes
  it. An instance the run cannot stop, including one an earlier session
  spawned, whose agent ID only that session can use, is handed to the
  maintainer with its agent ID, the reason, and, in a local interactive
  session, the step "press `x` on its row in `/tasks`" in the session that
  spawned it.
- In a cloud session, where `/tasks` is not documented, the handoff says that
  no human step is verified there instead of naming one.

The concrete counterexample to the first quoted rule is the review subagent
itself: denying every GitHub write tool would remove the direct publication that
rule requires; a deny-list naming only one connector's non-review GitHub write
tools, or one session's connector-provided session-starting or messaging tools,
would not cover other sessions' connectors and would leave the same writes
reachable through the shell; and a reader still needs the shell and the tracker
to read the change. The accepted exposure is
that a `reviewer` instance, whose definition carries the conditional posting
permission, can still write through the shell or the tracker, or start another
actor through a non-Agent tool, and the host would not stop it. The alternatives
are a second, posting-capable definition and a deny-list that also names GitHub
write tools, all of them or only the non-review ones, and session-starting and
messaging tools; the maintainer chose one definition and left the rest
instruction-only, and accepted the exposure.

The deviation described before this amendment, a general-purpose subagent child
whose limits beyond the worktree were all instructions, was adopted by the
maintainer's merge of the pull request that closes
[issue #127](https://github.com/axross/runners/issues/127), together with the
review route and grant amendments it served. This amendment follows the plan the
maintainer approved in
[issue #151](https://github.com/axross/runners/issues/151), recorded at
[this run-state comment](https://github.com/axross/runners/issues/151#issuecomment-7049089352),
and, like the earlier ones, is adopted by the maintainer's merge of the pull
request that closes that issue, which is its adoption evidence. Its scope
is the Claude Code independent-review route only; the Amp and Codex routes are
unaffected. It changes no installed skill, host restriction, or plan and review
gate, and authorizes no upstream publication. Reassess it when the installed
review-workflow rules change, when Claude Code documents an operation that ends
a completed subagent, enforces the posting condition or the remaining limits on
a project subagent, or documents `/tasks` for cloud sessions, when
[#155](https://github.com/axross/runners/issues/155) records a host fact that
contradicts what the route assumes, or when the maintainer withdraws this
choice.
