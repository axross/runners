# Agent Sessions

How agent sessions start in this project, the hooks that run during one, how Amp
and Codex runs handle delivery authorization, Claude Code's plan-gate delivery
confirmation, Amp's plan-gate confirmation for archiving review child threads,
host-owned CI and review waits, the write-fidelity procedure for GitHub text a
Claude Code cloud session writes, the subagents a session can delegate to,
including the Claude Code review child session, how a session's usage is
tagged, and the environment variables recommended for cutting a Claude Code
session's cost. Claude Code and Amp are the hosts this repository configures;
they share the pinned toolchain in [`mise.toml`](../../mise.toml) and nothing
else. Codex reads [`AGENTS.md`](../../AGENTS.md) and has no configuration here
beyond the delivery-authorization guidance below.

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
`mise trust` and `mise install`, runs [`.agents/install-rust`](../../.agents/install-rust)
(see [Rust Provisioning](#rust-provisioning)), persists mise activation and the
`~/.cargo/bin` `PATH` entry in the login shell, and verifies that every pinned
tool resolves, including `cargo`, `rustfmt`, and `clippy` for the toolchain
`cli/rust-toolchain.toml` pins. The login-profile block for Rust has its own
marker, so an environment provisioned before Rust support gains the line when
setup runs again. When changing the pinned mise version, update it in every place
together: the `version` and `sha256` inputs of each `jdx/mise-action` step in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) and
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml), and the
`version` and `expected_sha256` in `.agents/setup`. Both hold the SHA-256 of the
bare `mise-v<version>-linux-x64` binary, taken from the release's published
`SHASUMS256.txt`; the action hashes the binary it extracts from its archive, so
it does not use the archive's checksum.

## Amp Delivery Authorization

Amp can frame a task before reading [`AGENTS.md`](../../AGENTS.md), which is
why AGENTS.md's Amp routing requires this section before any delivery effect.
The project's agreement requests the Loop Engineering route, including a draft
pull request and the independent review selected by
[Development Workflow](./development-workflow.md), rather than stopping at a
local commit. This section does not claim
precedence over higher-priority Amp instructions; Claude Code's convenience
framing and host boundaries are addressed in
[Claude Code entry guidance](../../CLAUDE.md).

