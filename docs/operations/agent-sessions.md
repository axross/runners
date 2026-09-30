# Agent Sessions

How agent sessions start in this project, the hooks that run during one, the
subagents Claude Code can spawn, and how a session's usage is tagged. Claude
Code and Amp are the supported hosts; each has its own section because they
share the pinned toolchain in [`mise.toml`](../../mise.toml) and nothing else.

## Claude Code Sessions

### The Session-Start Hook

In a cloud session, [`.claude/hooks/session-start.sh`](../../.claude/hooks/session-start.sh)
activates mise, runs `mise trust` and `mise install` to provision the pinned
toolchain, copies the opt-in quality hooks into place, and echoes a pointer to
[`AGENTS.md`](../../AGENTS.md) so every session carries the working agreement
into its context. It installs no JavaScript dependencies, because this
repository has no `package.json`; Prettier and markdownlint come from mise.

It exits immediately unless `CLAUDE_CODE_REMOTE=true`, because a local session
manages its own toolchain and should not have one installed under it. Set that
variable by hand to exercise the hook locally.

The toolchain block activates mise only when it is **already** present. It MUST
NOT be changed to install mise unconditionally: a hard `curl | sh` turns a
transient network failure into a failed session start, a failure that surfaces
as every later command missing its tools rather than as an install error. When
mise is absent the hook warns and continues. [`.agents/setup`](../../.agents/setup)
is the place that installs mise, with a checksum, for hosts that provision it.

The hook is wired in [`.claude/settings.json`](../../.claude/settings.json),
which also sets the session's default reasoning effort, `effortLevel`, shipped as
`xhigh`. Both are read at session start, so a change to either reaches only the
next session.

The reminder it echoes names `AGENTS.md` rather than `CLAUDE.md` on purpose.
`CLAUDE.md` is an `@AGENTS.md` import, which is a Claude Code mechanism; a host
that does not resolve imports would read the literal import line instead of the
working agreement.

### The Opt-In Quality Hooks

Format-on-edit and check-before-stop are **opt-in**. They live in
[`.claude/settings.local-example.json`](../../.claude/settings.local-example.json),
which the session-start hook copies to the gitignored `settings.local.json` in a
cloud session; Claude Code hot-reloads them for that session. A local session
skips the hook entirely, so opting in there stays a manual copy.

That example file also pre-approves `send_later` and `delete_trigger`. They are
not a convenience: `loop-engineering` schedules its own wake with them while
waiting on CI and the independent review, and without the grant every wait
raises a permission prompt that an unattended session cannot answer.

A blocking `Stop` check is expensive in a way a `PostToolUse` repair is not: it
fires only after the agent believes the task is finished, so a failure there
costs a full main turn before the agent can stop again. Whether a check belongs
at `Stop` or earlier therefore turns on whether it needs an authoring decision or
is purely mechanical:

- **Formatting** is mechanical.
  [`format.sh`](../../.claude/hooks/format.sh) runs `prettier --write` on a
  Markdown, JSON, YAML, TypeScript, or JavaScript file the moment it is written
  through `Edit`, `Write`, or `MultiEdit`. A file changed another way, such as a
  Bash heredoc or `sed -i`, is not reached and is caught by `format:check` at
  `Stop` and in CI.
- **Lint, link, documentation, and skills-lock findings** need a decision:
  which link to fix, which word to choose, which quoting to apply.
  [`check.sh`](../../.claude/hooks/check.sh) runs `mise run check` on `Stop` for
  a session that has uncommitted changes or commits ahead of its upstream, falling
  back to `origin/HEAD` and then `origin/main` when there is no upstream, and
  blocks completion with the tail of the output when it fails.

### Subagents

[`.claude/agents/`](../../.claude/agents/) holds three definitions, and it is the
only home for any of them: an agent definition is not a skill, so the skills CLI
never carries it, and it never appears in `skills-lock.json`.

`implementer.md` is the worker `loop-engineering` delegates Code and Verify to.
It pins a lower-cost model, because a worker inheriting the session's model runs
at the main actor's cost and defeats the point of delegating. It states its
delivery boundary, that commits stay local and pushing and publishing belong to
whoever asked, in its own prose rather than by withdrawing a tool.

