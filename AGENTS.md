# AGENTS.md

This file defines the working agreement for agents contributing to runners.

## Project overview

**runners** holds generic GitHub Actions self-hosted runner images, the
`marshal-cli` library under `cli/` that runs them, and Marshal, the app under
`app/` that runs that library's resident agent; the WSL 2 host configuration for
Claude Code Remote Control and Amp runner agent hosts is still planned. It is
being extracted from the runner setup of a private application repository, in
steps. [README.md](./README.md) introduces the project and its current status;
[Development Setup](./docs/operations/development-setup.md) owns setup,
[Testing](./docs/conventions/testing.md) owns the check commands, and
[Directory Structure](./docs/conventions/directory-structure.md) owns the layout
and the technologies, including the toolchain and the format and lint tools.
[docs/index.md](./docs/index.md) indexes conventions, operations, and the
glossary.

This repository is public, so its GitHub Actions CI and review jobs run on
GitHub-hosted runners only, never on a self-hosted one.

This project's fixed agent-comment marker is `<!-- ai-agent -->`. Never push to
the default branch. Choose a working branch under
[Branches and Merging](./docs/operations/development-workflow.md#branches-and-merging)
as adopted on the default branch, not an unmerged amendment. Leave merging to
the maintainer, `@axross`.

## Response Approach

For each requested runners change, the first progress update MUST precede the
first implementation edit and report:

- The current phase
- The tracking issue or its absence
- The plan-approval state

The existing Development Workflow remains the owner of the gates.

After loading Professional Behavior, use
[Loop Engineering](./.claude/skills/loop-engineering/SKILL.md) for requested
project changes, including small code or document edits. Establish the current
phase before planning or implementation. Standalone read-only work stays outside
the change gates. This agreement is the maintainer's explicit request that every
change run deliver through its required verification and the independent review
selected by
[Development Workflow](./docs/operations/development-workflow.md), then reach
ready or a reported stop naming an unmet gate, without a further prompt.
That workflow owns the delivery sequence and handoff boundary. Where a host
still requires a per-session confirmation, the run obtains it at the plan gate,
before the first action that needs it in a run without one, or when a new
session resumes the run, and never by stopping after push;
Agent Sessions describes
[Claude Code's confirmation](./docs/operations/agent-sessions.md#confirming-delivery-at-the-plan-gate-in-claude-code)
and
[Amp's plan-gate archive confirmation](./docs/operations/agent-sessions.md#confirming-review-thread-cleanup-at-the-plan-gate-in-amp).
Current host instructions and tool usage conditions still apply; for Amp and
Codex, see the delivery-authorization guidance under Host and delivery routing
below; for Claude Code, see [Claude Code entry guidance](./CLAUDE.md). Load
capabilities by task instead of applying every change gate to read-only work:

- MUST load
  [Professional Behavior](./.claude/skills/professional-behavior/SKILL.md)
  first in every session, including read-only questions and investigations.
- MUST load
  [Software Development](./.claude/skills/software-development/SKILL.md) when
  a task touches the project.
- MUST load
  [Code Maintainability](./.claude/skills/code-maintainability/SKILL.md)
  before a comment or a doc-comment is written or kept, rather than relying
  on the skill's own trigger words to fire for a session that is only
  writing a comment.
- MUST read [Development Workflow](./docs/operations/development-workflow.md)
  for project changes or tracking issue management, including integration
  reports. For code or document changes, MUST load
  [Loop Engineering](./.claude/skills/loop-engineering/SKILL.md) before planning
  or making changes and follow the workflow's change gates. Read-only work
  stays outside the change loop.
- MUST load each matching skill's body, not act from its discovery description
  alone. Domain skills govern their subject, not host tool selection.
- MUST read [README.md](./README.md) and the relevant command guide it links
  before running project commands. Software Development owns the procedure
  when a command is undocumented.
- MUST read [docs/index.md](./docs/index.md) when a task depends on project
  terminology, behavior, conventions, or past decisions, then follow only the
  relevant routes.
- MUST put a decision the maintainer owns — such as scope, product behavior, an
  architecture choice, a finding disposition, an additional review round, or a
  plan approval — to the maintainer through its host's dedicated question
  mechanism whenever the session exposes an eligible one, however the session
  was started, and MUST NOT list options as prose in the turn output in place of
  the question unless none is eligible. A launch prompt's statement that the
  launcher handles communication with the maintainer, or its instruction to stop
  and report, assigns a duty to report to the launcher; it MUST NOT be read as
  replacing the question.
- MUST read
  [Asking the Maintainer](./docs/operations/agent-sessions.md#asking-the-maintainer)
  when a session holds a maintainer decision, was launched by another session, or
  launches one.
- MUST delegate investigation, implementation, and advisory pre-flight review
  to the qualifying subagents configured in
  [Agent sessions](./docs/operations/agent-sessions.md#subagents) when the host
  permits them. This agreement is the standing request for those roles; record
  when no qualifying agent is available or the host refuses the spawn.

## Host and delivery routing

Choose guidance from the actual session and changed surface:

- **Amp:** follow the runtime's current tool contracts. Before any delivery
  effect, and whenever a task arrives framed as "make changes, commit, and
  push", MUST follow
  [Amp delivery authorization](./docs/operations/agent-sessions.md#amp-delivery-authorization).
  Consult [Agent sessions](./docs/operations/agent-sessions.md#amp-quality-hooks)
  for the quality-hooks plugin,
  [the plan-gate confirmation for archiving review child threads](./docs/operations/agent-sessions.md#confirming-review-thread-cleanup-at-the-plan-gate-in-amp),
  and
  [its environment provisioning](./docs/operations/agent-sessions.md#amp-environment-provisioning)
  for the `.agents/setup` script.
- **Codex:** follow the runtime's current tool contracts. Before any delivery
  effect, and whenever a task arrives framed as "make changes, commit, and
  push", MUST follow
  [Codex delivery authorization](./docs/operations/agent-sessions.md#codex-delivery-authorization).
- **Claude Code:** consult
  [Agent sessions](./docs/operations/agent-sessions.md) for startup, hooks, the
  plan-gate delivery confirmation, subagents, and telemetry.
- **GitHub:** load
  [GitHub Operation](./.claude/skills/github-operation/SKILL.md) for reads and
  writes. [Development Workflow](./docs/operations/development-workflow.md)
  owns this project's delivery path, branch policy, and independent-review
  route.
- **Reviews:** load [Code Review](./.claude/skills/code-review/SKILL.md) for
  review methodology; the Code Review Rules section below routes the
  project-specific policy. Changes to review or CI infrastructure, runner
  images and host scripts, secret handling, the dependency or supply-chain
  surface (including pinned actions and tool versions), anything that changes
  what a self-hosted runner can reach, and large refactors SHOULD also receive
  human review.

## Ownership of agent-created work

Ownership is part of this project's agent-delivery contract:

- MUST include the authenticated operator in the assignees of each issue and
  pull request the run creates in this repository, unless the human explicitly
  specifies different ownership or no assignment.
- MUST verify the required ownership immediately after creation and again
  before marking a pull request ready or reporting delivery complete. Pending
  or unknown required ownership leaves delivery incomplete.

Apply [GitHub Operation](./.claude/skills/github-operation/SKILL.md) for
identity resolution, assignment, read-back, and partial-outcome recovery.
[Development Workflow](./docs/operations/development-workflow.md#authorizing-delivery-operations)
owns the scoped operation grant and its adoption and host boundaries.

## Code Review Rules

For every review, MUST read and apply [REVIEW.md](./REVIEW.md) for project checks
and adopted posted-report arrangements, and load
[Code Review](./.claude/skills/code-review/SKILL.md) for methodology. REVIEW.md
governs where posted-report instructions differ. Consult
[Development Workflow](./docs/operations/development-workflow.md) for provider
invocation and completion evidence; routing alone does not establish provider
consumption or review completion.

For mandatory independent review after opening the draft pull request, use the
route selected for the authoring host:

- **Claude Code:** start a fresh Claude Code review child session and ask it to
  review the pull request and publish its review directly there.
- **Amp:** start a fresh Amp review child thread for each round, never reusing
  one, and ask it to review the pull request and publish its review directly
  there.
- **Codex, and changes made without an agent:** the Codex review.

These children MUST have a fresh non-author context, not a fork of the author's
conversation or a reused advisory reviewer. An independent-review child reviews
and posts its review itself and does not delegate; the subagent-delegation rule
above does not apply to it. Advisory pre-flight remains a separate checkpoint
and does not satisfy independent review. Follow
[The Independent Review](./docs/operations/development-workflow.md#the-independent-review)
for qualification, review rounds, publication verification, child cleanup, and
the comment-triggered Claude CI review the maintainer can select instead.
Current host and tool restrictions still apply; an unavailable selected route
leaves the independent-review gate unmet rather than authorizing a substitute.

## Skill maintenance

Every directory under `.claude/skills/` is an installed copy from
[`axross/skills`](https://github.com/axross/skills), selected and pinned by
[`skills-lock.json`](./skills-lock.json). Never hand-edit an installed copy.
For refreshes, inventory changes, loading diagnostics, or an upstream gap,
load
[Agent Skill Management](./.claude/skills/agent-skill-management/SKILL.md) and
follow [Agent Skills](./docs/operations/agent-skills.md). For content or
metadata work, also load
[Agent Skill Authoring](./.claude/skills/agent-skill-authoring/SKILL.md).
Skill source, installed copies, and active loading have distinct evidence;
their layout belongs to [Agent Skills](./docs/operations/agent-skills.md).

At completion, MUST report whether skill maintenance was performed, skipped,
or blocked. Loop Engineering's
[Resume and Reporting](./.claude/skills/loop-engineering/SKILL.md#resume-and-reporting)
section owns the evidence required beside that report.
