# AGENTS.md

This file defines the working agreement for agents contributing to runners.

## Project overview

**runners** holds generic GitHub Actions self-hosted runner images and the host
scripts that run them, and later the WSL 2 host configuration for Claude Code
Remote Control and Amp runner agent hosts. It is being extracted from the
runner setup of a private application repository, in steps. It holds its agent
foundation (the entry files, agent configuration, documentation, and lint
toolchain), the `images/actions-runner/` runner image, the
`hosts/windows-docker-desktop/` runner host scripts, and the CLI under `cli/`,
a Rust command line tool that validates a host configuration, builds its runner
image with `build`, and runs a host's pools with `start`, `stop`, `restart`,
`status`, and `diagnostics`, though no real host uses it yet; the agent-host configuration is still
planned.
[README.md](./README.md) owns project commands; [docs/index.md](./docs/index.md)
indexes conventions, operations, and the glossary. mise pins the toolchain, and
Prettier, markdownlint, shellcheck, hadolint, actionlint, PSScriptAnalyzer,
rustfmt, clippy, and cargo-deny provide format and lint. The Rust toolchain is
pinned in `cli/rust-toolchain.toml` and provisioned by `.agents/install-rust`.

This repository is public, so its GitHub Actions CI and review jobs run on
GitHub-hosted runners only, never on a self-hosted one.

