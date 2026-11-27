---
type: Specification
title: Agent quality hooks
description: Determine what Amp and Claude Code check after edits and at turn end, including failure and skip conditions.
---

# Agent quality hooks

## Amp Quality Hooks

Amp loads [`.amp/plugins/quality-hooks.ts`](../../.amp/plugins/quality-hooks.ts)
as a project plugin. Its `tool.result` handler reacts only to successful tool
results for project-owned Markdown, JSON, YAML, TypeScript, JavaScript, and Rust
files reported by Amp's file-modification helper, and runs `mise run format` once
for the event, which formats the Rust files with the pinned `rustfmt`. Every
command it runs has `~/.cargo/bin` on its `PATH` beside mise's directory. The repair is best effort: a failure is recorded in the plugin log
without replacing the tool's original result.

On a successfully completed turn, the plugin's `agent.end` handler runs
`mise run lint` when changes are uncommitted or committed ahead of the available
upstream, falling back to the default remote branch resolved through `origin/HEAD`
when the current branch has no upstream, and to `origin/main` with a logged
notice when that does not resolve either. A failure starts one follow-up turn with
the tail of the lint output. A marker in that follow-up prevents the same failure
from starting an unbounded sequence of turns. Unlike [Claude Code's blocking `Stop` hook](#the-claude-code-quality-hooks), Amp's public completion result cannot reject completion
directly; the guarded follow-up gives the agent one opportunity to repair the
failure. Error and cancelled turns, and completed turns without pending changes,
do not run this check. The Amp hook runs `lint` where the Claude Code hook runs
`check`, so a link, documentation, or skills-lock failure surfaces in Amp at
`mise run check` and in CI rather than at turn end.

All quality commands share one in-process queue so concurrent lifecycle events
cannot inspect or mutate the same workspace at the same time. The plugin is an
Amp runtime entry point only: it neither reads nor invokes `.claude/` hooks. A
smoke test, [`.amp/tests/quality-hooks.test.ts`](../../.amp/tests/quality-hooks.test.ts),
runs under `mise run check:amp` with Node's built-in test runner, using stubs for
Amp's plugin API and for the command runner. Node 24 strips the plugin's
type-only import of `@ampcode/plugin`, so the package need not be installed. The
test does not show that Amp loads the plugin or calls its handlers; reload
plugins from Amp's command palette or restart Amp after changing it, and confirm
the hook runs before relying on it.

## The Claude Code Quality Hooks

Format-on-edit and check-before-stop are **opt-in**. They live in
[`.claude/settings.local-example.json`](../../.claude/settings.local-example.json),
which the session-start hook copies to the gitignored `settings.local.json` in a
cloud session; Claude Code hot-reloads them for that session. A local session
skips the hook entirely, so opting in there stays a manual copy.

That example file also contains `permissions.allow` entries for
`mcp__Claude_Code_Remote__send_later` and `mcp__Claude_Code_Remote__delete_trigger`.
They exist only to spare a permission prompt when a host exposes those tools.
They are tool-permission rules, subject to Claude Code's
[permission precedence](https://code.claude.com/docs/en/permissions), not
standalone evidence of live availability, human adoption, or a scoped operation
grant. Waiting belongs to the host; `loop-engineering` requires no scheduler or
particular wake tool. [Waiting on CI and Review](./wait-for-checks.md#waiting-on-ci-and-review)
distinguishes Claude's documented mechanisms from those configured names.

A blocking `Stop` check is expensive in a way a `PostToolUse` repair is not: it
fires only after the agent believes the task is finished, so a failure there
costs a full main turn before the agent can stop again. Whether a check belongs
at `Stop` or earlier therefore turns on whether it needs an authoring decision or
is purely mechanical:

- **Formatting** is mechanical.
  [`format.sh`](../../.claude/hooks/format.sh) runs `prettier --write` on a
  Markdown, JSON, YAML, TypeScript, or JavaScript file the moment it is written
  through `Edit`, `Write`, or `MultiEdit`, and runs `cargo fmt` from `cli/` for a
  Rust file, so the `rustfmt` that `cli/rust-toolchain.toml` pins formats it. The
  Rust branch never starts a toolchain download: with no pinned toolchain it does
  nothing, and `format:check` at `Stop` catches the file. A file changed another way, such as a
  Bash heredoc or `sed -i`, is not reached and is caught by `format:check` at
  `Stop` and in CI.
- **Lint, link, documentation, and skills-lock findings** need a decision:
  which link to fix, which word to choose, which quoting to apply.
  [`check.sh`](../../.claude/hooks/check.sh) runs `mise run check` on `Stop` for
  a session that has uncommitted changes or commits ahead of its upstream, falling
  back to `origin/HEAD` and then `origin/main` when there is no usable upstream.
  When none of those resolves, it cannot tell what changed, so it runs the check.
  It blocks completion with the tail of the output when the check fails.

The hook decides from the checkout's Git state, not from what the session edited,
and it does not fetch refs. After a push that sets the upstream, the checkout is
clean and nothing is ahead of it, so the hook exits without running the check;
it cannot see a loop that stopped before the draft pull request. The What
Catches a Loop Stopped Mid-Flight section of
[Development Workflow](./change-delivery.md#what-catches-a-loop-stopped-mid-flight) states what that leaves
uncovered.