`reviewer.md` is the reader for the advisory pre-flight review. It denies exactly
two things, editing and spawning, and nothing else. Widening that deny-list is
the tempting mistake and MUST be resisted: judging a change means confirming what
was asked and not only what was written, which reaches the issue, the plan's
artifacts, and the documentation behind a factual claim. A reviewer that cannot
reach one of those does not fail to start; it returns a report short by exactly
those checks, and an under-equipped review reads exactly like a clean one.

`investigator.md` is the reader for a payload the main actor needs only one
conclusion from: a log, a long thread, a wide search across files or history, a
file tree. It returns a conclusion and a locator precise enough to go back to the
source, never the payload itself. Like the reviewer, it denies editing and
spawning, and it decides nothing the material does not itself settle.

All three run at `effort: high`. Deleting any of the three files degrades
gracefully rather than breaking the loop. Without the implementer, the loop
delegates to a generic agent or runs single-agent; without the reviewer, the
pre-flight stage is skipped rather than performed by the main actor, which is
what keeps it from collapsing into self-review; without the investigator, the
main actor reads the payload itself.

### Telemetry Tagging

[`.claude/settings.json`](../../.claude/settings.json) carries an `env` block
setting two OpenTelemetry variables, so this project's usage separates from every
other repository sharing an account or a cloud environment.
`OTEL_RESOURCE_ATTRIBUTES` stamps `repository=runners` onto the resource Claude
Code exports; `OTEL_METRICS_INCLUDE_ENTRYPOINT` adds the session's launch surface
to metric datapoints. They are two mechanisms rather than one: the resource
describes what is emitting, and the datapoint attribute describes one emission.
The block configures nothing else, no endpoint, no credential, and no
`CLAUDE_CODE_ENABLE_TELEMETRY`, so a contributor who has never set telemetry up
sees no behavior change from it.

Verifying a change to that block is the catch: Claude Code does not pass `OTEL_*`
variables to the subprocesses it spawns, so `echo $OTEL_RESOURCE_ATTRIBUTES`
inside a session prints nothing even when the exporter holds the value. Confirm it
in the metrics backend instead, against a session started **after** the change.

## Amp Sessions

### The Quality-Hooks Plugin

Amp loads [`.amp/plugins/quality-hooks.ts`](../../.amp/plugins/quality-hooks.ts)
as a project plugin. Its `tool.result` handler reacts only to successful tool
results for project-owned Markdown, JSON, YAML, TypeScript, and JavaScript files
reported by Amp's file-modification helper, and runs `mise run format` once for
the event. The repair is best effort: a failure is recorded in the plugin log
without replacing the tool's original result.

On a successfully completed turn, the plugin's `agent.end` handler runs
`mise run lint` when changes are uncommitted or committed ahead of the available
upstream, falling back to the default remote branch resolved through `origin/HEAD`
when the current branch has no upstream, and to `origin/main` with a logged
notice when that does not resolve either. A failure starts one follow-up turn with
the tail of the lint output. A marker in that follow-up prevents the same failure
from starting an unbounded sequence of turns. Unlike Claude Code's blocking
`Stop` hook above, Amp's public completion result cannot reject completion
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

### Environment Provisioning

[`.agents/setup`](../../.agents/setup) provisions an Amp environment. It is
idempotent: it installs any missing system packages (`ca-certificates`, `curl`,
`git`), installs the pinned mise release only when the installed version differs
and verifies the download against a SHA-256 recorded in the script, runs
`mise trust` and `mise install`, persists mise activation in the login shell, and
verifies that every pinned tool resolves. When changing the pinned mise version,
update it in both places together: the `version` and `sha256` inputs of the
`jdx/mise-action` step in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml), and the
`version` and `expected_sha256` in `.agents/setup`. Both hold the SHA-256 of the
bare `mise-v<version>-linux-x64` binary, taken from the release's published
`SHASUMS256.txt`; the action hashes the binary it extracts from its archive, so
it does not use the archive's checksum.