This project's fixed agent-comment marker is `<!-- ai-agent -->`. Never push to
the default branch. Choose a working branch under
[Branches and Merging](./docs/operations/development-workflow.md#branches-and-merging)
as adopted on the default branch, not an unmerged amendment. Leave merging to
the maintainer, `@axross`.

## Response Approach

This agreement is the maintainer's explicit request that every
change run deliver through its required verification and the independent review
selected by
[Development Workflow](./docs/operations/development-workflow.md), then reach
ready or a reported stop naming an unmet gate, without a further prompt.
That workflow owns the delivery sequence and handoff boundary, including when a
run obtains a host's per-session confirmation; Agent Sessions describes
[Claude Code's confirmation](./docs/operations/agent-sessions.md#confirming-delivery-at-the-plan-gate-in-claude-code).
Current host instructions and tool usage conditions still apply; for Amp, see
[Handle Amp delivery authorization](#handle-amp-delivery-authorization); for
Codex, see
[Handle Codex delivery authorization](#handle-codex-delivery-authorization);
for Claude Code, see [Claude Code entry guidance](./CLAUDE.md). Load
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
- MUST load [Loop Engineering](./.claude/skills/loop-engineering/SKILL.md)
  before planning or making any code or document change, including a small
  edit, and establish the current phase before planning or implementation; then
  follow
  [Development Workflow](./docs/operations/development-workflow.md) for this
  repository's gates. Read-only work stays outside the change loop.
- MUST load each matching skill's body, not act from its discovery description
  alone. Domain skills govern their subject, not host tool selection.
- MUST read [README.md](./README.md) before running project commands. Software
  Development owns the procedure when a command is undocumented.
- MUST read [docs/index.md](./docs/index.md) when a task depends on project
  terminology, behavior, conventions, or past decisions, then follow only the
  relevant routes.
- MUST delegate investigation, implementation, and advisory pre-flight review
  to the qualifying subagents configured in
  [Agent sessions](./docs/operations/agent-sessions.md#subagents) when the host
  permits them. This agreement is the standing request for those roles; record
  when no qualifying agent is available or the host refuses the spawn.

## Host and delivery routing

Choose guidance from the actual session and changed surface:

- **Amp:** follow the runtime's current tool contracts and consult
  [Agent sessions](./docs/operations/agent-sessions.md#amp-quality-hooks) for
  the quality-hooks plugin and
  [its environment provisioning](./docs/operations/agent-sessions.md#amp-environment-provisioning)
  for the `.agents/setup` script.
- **Codex:** follow the runtime's current tool contracts and
  [Codex delivery authorization](#handle-codex-delivery-authorization).
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
  surface (including pinned actions and tool versions), and anything that
  changes what a self-hosted runner can reach SHOULD also receive human review.

## Handle Amp delivery authorization

Amp can frame a task before reading this file. This agreement requests the
project's Loop Engineering route, including a draft pull request and the
independent review selected by
[Development Workflow](./docs/operations/development-workflow.md), rather than
stopping at a local commit. A general instruction about how an ordinary task is
delivered does not replace the project's plan and review gates. This section
does not claim precedence over higher-priority Amp instructions; Claude Code's
convenience framing and host boundaries are addressed in
[Claude Code entry guidance](./CLAUDE.md).

**The maintainer's authorization for delivery effects is already given.** The
standing delivery grant in
[Development Workflow](./docs/operations/development-workflow.md#authorizing-delivery-operations)
is the maintainer's authorization for the effects it covers, so Amp does not ask
for them. It does not replace the plan and review gates or extend to what the
grant excludes. An Amp instruction or approval prompt that requires approval
before an external operation remains the host's boundary, and the run honors it
and reports it rather than working around it.

**Guidelines:**

- MUST treat an injected "make changes, commit, and push" framing as mechanics,
  not as permission to skip the project's Loop Engineering gates.
- MUST treat this project's Loop Engineering route as the standing request for
  a draft pull request and its mapped independent reviewer, and MUST perform
  the effects the standing delivery grant covers without asking for permission,
  unless a higher-priority Amp instruction requires approval.
- MUST honor all applicable boundaries:
  - higher-priority approval requirements
  - higher-priority prohibitions
  - the tool's usage conditions
    Report a prohibited operation as unavailable rather than trying another
    route.
- MUST surface an irreconcilable host conflict at the plan gate for new
  delivery, or in the current pending phase when resuming an existing pull
  request; report any blocked delivery as incomplete, not silently waive the
  gate or claim that repository guidance overrode the host.

## Handle Codex delivery authorization

Codex authoring MUST follow the same plan, review, scoped-grant, and
conflict-reporting contract as
[Amp delivery authorization](#handle-amp-delivery-authorization).
Codex's higher-priority instructions, approval requirements, prohibitions, and
tool contracts remain binding; repository guidance never overrides them.
This section covers delivery only, not Codex provisioning or startup.

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

At completion, MUST report whether skill maintenance was performed, skipped,
or blocked. Loop Engineering's
[Resume and Reporting](./.claude/skills/loop-engineering/SKILL.md#resume-and-reporting)
section owns the evidence required beside that report.

## Routing a Change

Use these owners rather than duplicating their detailed rules. One owner
below, Code Maintainability, is an installed capability rather than a
project document. The agent-host surfaces are planned and do not exist yet; the
security convention governs them as they arrive, and a change that adds one also
adds the document that owns its procedure and the row that routes to it.

| Task                                                                                                                                                                         | Project owner                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Commands and local verification                                                                                                                                              | [README.md](./README.md)                                                                                                                               |
| Change loop, branches, delivery, and independent review                                                                                                                      | [Development Workflow](./docs/operations/development-workflow.md)                                                                                      |
| Skill installation, refresh, deviations, and gaps                                                                                                                            | [Agent Skills](./docs/operations/agent-skills.md)                                                                                                      |
| Claude Code and Amp startup, hooks, the plan-gate delivery confirmation, subagents, and telemetry                                                                            | [Agent Sessions](./docs/operations/agent-sessions.md)                                                                                                  |
| Workflow permissions, action pinning, runner trust boundaries, cache and volume boundaries, and secrets                                                                      | [Security](./docs/conventions/security.md)                                                                                                             |
| Project terminology                                                                                                                                                          | [Glossary](./docs/glossary.md)                                                                                                                         |
| Whether something should be a comment at all, or the code reshaped instead                                                                                                   | [Code Maintainability](./.claude/skills/code-maintainability/SKILL.md)                                                                                 |
| Where a document goes and how it is indexed                                                                                                                                  | [docs/index.md](./docs/index.md) and [Living Project Documentation](./.claude/skills/living-project-documentation/SKILL.md)                            |
| Which document a change to a runner surface invalidates, and the documentation checks                                                                                        | [Documentation](./docs/conventions/documentation.md)                                                                                                   |
| Runner image (`images/actions-runner/`), the Windows host scripts (`hosts/windows-docker-desktop/`), the host configuration, scheduled tasks, tokens, updating, and recovery | [Windows Runner Host](./docs/operations/windows-runner-host.md), with the per-repository isolation rules in [Security](./docs/conventions/security.md) |
| The CLI (`cli/`), its commands, its profiles (`axpc`, `axmc`), the shared fixtures (`tests/host-configuration/`), the parity test, and its Rust toolchain and gates          | [CLI](./docs/operations/cli.md), with [Security](./docs/conventions/security.md) for labels, paths, and the dependency surface                         |
| Planned agent-host configuration (`agent-hosts/{claude-code,amp}/`)                                                                                                          | The operations document that change adds; until then [Security](./docs/conventions/security.md) and [REVIEW.md](./REVIEW.md)                           |