The standing delivery grant in
[Development Workflow](./development-workflow.md#authorizing-delivery-operations)
is the maintainer's authorization for the effects it covers. It does not
replace the plan and review gates or extend to what the grant excludes.

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

## Codex Delivery Authorization

Codex authoring MUST follow the same plan, review, scoped-grant, and
conflict-reporting contract as
[Amp delivery authorization](#amp-delivery-authorization), above.
Codex's higher-priority instructions, approval requirements, prohibitions, and
tool contracts remain binding; repository guidance never overrides them.
This section covers delivery only, not Codex provisioning or startup.

## The Session-Start Hook

In a cloud session, [`.claude/hooks/session-start.sh`](../../.claude/hooks/session-start.sh)
activates mise, runs `mise trust` and `mise install` to provision the pinned
toolchain, runs [`.agents/install-rust`](../../.agents/install-rust) to provision
the pinned Rust toolchain, copies the opt-in quality hooks into place, and echoes a pointer to
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

The Rust provisioning follows the same rule: the hook calls
`.agents/install-rust` as warn-and-continue, with its output on standard error, so
a network failure never fails a session start. The Rust tasks then fail at once
with a line naming the script. The hook puts `~/.cargo/bin` on its own `PATH` and
appends that line to the session environment file once, so a later run of the hook
in the same session does not repeat it.

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
particular wake tool. The Waiting on CI and Review section below
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
[Development Workflow](./development-workflow.md) states what that leaves
uncovered.

## Rust Provisioning

[`.agents/install-rust`](../../.agents/install-rust) makes the Rust gates usable in
a session. It does nothing when the toolchain `cli/rust-toolchain.toml` names
already has `cargo`, `rustfmt`, and `clippy`. Otherwise it installs `rustup` when
that is absent, from a pinned `rustup-init` release for
`x86_64-unknown-linux-gnu` whose SHA-256 the script checks before running it, with
no default toolchain, the minimal profile, and no shell-profile edits. Then it
runs `rustup toolchain install` from `cli/`, which installs the pinned channel and
its components. `RUSTUP_HOME` and `CARGO_HOME` are honoured, so the script can be
exercised against an empty directory. On any other platform it stops with a
message: install the pinned toolchain with `rustup` there.

`.agents/install-rust --check` runs only that readiness test and exits 0 or 1,
installing nothing, on any platform and against the caller's `PATH`. The
`rust:require` task and the verification step of `.agents/setup` call it, so the
probe is written once.

[`.agents/setup`](../../.agents/setup) and the session-start hook both call the
script, so the `rustup-init` version and hash live in the script alone. Refresh
them together, with the hash computed from the real download and checked against
the `.sha256` file Rust publishes beside it; Dependabot does not cover them. The
Rust toolchain itself is the recorded exception to the checksum rule in
[Security](../conventions/security.md).

A Rust task run without the toolchain fails at once with one line that names this
script, never with a long tool error and never by skipping, so a session cannot
report a change as checked when the Rust gates did not run.

## Confirming Delivery at the Plan Gate in Claude Code

Claude Code's host conditions pull request creation on an explicit request from
the user. The checked-in request in [`AGENTS.md`](../../AGENTS.md) does not
establish that on its own, so a Claude Code run collects that request where the
human is already answering: in the plan-approval question.

Every plan-approval question, including the one for a revised plan, MUST also
ask the human to confirm, in the same answer:

- delivery of the run's draft pull request;
- creation of a fresh Claude Code review child session that reviews it and
  publishes directly to it, or, when the maintainer has selected the Claude CI
  alternative for the run, the Claude CI review request on it;
- watching that pull request's activity, where the host offers it, until the
  ready transition or a reported stop.

Only pull request creation, and `TaskStop` where its live contract conditions
it, carry a host condition. The maintainer chose
to settle the review item and the watch in the same answer, so that one
answer covers the whole delivery path. This bounded exception to dependent
question ordering is recorded in the delivery, review, and watching deviation in the register of
[Agent Skills](./agent-skills.md).
The run MUST offer coherent combined outcomes in that single question, with
review and watching inapplicable when delivery is declined.

A confirming answer is the user's explicit request for that run's pull
request. The run then follows the post-approval sequence in
[Development Workflow](./development-workflow.md)'s After Approval section,
which also says when a run obtains this confirmation. For a revision approved after the pull request exists, the
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
  resumes the run, asks for the same confirmation once, before any further
  change.
- MUST, when the human declines delivery, stop after push, report the
  external-review gate as unmet and the loop as not complete, and substitute
  no other review.
- MUST, when composing the plan-approval question, check the live `TaskStop`
  contract; when it requires the human's request, ask for it as a separate
  question after approval, as the Amp section below does for archiving, and
  record the answer in run state. A run with no plan gate, such as review-only
  recovery, asks once before its first `TaskStop`, and a fresh session that
  resumes the run and lacks that exchange in its own conversation asks once
  before its first `TaskStop`, even when run state records an earlier
  confirmation. A recorded decline carries forward, so no later session asks
  again, and a child still running is handed over as one the run cannot stop.
- MUST, when the human confirms delivery but declines the review item,
  open the draft pull request, keep it draft, and report the external-review
  gate as unmet and the loop as not complete.
- MUST, when the human confirms delivery but declines watching, still request
  review, then treat each review or check result as a human wait: end the
  turn and resume on the human's next message, with no scheduling.

## Confirming Review-Thread Cleanup at the Plan Gate in Amp

An Amp run reviews through a review child thread per round, under the Amp review
child thread section of [Development Workflow](./development-workflow.md). Amp's
archive tool's only condition that fits this route is the user's request: its
self-archiving option would archive the child before the run verifies the
review, and the route adopted in [`AGENTS.md`](../../AGENTS.md) does not make
that request on its own. Every Amp run MUST therefore ask the human, within the
plan-approval exchange as a separate question after the plan is approved, to
confirm archiving the run's review child threads once each round's result is
verified or the round has failed. That answer is the plan-gate archive
confirmation. A plan revision approved in the same session does not ask again.
A run with no plan gate, such as review-only recovery, asks once before its
first archive.

**Guidelines:**

- MUST record the answer in run state. The confirming answer applies to the
  session that collected it, as the archive tool's condition does: a fresh
  session that resumes the run and lacks that exchange in its own conversation
  asks once, before archiving, even when run state records an earlier
  confirmation.
- MUST, when the human declines, still run the review rounds and leave the
  child threads unarchived, handing each one over as the Amp review child
  thread section of [Development Workflow](./development-workflow.md)
  describes. A recorded decline carries forward, so no later session asks again.
- MUST NOT read this confirmation as covering any other thread, or as granting
  any delivery effect the standing grant does not name.

## Waiting on CI and Review

The host supplies the wait mechanism. The portable contract lives in
[Loop Engineering's independent-review reference](../../.claude/skills/loop-engineering/references/independent-review.md),
whose Waiting Bound section owns capability resolution, deadlines, budget
resets, human waits, and teardown at each stop. A scheduler's expiry or cloud
environment's lifetime is not that deadline. Authorization and recovery state
belong to [Loop Engineering's grant and run-state reference](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
This project's delivery grant, in [Development Workflow](./development-workflow.md),
does not name scheduling or subscriptions.

For a Codex-authored pull request, the awaited results are its required GitHub
checks and the independent Codex review selected by Development Workflow.
A permitted in-session wait followed by sanctioned GitHub reads can retrieve
those results; a missing scheduler alone does not establish inability.
Qualify the actual session's tools under GitHub Operation rather than assume
all Codex surfaces expose the same mechanisms.

For an Amp-authored pull request, the awaited results are its required GitHub
checks and the review its Amp review child thread posts. Wait for the child with
Amp's thread-wait tool, then read the posted review back on the pull request;
Development Workflow owns what counts as the result. Wait for checks with an in-session
wait followed by sanctioned GitHub reads, or with a subscription only where the
host and the run already permit it.

For a Claude Code-authored pull request, the awaited results are its required
GitHub checks and the review its Claude Code review child session posts, or,
when the maintainer selected the Claude CI alternative for the run, the Claude
CI review.
Claude Code's [subagent documentation](https://code.claude.com/docs/en/sub-agents)
says subagents run in the background by default and that, while fork mode is
on, an interactive session cannot block on one, so the child's completion
arrives as a notification rather than as the result of a blocking wait; this
has not yet been exercised in this repository. After it arrives, read the posted
review back on the pull request. A child's permission prompt appears in the
authoring session, and one the run cannot answer for the human is a human wait,
as the After Approval section of [Development Workflow](./development-workflow.md)
lists.

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

## GitHub Write Fidelity in Claude Code Cloud Sessions

In a Claude Code cloud session, an agent writes issue bodies, comments, and pull
request bodies through the host's hosted GitHub MCP tools. On 2026-11-19 (PST),
two `issue_write` updates stored a 151-character link destination wrapped in a
double-backtick code span, which stops it working as a link. The model's tool
input had no wrapper. The layer that adds it is unknown: the client's MCP
transport, the Anthropic gateway, the hosted `github-mcp-server`, or GitHub. A
session can observe only its tool input and what GitHub then stores.

This section gives the rules that keep composed text clear of the rewrites
observed so far, the commands that check it, and the read-back that shows what
GitHub stored. The evidence is the
[tracking issue](https://github.com/axross/runners/issues/144) and its
[probe-results comment](https://github.com/axross/runners/issues/144#issuecomment-7033840359).
The findings are observations, not guarantees. The rewrites belong to the hosted
write path and can change without notice, so the read-back stays mandatory. A
check before writing cannot see what GitHub stores; it only prevents the
rewrites already known.

The section covers writes from a Claude Code cloud session through those MCP
tools. Amp and Codex use other write paths, which were not tested. Neither were
the MCP tools that write file contents. Commits pushed with git are expected to
go through the session's git remote instead of this path; that was not tested
either.

### The Rewrites Observed

A probe on a scratch issue, run on 2026-11-19 (PST) with the maintainer's
approval, wrote URLs of known lengths and read back what GitHub stored. Each
cell lists the lengths tested.

| Form                              | Stored intact                                                                       | Stored wrapped in a code span                                                        |
| --------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Inline link destination           | 59, 65, 91, 104, 107, 115, 130, 139, and each of 140 to 149 characters (18 lengths) | 150, 151, and 160 characters in the probe; 151 characters in the two incident writes |
| Titled link, reference definition | Not tested below 150 characters                                                     | 150 characters                                                                       |
| Bare URL, autolink                | 149 and 150 characters                                                              | 200 characters                                                                       |

The threshold for a bare URL or an autolink lies above 150 and at or below 200
characters. The rewrite was deterministic, did not depend on the URL's content
or the surrounding line, and applied to every qualifying URL in a body. One
comment was tested, and its 151-character link was wrapped too. A wrapped
inline link or reference definition no longer works as a link, and a wrapped
autolink also shows `&lt;` and `&gt;` to readers.

Two more rewrites were observed:

- **The comment footer.** The host instructs agents to end every comment,
  review, and reply with `\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_`.
  One comment sent without it was stored with it appended (comment 7032656088,
  2026-11-19 PST). Four comments that already ended with it were stored
  unchanged, with no second copy (comments 7032949620, 7033812409, 7033840359,
  and 7044614711, 2026-11-19 and 2026-11-20 PST). Whitespace after the footer
  was not tested.
- **U+200B.** It was removed when an issue was created, the only write path
  tested. MCP reads also drop it; REST reads keep it. U+200C, U+200D, U+2060,
  and U+FEFF were not tested.

No other unintended change was found for the constructs tested. These were
stored byte-identical, and MCP reads returned them unchanged: the character
references `&amp;`, `&#x27;`, `&lt;`, `&gt;`, `&quot;`, and `&#39;`; raw
`& < > ' "`; angle-bracket placeholders; HTML comments; `<details>`; and fence
info strings. The three sanitized-read stages that GitHub Operation's Body
Integrity describes were not reproduced.

An open report, `github/github-mcp-server#3299`, was still open on 2026-11-19
(PST), as read through a model-summarized fetch of its public page. It reports
long URLs wrapped in code spans, including link destinations, bare URLs, and
autolinks. It also reports an autolink's angle brackets escaped and the closing
backticks sometimes landing after the link's `)`. Its threshold of 104
characters and its wrapper shape differ from what was measured here, so it may
be the same defect, but that is not established.

### Rules Before Writing

The rules apply to every issue body, comment, review, reply, and pull request
body an agent writes from the session. The 149-character cap rests on the
threshold measured for this endpoint, not on the 104 characters the open report
gives. If that lower threshold ever applies here, the read-back catches it.

**Guidelines:**

- MUST compose the text in a local file that holds exactly what will be sent.
  An issue or pull request body may end with a newline or not, but the file and
  the sent text must agree; every stored probe body kept its trailing newline.
  A comment ends with the host footer and no newline after it. Keep the file
  out of the commit, for example in the session's scratch directory.
- MUST keep every URL at 149 characters or fewer, in any form.
- MUST NOT write U+200B, U+200C, U+200D, U+2060, or U+FEFF.
- MUST end each comment, review, and reply with the exact host footer and
  nothing after it.

### Checks Before Writing

Run these on the composed file, named `body.md` below. Each prints an explicit
result when it finds nothing, and none reports a pass for a file it cannot read.
They need `node`, GNU `grep`, `tail`, and `cmp`; `node` is pinned in
[`mise.toml`](../../mise.toml).

The URL check lists every `http` or `https` URL of 150 or more characters with
its line number and length. It does not count a closing `)` that belongs to the
link syntax. It may count trailing punctuation, which errs on the safe side. It
does not check relative or non-`http` destinations, which were not tested.

```sh
node -e '
const lines = require("fs").readFileSync(process.argv[1], "utf8").split("\n");
let found = false;
lines.forEach((line, i) => {
  for (const [match] of line.matchAll(/https?:\/\/[^\s<>]+/g)) {
    let url = match;
    while (url.endsWith(")") && url.split(")").length > url.split("(").length) url = url.slice(0, -1);
    if (url.length >= 150) { found = true; console.log(`${i + 1}: ${url.length} characters`); }
  }
});
console.log(found ? "shorten the URLs above" : "ok: no URL of 150 or more characters");
' body.md
```

The invisible-character check lists every line that contains one of the five
characters.

```sh
if LC_ALL=C.UTF-8 grep -nP '[\x{200B}-\x{200D}\x{2060}\x{FEFF}]' body.md
then echo "remove the characters above"
elif [ $? -eq 1 ]; then echo "ok: no invisible characters"
else echo "check failed"; fi
```

The footer check applies to a comment, review, or reply. It accepts text that ends exactly with
the footer and rejects text with no footer or with a newline after it.

```sh
printf '\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_' > footer.txt
tail -c "$(wc -c < footer.txt)" body.md | cmp -s - footer.txt \
  && echo "ok: ends with the footer" || echo "footer missing or not at the end"
```

### Reading Back a Body

After writing an issue or pull request body, compare what GitHub stored with the
file that was sent, by command and not by eye. First note the update time that
an MCP get of the item shows after the write. Then read the stored body as
structured data through a route qualified under
[GitHub Operation](../../.claude/skills/github-operation/references/channel-selection.md):

- **The direct REST read** of the issue or pull request, where the session may
  use it.
- **The REST search read**, otherwise: `is:issue` or `is:pr`, sorted by update
  time, newest first, 100 per page, with the item selected by number. No title
  goes in the query, so quotes or apostrophes in a title cannot break it.

Whether a session may use either route is a fact about that session, not about
Claude Code cloud sessions in general. Establish it for the operation and target
in hand, and report the limit observed, not a limit of the host. MCP reads are
a quick check only: they drop U+200B, and their result reaches the agent's
context but not a command, so they cannot supply a byte comparison.

The direct read writes the item to a file, prints its update time, and compares
the body. Use `pulls/<number>` in place of `issues/<number>` for a pull request.

```sh
gh api repos/axross/runners/issues/<number> > stored.json
jq -r .updated_at stored.json
jq -j .body stored.json | cmp - body.md && echo "ok: stored body is identical"
```

The search read selects the item by number. Use `is:issue` for an issue.

```sh
gh api -X GET search/issues -f q='repo:axross/runners is:pr' \
  -f sort=updated -f order=desc -f per_page=100 > stored.json
jq -r '.items[] | select(.number == <number>) | .updated_at' stored.json
jq -j '.items[] | select(.number == <number>) | .body' stored.json \
  | cmp - body.md && echo "ok: stored body is identical"
```

`jq -j` writes the raw string with nothing added, and the pipe feeds it straight
to `cmp`. No shell variable holds the body, so a trailing newline cannot be lost
or added. The command therefore compares the exact structured field, as Body
Integrity requires. A difference prints a `cmp` line and no `ok`, including a
difference of one trailing newline.

**Guidelines:**

- MUST treat an item missing from search, or an update time older than the one
  noted, as a read that is not current yet, not as a mismatch. Retry up to three
  times over about five minutes, using only a wait the session itself provides,
  never a scheduler. When the session has no such wait, or the read is still not
  current, report the write as unverified.

### Reading Back a Comment

A session that can read a comment only through MCP has no mechanical comparison
route for it. The agent checks the comment as MCP's comments read returns it:
each URL is unwrapped, and the footer appears exactly once, at the end. This
check is not byte-exact, because MCP reads drop U+200B.

### Recovering from a Difference

The rewrite is deterministic, so identical text written again is stored the same
way.

**Guidelines:**

- MUST NOT re-send identical text.
- MUST confirm, before any corrective write, that the stored text differs from
  the intended text only by the known patterns: a URL wrapped in a code span, an
  appended footer, or a removed invisible character. Any other difference,
  including a change someone else made in the meantime, stops the operation for
  reconciliation under GitHub Operation's
  [Body Integrity](../../.claude/skills/github-operation/references/body-integrity.md).
- MUST, for a body on a target the delivery grant in
  [Development Workflow](./development-workflow.md) covers, compose the
  corrected text from the intended text with the URL shortened, write it once,
  and read it back again. When the corrected write also differs, report the
  write as unverified and stop that operation.
- MUST, for a status comment on the run's tracking issue, append a corrected
  status comment that supersedes it instead of editing the original. Status
  comments stay append-only.
- MUST, for any other comment, report it as unverified and ask the maintainer,
  because the delivery grant names no corrective comment for it.
- MUST report a difference of a kind this section does not list as an
  unverified write, not work around it.

### The Plan Body

A plan's identity is derived from its stored bytes, so a rewrite of the plan
body changes what the maintainer approves.

**Guidelines:**

- MUST re-derive the plan's identity from the stored bytes after publishing or
  revising it, using the body read-back above.
- MUST fix a difference found before approval with a corrective write that
  produces a new revision. That revision goes through plan review under the
  loop's review budget before approval is asked for.
- MUST stop the run before any edit when an identity mismatch appears after
  approval, under Plan Revision Identity in
  [Loop Engineering's plan document reference](../../.claude/skills/loop-engineering/references/plan-document.md).
- MUST treat replacing a URL in an approved plan as a plan revision that needs
  new approval.

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
Its review is advisory; the independent-review rule in
[Development Workflow](./development-workflow.md) says it never stands in for
that review.

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
resulting delivery restriction. That outcome is not a clean review; the Deliver a
draft when advisory pre-flight is unavailable section of
[Development Workflow](./development-workflow.md) states what it permits.

The independent review's Claude Code review child session is none of these
three. Each round starts a new child with the built-in `general-purpose`
subagent type and `isolation: worktree` on the call itself, never as a fork,
because a fork inherits the author's conversation. `reviewer.md` cannot serve:
it enforces its tool limits but tells its reader not to post, and the
independent review must be posted on the pull request. Claude Code's
[subagent documentation](https://code.claude.com/docs/en/sub-agents) says a
subagent loads the project's CLAUDE.md, which here imports AGENTS.md; this has
not yet been exercised in this repository. AGENTS.md's Code Review Rules exempt
the child from the delegation rule. The Claude Code review child session
procedure in [Development Workflow](./development-workflow.md) owns its
assignment, checkpoints, and ending, and the register in
[Agent Skills](./agent-skills.md) records where it departs from Loop
Engineering's review-child defaults.

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
the host mapping in the Independent Review section of
[Development Workflow](./development-workflow.md).

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
