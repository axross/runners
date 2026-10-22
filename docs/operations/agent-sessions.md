# Agent Sessions

How agent sessions start in this project, the hooks that run during one,
Claude Code's plan-gate delivery confirmation, host-owned CI and review waits,
the subagents a session can delegate to, how a session's usage is tagged, and
the environment variables recommended for cutting a Claude Code session's cost.
Claude Code and Amp are the hosts this repository configures; they share the
pinned toolchain in [`mise.toml`](../../mise.toml) and nothing else. Codex
reads [`AGENTS.md`](../../AGENTS.md) and has no configuration here.

## Amp Quality Hooks

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
`Stop` hook below, Amp's public completion result cannot reject completion
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

## Amp Environment Provisioning

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

## The Session-Start Hook

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
particular wake tool. [Waiting on CI and Review](#waiting-on-ci-and-review)
distinguishes Claude's documented mechanisms from those configured names.

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
  back to `origin/HEAD` and then `origin/main` when there is no usable upstream.
  When none of those resolves, it cannot tell what changed, so it runs the check.
  It blocks completion with the tail of the output when the check fails.

The hook decides from the checkout's Git state, not from what the session edited,
and it does not fetch refs. It therefore says nothing about whether the change
loop ran to its end: a session stopped after a push and before its draft pull
request passes it when the check passes. Development Workflow's
[What Catches a Loop Stopped Mid-Flight](./development-workflow.md#what-catches-a-loop-stopped-mid-flight)
states what that leaves uncovered.

## Confirming Delivery at the Plan Gate in Claude Code

Claude Code's host conditions pull request creation on an explicit request from
the user. The checked-in request in [`AGENTS.md`](../../AGENTS.md) does not
establish that on its own: a run that relied on it would stop after push to ask
whether to open a pull request, and might treat its own subagent reviews as the
review step. A Claude Code run therefore collects that request where the human
is already answering: in the plan-approval question.

Every plan-approval question, including the one for a revised plan, MUST also
ask the human to confirm, in the same answer:

- delivery of the run's draft pull request;
- the Claude review request on it;
- watching that pull request's activity, where the host offers it, until the
  ready transition or a reported stop.

Only pull request creation carries the host's condition. The maintainer chose
to settle the review request and the watch in the same answer, so that one
answer covers the whole delivery path. This bounded exception to dependent
question ordering is recorded in [Agent Skills](./agent-skills.md#deviation---delivery-review-and-watching-share-one-confirmation).
The run MUST offer coherent combined outcomes in that single question, with
review and watching inapplicable when delivery is declined.

A confirming answer is the user's explicit request for that run's pull
request. The run then MUST follow the post-approval sequence in
[Development Workflow](./development-workflow.md)'s After Approval section
without asking again. For a revision approved after the pull request exists, the
delivery item confirms continuing on that pull request.

The confirmation meets the host's condition and nothing more. It is not a
request for permission under the standing delivery grant in
[Development Workflow](./development-workflow.md), which still authorizes its
covered effects, and it does not authorize scheduling. For watching, it
covers the event subscription only; any push or review-thread reply that
follows stays under the delivery grant.

**Guidelines:**

- MUST obtain the confirmation in the plan-approval exchange. When approval
  arrives some other way, such as a chat message or an issue comment, obtain
  it before any change, in that same exchange where the host allows.
- MUST record the confirmation and each of its three answers in run state.
  The host's condition is per session, so the record shows what an earlier
  session confirmed, not a request made in this one. A session whose own
  conversation lacks the confirming exchange, such as a fresh session that
  resumes the run, asks for the confirmation once, at resumption and before
  any further change, even when the run is already past push.
- MUST NOT stop after push to ask about the pull request. Asking at
  resumption, as above, is recovery, not such a stop.
- MUST, when the human declines delivery, stop after push, report the
  external-review gate as unmet and the loop as not complete, and substitute
  no other review.
- MUST, when the human confirms delivery but declines the review request,
  open the draft pull request, keep it draft, and report the external-review
  gate as unmet and the loop as not complete.
- MUST, when the human confirms delivery but declines watching, still request
  review, then treat each review or check result as a human wait: end the
  turn and resume on the human's next message, with no scheduling.

## Waiting on CI and Review

The host supplies the wait mechanism. The portable contract lives in
[Loop Engineering's independent-review reference](../../.claude/skills/loop-engineering/references/independent-review.md),
whose Waiting Bound section owns capability resolution, deadlines, budget
resets, human waits, and teardown at each stop. A scheduler's expiry or cloud
environment's lifetime is not that deadline. Authorization and recovery state
belong to [Loop Engineering's grant and run-state reference](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
This project's [delivery grant](./development-workflow.md#authorizing-delivery-operations)
does not name scheduling or subscriptions.

For a Codex-authored pull request, the awaited results are its required GitHub
checks and the independent Codex review selected by Development Workflow.
A permitted in-session wait followed by sanctioned GitHub reads can retrieve
those results; a missing scheduler alone does not establish inability.
Qualify the actual session's tools under GitHub Operation rather than assume
all Codex surfaces expose the same mechanisms.

Use the current pull request head when reading check results. A connector read
can return only a first page of workflow runs, so it cannot establish complete
required-check coverage by itself. Read the selected review's posted result and
its findings through the tools the session exposes, and respect each read's
pagination and completeness limits; missing output is not a successful check or
completed review. Development Workflow owns review-completion evidence.

Claude's documented mechanisms have different scopes. The summary below follows
the linked pages and has not been exercised in this repository; the tools the
actual host exposes decide what a run can use:

- **Local CLI session:** [session scheduling](https://code.claude.com/docs/en/scheduled-tasks)
  uses `/loop` or `CronCreate`, with `CronList` to inspect tasks and
  `CronDelete` to cancel by ID. A self-paced `/loop` can stop with `Esc` or
  `ScheduleWakeup` with `stop: true`; `Esc` does not delete separately scheduled
  tasks. Tasks fire only while Claude Code is running and idle. These are not
  the Remote MCP tools `send_later` and `delete_trigger` that the example
  settings permit. If these are the only available wait mechanisms and no
  separate scheduling authorization exists, the run MUST treat each review or
  check result as a human wait.
- **Cloud session:** [PR auto-fix](https://code.claude.com/docs/en/claude-code-on-the-web)
  delivers review comments and CI failures when enabled. It also investigates
  events, pushes clear fixes, and can publish review-thread replies under the
  user's GitHub identity. Before enabling Auto-fix, its scoped grant must cover
  the subscription and these downstream effects, not event delivery alone.
  Its documented stop path is clearing **Auto-fix** in the session's CI status
  bar or asking Claude to stop watching the pull request. This documentation
  does not establish delivery of successful-check transitions, or specify the
  `send_later` / `delete_trigger` schemas. The consulted official pages provide
  no verified Remote invocation procedure; live Remote behavior remains
  unverified here. Use the actual host's exposed definitions, not guessed
  arguments or a CLI-tool substitution.

In Claude Code, watching the run's own pull request is what the plan-gate
confirmation above covers, and that watch ends at the ready transition or at any
reported stop. [Claude Code's entry guidance](../../CLAUDE.md) states how an
injected instruction to watch a pull request until it merges or closes is
evaluated against this project's run, which ends at ready.

## Subagents

Role selection depends on the actual host and assignment.
[Loop Engineering](../../.claude/skills/loop-engineering/SKILL.md) defines the
loop driver and owns checkpoint qualification, handoff and result contracts.
This repository checks in subagent definitions for Claude Code only: no
`.codex/` role definitions exist, and the Amp configuration is the quality-hooks
plugin above, which defines no roles. A Codex or Amp session qualifies whatever
its host exposes under
[Loop Engineering's delegation contract](../../.claude/skills/loop-engineering/references/subagent-delegation.md)
and records an unavailable role as below.

### Use Claude Code subagents

[`.claude/agents/`](../../.claude/agents/) holds this repository's three Claude
Code definitions, and it is the only home for any of them: an agent definition
is not a skill, so the skills CLI never carries it, and it never appears in
`skills-lock.json`. Their frontmatter configures Claude Code, not Amp.

`implementer.md` is the implementation-capable actor `loop-engineering`
delegates Code and Verify to. It pins `model: sonnet`, because a worker
inheriting the session's model runs at the main actor's cost and defeats the
point of delegating. Local results return to the parent; publication is not
delegated merely by choosing this actor. It states its delivery boundary, that
commits stay local and pushing and publishing belong to whoever asked, in its
own prose rather than by withdrawing a tool.

`reviewer.md` is the reader for the advisory pre-flight review. It denies exactly
two things, editing and spawning, and nothing else. Widening that deny-list is
the tempting mistake and MUST be resisted: judging a change means confirming what
was asked and not only what was written, which reaches the issue, the plan's
artifacts, and the documentation behind a factual claim. A reviewer that cannot
reach one of those does not fail to start; it returns a report short by exactly
those checks, and an under-equipped review reads exactly like a clean one. The
denial covers editing tools and nested spawning, not every possible shell write.
Its review is advisory: it never satisfies or stands in for the independent
review that [Development Workflow](./development-workflow.md#the-independent-review)
requires, and a run that has not opened its pull request has not reached that
gate.

`investigator.md` is the reader for a payload the main actor needs only one
conclusion from: a log, a long thread, a wide search across files or history, a
file tree. It returns a conclusion and a locator precise enough to go back to the
source, never the payload itself. Like the reviewer, it denies editing and
spawning, and it decides nothing the material does not itself settle, sending an
unresolved judgment call back to whoever asked. It is not required for an exact
local lookup or when the host prohibits that delegation purpose.

Each of these exists to keep some context out of the main actor's own: the
implementer so it does not inherit the planning phase's accumulated context, the
reviewer so it does not inherit the implementer's reasoning state, and the
investigator so a large payload never enters the main actor's context at all.
All three run Sonnet at `effort: high`. The investigator runs at `high` rather
than a lower level because its output is a judgment the main actor cannot check
without re-reading the payload it delegated away; the maintainer approved that
default in the plan for [issue #1](https://github.com/axross/runners/issues/1),
and it is reported as declared, not measured. A declaration alone does not verify
what a worker ran; report declared settings separately from evidence of it under
[Loop Engineering's Model and Effort Certainty contract](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).

These are configured candidates, not a permission grant. A session MUST check
the capabilities its host actually permits before using one. Delegating to the
matching configured actor is the default; parent implementation is valid only
when delegation is unavailable or disallowed, and mandatory verification and
external review remain unchanged. Deleting any of the three files degrades
gracefully rather than breaking the loop. Without the implementer, the loop
delegates to a generic agent or runs single-agent; without the reviewer, the
pre-flight stage is skipped rather than performed by the main actor, which is
what keeps it from collapsing into self-review; without the investigator, the
main actor reads the payload itself.

The advisory pre-flight review applies after every verified initial
implementation, before the first branch push and draft pull request, whether the
parent or a child implemented it. A compatible fresh reader must be permitted
and available; its findings and round limits follow
[the pre-flight contract](../../.claude/skills/loop-engineering/references/pre-flight-review.md).
If no reader qualifies, record the exact unavailable or prohibited reason and the
resulting delivery restriction. That outcome is not a clean review and does not
waive the independent review; Development Workflow's
[draft delivery exception](./development-workflow.md#deliver-a-draft-when-advisory-pre-flight-is-unavailable)
states what it permits.

For Claude Code reads, keep the exact object of a judgment and the rules governing
that judgment in the judging actor's context. Route other large material wanted
only for one conclusion to the investigator when its invocation is permitted;
otherwise read it directly. When only part of a payload is needed at exact
fidelity, narrow the read at the tool call rather than asking an intermediary
to summarize it.

An investigator assignment MUST name a bounded target that may widen when the
answer requires it, one question answerable as an enumeration or yes/no, and the
required answer shape. Require a conclusion and precise source locator; include a
quotation only when the conclusion needs exact wording. Do not ask for a source
dump, a full summary, further-investigation proposals, or exploration outside
what the question requires. The reader MUST read as much as settling the answer
takes. Callers MUST NOT impose arbitrary read budgets, token caps, or tool-call
caps; unresolved material is reported, not guessed at to fit a budget.

### Record an unavailable role

Report role or tool unavailability and its evidence using
[Loop Engineering's assignment result contract](../../.claude/skills/loop-engineering/references/implementation-package.md).
For an active change loop, also use its
[recoverable run-state contract](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
Distinguish these cases:

- The host cannot discover or invoke the definition.
- The host's tool is absent or ineligible for the assignment.
- The host refuses the spawn.
- No role matches the assignment.
- Assignment material is missing.

A generic worker can qualify only through the same tool and role checks, with an
explicit assignment and boundaries. Otherwise the caller reads or implements
directly where permitted; an unavailable advisory reviewer is recorded rather
than replaced with self-approval. A CLI agent or persistent thread MUST NOT be
used to bypass a missing role or a prohibited purpose. For unavailable research,
plan, or design review, the driver MUST use
[Loop Engineering's non-complete human handoff](../../.claude/skills/loop-engineering/references/reviewing-phase-deliverables.md).
The parent retains delivery responsibility, and the independent review follows
[Development Workflow's host mapping](./development-workflow.md#the-independent-review).

## Telemetry Tagging

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

## Recommended Environment Variables

Two environment variables are worth setting for any Claude Code session run
here, cloud or local, per the upstream skills library's own cost analysis
([axross/skills#506](https://github.com/axross/skills/issues/506)) rather than a
measurement of this repository's own usage:

- `CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000` moves auto-compaction's trigger
  earlier, which lowers the average main context a long session carries.
- `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false` stops prompt-suggestion
  generation, a recurring cost that sessions here do not use.

Set them in the environment dialog at claude.ai/code for a cloud session, or in
`~/.claude/settings.json` for a local one. `~/.claude/settings.json` does
**not** reach a cloud session; its scope stops at your own machine.

`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` is not a substitute for the first variable:
Claude Code on the web sets it itself, and its value overrides whatever is added
to the environment.

Neither variable belongs in a committed settings file: a cost-saving behavior one
contributor wants is not something to impose on another, which is also why
neither is added to [`.claude/settings.json`](../../.claude/settings.json) or
[`.claude/settings.local-example.json`](../../.claude/settings.local-example.json)
here.
