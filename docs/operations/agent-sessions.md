# Agent Sessions

How agent sessions start in this project, the hooks that run during one, how Amp
and Codex runs handle delivery authorization, how a session asks the maintainer
(launched sessions and subagents included), Claude Code's plan-gate delivery
confirmation, Amp's plan-gate confirmation for archiving review child threads,
CI and review waits, the write-fidelity procedure for GitHub text a Claude Code
cloud session writes, the subagents a session can delegate to, how a session's
usage is tagged, and the environment variables recommended for cutting a Claude
Code session's cost. Claude Code and Amp are the hosts this repository
configures; they share the pinned toolchain in [`mise.toml`](../../mise.toml)
and nothing else. Codex uses the project entry in the intended checkout.

## Starting Codex in the intended checkout

From the runners checkout root, pass the task to Codex.

```sh
codex 'Use Technical Document Authoring to update the contributor launch guide.'
```

Codex starts in the directory containing [`AGENTS.md`](../../AGENTS.md).
Its Response Approach routes the change through Professional Behavior, Software
Development and Loop Engineering before planning, delegation or edits. A named
skill also applies to its subject.

From the parent directory, put the checkout and the route in the **initial
task**. Replace `./runners checkout` with the intended checkout's path and the
last sentence with the task. A launcher that already starts Codex above the
checkout can pass the same text.

```sh
codex 'Enter the runners checkout at "./runners checkout".
Read its scoped AGENTS.md and follow its Response Approach, including the
prerequisites and Loop Engineering, before planning, assigning work or editing.
Use Technical Document Authoring to update the contributor launch guide.'
```

### Launch walkthrough

Tracing the two examples reaches the same Response Approach. From the root the
entry is in the launch directory. From the parent the initial task identifies
the child even if a sibling also contains an AGENTS.md. In either case the
first update can give the phase as intake and the tracking issue or approval
as unknown while the actor checks them.

- **Continue an issue with new scope.** Follow
  [Retrieve state on resume](./development-workflow.md#retrieve-state-on-resume)
  from the named issue. Compare its canonical revision, recorded human approval
  and pending phase with the current checkout and existing edits. Preserve
  those edits, revise the plan for the expansion and obtain the maintainer's
  approval before implementing or delegating the new work.
- **Continue unchanged approved scope.** Resume at the recorded pending phase.
  For example, when verification and pre-flight allow a push, an append-only
  push to the run's working branch matches the adopted grant. Hold a request
  to assign an additional person to the pull request until separately
  authorized. Check the actual host and tool conditions for each effect.
  [Codex delivery authorization](#codex-delivery-authorization) routes through
  [Amp delivery authorization](#amp-delivery-authorization) to the
  [adopted grant](./development-workflow.md#authorizing-delivery-operations).
- **Ask a standalone read-only question.** For “Where is Amp provisioned?” the
  project entry points directly to [Amp Environment Provisioning](#amp-environment-provisioning)
  and [`.agents/setup`](../../.agents/setup). After loading the applicable
  prerequisites, answer the question directly.

The shell walkthrough captured the working directory and initial task passed to
a stand-in for Codex, then traced the project links. Startup by a live host and
an actor receiving and following the guidance were unobserved.

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

[`.claude/settings.json`](../../.claude/settings.json) registers two
independent SessionStart commands. [The reminder](../../.claude/hooks/session-guidance.sh)
directs the actor to the checkout's AGENTS.md, its prerequisites and Loop
Engineering before planning, delegating or editing a change. The reminder
names [`AGENTS.md`](../../AGENTS.md) directly so the actor can read the working
agreement even on a host that treats the `@AGENTS.md` import in
[`CLAUDE.md`](../../CLAUDE.md) as literal text.

[The bootstrap](../../.claude/hooks/session-start.sh) activates mise, runs
`mise trust` and `mise install`, calls [`.agents/install-rust`](../../.agents/install-rust)
and copies the opt-in quality hooks. A bootstrap failure leaves the reminder
independently runnable. Prettier and markdownlint come from mise.

Both commands exit silently unless `CLAUDE_CODE_REMOTE=true`. A local session
manages its own toolchain. Set that variable by hand to opt into the remote
behavior locally.

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

The registration also sets the session's default reasoning effort,
`effortLevel`, to `xhigh`. Claude Code reads those settings at session start.

### SessionStart fixture results

At the baseline for [issue #166](https://github.com/axross/runners/issues/166),
invoking the registered command in a checkout path containing spaces exited
127 with zero reminders. In a plain path, forcing the settings copy to exit 42
produced status 42 and zero reminders.

The [regression fixture](../../.claude/tests/session-start.test.mjs) behind
`mise run check:claude` invoked the commands from the checked-in registration
in disposable checkout directories with an isolated home, mise and Rust
stand-ins, and a failing copy stand-in for the fatal case.

| Case                                                          | Reminder on stdout                 | Bootstrap result                                                       |
| ------------------------------------------------------------- | ---------------------------------- | ---------------------------------------------------------------------- |
| Remote, checkout path with spaces                             | One before provisioning            | Exit 0, environment written and settings copied                        |
| Remote, Rust installer failed                                 | One before provisioning            | Warning on stderr, exit 0, environment written and settings copied     |
| Remote, settings copy failed, plain path and path with spaces | One when invoked after the failure | Copy error on stderr, exit 42, environment written and settings absent |
| Local, remote flag unset                                      | Zero                               | Exit 0, silent, no provisioning or settings writes                     |

The fixture exercised command execution. Actual host dispatch, delivery to an
actor and the actor's compliance were unobserved. Live actor trials and changes
to shared Cloud configuration require separate authorization.

## The Claude Code Quality Hooks

Format-on-edit and check-before-stop are **opt-in**. They live in
[`.claude/settings.local-example.json`](../../.claude/settings.local-example.json),
which the session-start hook copies to the gitignored `settings.local.json` in a
cloud session; Claude Code hot-reloads them for that session. A local session
skips the hook entirely, so opting in there stays a manual copy.

That example file also contains `permissions.allow` entries for
`mcp__claude-code-remote__subscribe_pr_activity` and
`mcp__claude-code-remote__unsubscribe_pr_activity`, and for no other tool. They
exist only to spare a permission prompt when a Claude Code Remote session
subscribes to its own pull request's activity under the plan-gate confirmation
and unsubscribes when the watch ends, as the Waiting on CI and Review section
below describes. They are tool-permission rules, subject to Claude Code's
[permission precedence](https://code.claude.com/docs/en/permissions), not
standalone evidence of live availability, human adoption, or a scoped operation
grant. The example allows no scheduling tool: an agent schedules a check-in only
when no subscription route exists and the human explicitly permitted it, and a
standing allowance is not that permission.

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

## Asking the Maintainer

[`AGENTS.md`](../../AGENTS.md) states, for every host, the rule for asking the
maintainer. It states the rule itself, not a pointer to this section, because
AGENTS.md is the one file every session loads, and a session can miss a route it
must first follow. This section names Claude Code's mechanism, says how launched
and launching sessions on any host apply that rule, and covers Claude Code
subagents and forks. The installed
[Asking the Human](../../.claude/skills/professional-behavior/references/asking-the-human.md)
practice owns how a question is framed, recovered, and made to fall back, and
this section does not restate it.

In Claude Code the mechanism is `AskUserQuestion`. The maintainer owns every
decision that [`AGENTS.md`](../../AGENTS.md),
[Development Workflow](./development-workflow.md), and
[Loop Engineering](../../.claude/skills/loop-engineering/SKILL.md) leave to the
maintainer, among them scope, product behavior, an architecture choice, a
finding's disposition such as dismissing a Critical or Major advisory finding,
an additional review round, and a plan approval with its delivery confirmation.
In plan mode, `ExitPlanMode` carries plan approval instead, as that tool's live
contract states. Plan-mode approval, like approval that arrives some other way,
MUST follow the plan-gate confirmation's rule for collecting the delivery
confirmation.

**Guidelines:**

- MUST ask each such decision with `AskUserQuestion` whenever the session
  exposes it and its live contract qualifies it for that question, however the
  session was started. MUST NOT list options as prose in the turn output in place
  of the question unless no mechanism is eligible.
- MUST begin a wait for a maintainer decision with the question, and handle each
  of its three outcomes as the Choosing the Route section of the practice
  states: answered by the maintainer; accepted and still awaiting a reply; or
  closed, cancelled, errored, reported missing, or ended without an answer. For
  the last outcome, MUST NOT end the turn with the dependent work blocked before
  that section's turn-output fallback has been used.
- MUST NOT ask a question for a wait on review or check results. Those waits are
  as Waiting on CI and Review below describes.

### Judge eligibility from the session

**Guidelines:**

- MUST judge eligibility from the session's tool list and the tool's live
  contract.
- MUST NOT read any of these as making the tool ineligible: a seeded prompt's
  framing as an automated or "not from the user" event, which says who sent that
  message and not whether the maintainer can answer a question; cloud or
  headless hosting; or the absence of earlier human messages.
- MUST treat only a human-authored answer to the question, not the tool's
  acceptance or an error, as showing that the maintainer answered it, and MUST
  NOT conclude from an error alone that no one can answer.
- MUST let the host's own statement that the tool is unavailable or prohibited
  for the session govern, and report the conflict.

### Ask from a launched session

A launched session is a separate session that another session started, with
`create_session` or otherwise. A subagent is not one.

When a launched session has an eligible mechanism, the maintainer can answer in
that session's own conversation. Options sent to the launcher, or left as prose,
reach the maintainer only if someone finds them and carries the answer back. A
launcher's wording about channels says how the launcher wants to hear about a
decision, not who decides it or where the maintainer answers, so it adds a duty
to report rather than replacing the question.

**Guidelines:**

- MUST ask the maintainer directly with its own eligible mechanism, and report to
  its launcher at each outcome:
  - while the question is with the maintainer and still pending, saying so, so
    the launcher does not ask it again;
  - when the maintainer answers, with the answer;
  - when the question ends without an answer, with what stays blocked.
- MUST send the pending report in the same turn as the question, alongside it or
  just before it. A report MUST NOT take the question's place or end the turn
  before the question is asked.
- MUST read a launch prompt's statement that the launcher handles communication
  with the maintainer, or its instruction to stop and report or to send the
  options to the launcher, as the duty to report. A session MUST NOT read such
  wording as making its mechanism ineligible or the launcher its only channel.
- MUST take only the maintainer's own message in the launched session's
  conversation as routing a decision some other way. A launch prompt that quotes
  or paraphrases the maintainer does not, and a session that is unsure MUST ask.
- MAY treat a maintainer answer its launcher passes on, quoted with where and
  when the maintainer gave it, as settling a decision it is waiting on. MUST NOT
  treat it as a rerouting, a plan approval with its delivery confirmation, or a
  tool request the host conditions on the user's own request, and MUST ask when
  unsure whether it settles a decision.
- MUST return the question and its partial results to the launcher only when no
  mechanism is eligible and the session cannot reach the maintainer, as the
  Execution Result section of
  [Loop Engineering's assignment contracts](../../.claude/skills/loop-engineering/references/implementation-package.md)
  states.

### Launch a session

A session that writes another session's starting prompt for separately owned
work, with `create_session` or otherwise, shapes the route the launched session
takes.

**Guidelines:**

- MUST say in the prompt that maintainer decisions are asked directly with the
  launched session's own question tool and reported back, and MUST NOT present
  the launcher as the launched session's only channel.
- MUST NOT start the launched session in a permission mode that denies its
  question tool, such as Claude Code's `dontAsk`, which denies `AskUserQuestion`
  even when an allow rule matches it.
- MUST pass on maintainer answers that affect the launched session's work,
  quoting the answer and saying where and when the maintainer gave it, and MUST
  NOT present one as settling an answer the maintainer must give in the launched
  session itself: a plan approval with its delivery confirmation, or a tool
  request the host conditions on the user's own request.
- MUST NOT put to the maintainer a decision that a launched session reports as
  asked and still pending, since the maintainer answers it in that session.
- This rule does not cover subagent assignments or the Amp review child
  thread's assignment, which Development Workflow defines.

### Return a decision from a subagent

Claude Code's [subagent documentation](https://code.claude.com/docs/en/sub-agents)
lists `AskUserQuestion` among the tools it removes from every subagent "even when
listed in the `tools` field" (Available tools). That is documented, not
exercised in this repository. The configured implementer, reviewer, and
investigator therefore MUST return an unresolved maintainer decision and their
partial results to their caller, which asks it, as the practice's Returning a
Question You Cannot Ask section states.

The same page says a fork receives its parent conversation's exact tool pool, so
a fork is inferred to keep `AskUserQuestion`, which is also not exercised here. A
fork MUST follow the first guideline of Asking the Maintainer, and MUST return
any question it asked, and the answer, to its caller with its result.

## Confirming Delivery at the Plan Gate in Claude Code

Claude Code's host conditions pull request creation on an explicit request from
the user. The checked-in request in [`AGENTS.md`](../../AGENTS.md) does not
establish that on its own, so a Claude Code run collects that request where the
human is already answering: in the plan-approval question.

Every plan-approval question, including the one for a revised plan, MUST also
ask the human to confirm, in the same answer:

- delivery of the run's draft pull request;
- requesting the Claude CI review of it each round, which is the review item;
- watching that pull request's activity, where the host offers it, until the
  ready transition, the round cap, the waiting bound, or a reported stop.

Only pull request creation carries a host condition. The maintainer chose to
settle the review item and the watch in the same answer, so that one answer
covers the whole delivery path. This bounded exception to dependent question
ordering is recorded in the delivery, review, and watching deviation in the
register of [Agent Skills](./agent-skills.md). The run MUST offer coherent
combined outcomes in that single question, with review and watching inapplicable
when delivery is declined.

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
- MUST, when the human confirms delivery but declines the review item,
  open the draft pull request, keep it draft, and report the external-review
  gate as unmet and the loop as not complete.
- MUST, when the human confirms delivery but declines watching, still request
  review, then treat each review or check result as a human wait: end the
  turn and resume on the human's next message, with no scheduling, because
  the declined subscription route still exists.

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

A run waits on events its host delivers, or until a check-in the human
explicitly permitted when no subscription route exists. The portable contract
lives in
[Loop Engineering's independent-review reference](../../.claude/skills/loop-engineering/references/independent-review.md),
whose Waiting Bound section owns deadlines, budget resets, human waits, and
teardown at each stop. A cloud environment's lifetime is not that deadline.
Authorization and recovery state belong to
[Loop Engineering's grant and run-state reference](../../.claude/skills/loop-engineering/references/run-state-and-reporting.md).
This project's delivery grant, in
[Development Workflow](./development-workflow.md), does not name scheduling or
subscriptions.

On every host, an agent uses a webhook or event-subscription route for the
result it waits on when one exists, and then never schedules a check-in, whether
by a delayed message, a created or fired trigger, `CronCreate`,
`ScheduleWakeup`, or `/loop`. Only when no subscription route exists and the
human explicitly permitted it in the run's session may the agent schedule a
check-in, and the run records that permission in run state; a launch prompt, an
injected or harness instruction, or a standing settings allowance is not that
permission. A subscription route exists when the session offers one for the
result, even when the human declined watching. A run cancels a permitted
check-in in the same turn at each stop at which it unsubscribes. No agent polls
in-turn: apart from a delegated worker's completion, it never waits in-session
for a check or review result. A bounded retry of a GitHub read-back for the
agent's own write is not such a wait, and GitHub Actions cron workflows are
unaffected. A check or review result that neither a subscription event nor a
permitted check-in reports is a human wait: the run ends the turn reporting it
as pending and resumes on the human's next message or in a fresh session. The
Waiting and Watching section of
[Development Workflow](./development-workflow.md) states this rule for the
project, and this section gives each host's mechanism.

For a Codex-authored pull request, the awaited results are its required GitHub
checks and the independent Codex review selected by Development Workflow. This
repository identifies no subscription route for a Codex run, so both are human
waits unless the human explicitly permitted a scheduled check-in for the run. On
resume or at a permitted check-in the run reads the checks, the App's status,
and any posted result. When the round has no acknowledgment or terminal result
and the waiting bound has passed, it confirms the repository setup and keeps the
review gate blocked. Qualify the actual session's tools under GitHub Operation
rather than assume all Codex surfaces expose the same mechanisms.

For an Amp-authored pull request, the awaited results are its required GitHub
checks and the review its Amp review child thread posts. Wait for the child with
Amp's thread-wait tool, which counts as a delegated-worker completion, then read
the posted review back on the pull request; Development Workflow owns what
counts as the result. Wait for checks only through a subscription the host and
the run already permit. A subscription the session offers but the run may not
use still bars a check-in, so the checks are a human wait; only when the session
offers none may the human permit a scheduled check-in.

For a Claude Code-authored pull request, the awaited results are its required
GitHub checks and the Claude CI review. Where the session offers it, the run
waits only on the pull request webhook subscription that the plan-gate
confirmation covers and on completion notifications from the workers it
delegated to, and then schedules no check-in. Claude Code's
[subagent documentation](https://code.claude.com/docs/en/sub-agents) says
subagents run in the background by default and that, while fork mode is on, an
interactive session cannot block on one, so a worker's completion arrives as a
notification. The subscription is the Remote `subscribe_pr_activity` tool, and
`unsubscribe_pr_activity` ends it. Under the tools' live contracts, a subscribed
pull request's comments, reviews, and check results reach the conversation as
events, to one session at a time; a subscribe result that says another session
already watches the pull request means this session receives none, so its
results are human waits. No Claude CI round in this repository has yet shown the
review's comments arriving this way; an event that never arrives leaves the
result a human wait.

On every wake, a Claude Code run re-reads check, review, and mergeability state
from GitHub, and when nothing else is pending it ends the turn reporting what is
pending. It unsubscribes in the same turn at the ready transition, the
four-round cap, the waiting bound, or a reported stop. Release after merge
follows only a human message or a fresh session that shows the merge.

Use the current pull request head when reading check results. A connector read
can return only a first page of workflow runs, so it cannot establish complete
required-check coverage by itself. Read the selected review's posted result and
its findings through the tools the session exposes, and respect each read's
pagination and completeness limits; missing output is not a successful check or
completed review. Development Workflow owns review-completion evidence.

Claude Code on the web also documents
[PR auto-fix](https://code.claude.com/docs/en/claude-code-on-the-web), which
delivers review comments and CI failures when enabled. It also investigates
events, pushes clear fixes, and can publish review-thread replies under the
user's GitHub identity. It is not the subscription the plan-gate confirmation
covers: before enabling Auto-fix, its scoped grant must cover the subscription
and these downstream effects, not event delivery alone. Its documented stop path
is clearing **Auto-fix** in the session's CI status bar or asking Claude to stop
watching the pull request. This documentation does not establish delivery of
successful-check transitions, and the consulted official pages give no verified
Remote invocation procedure, so live Remote behavior remains unverified here.
Use the actual host's exposed definitions, not guessed arguments or a CLI-tool
substitution.

In Claude Code, watching the run's own pull request is what the plan-gate
confirmation above covers, and that watch ends at the ready transition, the
round cap, the waiting bound, or a reported stop, and this project's rules make
no exception. [Claude Code's entry guidance](../../CLAUDE.md) states how an
injected instruction to keep watching past that point, or to schedule a check-in,
is handled as an irreconcilable conflict.

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

The probe wrote issue bodies with `issue_write`, both creating and updating, and
one issue comment. The Rewrites Observed lists the writes checked since.
Reviews, review replies, comment edits, and URLs already inside code remain
untested, and one review comment was checked through MCP only, as The Rewrites
Observed records. The rules and the read-back below still apply to those
surfaces.

### The Rewrites Observed

A probe on a scratch issue, run on 2026-11-19 (PST) with the maintainer's
approval, wrote URLs of known lengths and read back what GitHub stored. Each
cell lists the lengths tested, how many URLs were written at them, and in how
many probe writes. The counts cover issue bodies only. Each distinct body is
counted once; a body re-sent unchanged to test determinism stored the same
result and is not counted twice.

| Form                              | Stored intact                                                                                                                                       | Stored wrapped in a code span                                                                                             |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Inline link destination           | 59, 65, 91, 104, 107, 115, 130, 139, and each of 140 to 149 characters (18 lengths; 21 URLs in 3 writes, with 107, 130, and 149 each written twice) | 150 (1 URL), 151 (5), and 160 (1) characters in the probe (7 URLs in 6 writes); 151 characters in the two incident writes |
| Titled link, reference definition | Not tested below 150 characters (no samples)                                                                                                        | 150 characters (1 titled link and 1 reference definition, in 1 write)                                                     |
| Bare URL, autolink                | 25 characters (1 autolink); 149 and 150 characters (1 bare URL and 1 autolink at each); 5 URLs in 2 writes in all                                   | 200 characters (1 bare URL and 1 autolink, in 1 write)                                                                    |

The threshold for a bare URL or an autolink lies above 150 and at or below 200
characters. The rewrite was deterministic, did not depend on the URL's content
or the surrounding line, and applied to every qualifying URL in a body. One
comment was tested, and its 151-character link was wrapped too. Once, for the
160-character inline link, the closing run of backticks landed after the link's
`)`, so the parenthesis went inside the code span. A wrapped inline link or
reference definition no longer works as a link, and a wrapped autolink also
shows `&lt;` and `&gt;` to readers.

Three more rewrites were observed:

- **The comment footer.** The host instructs agents to end every comment,
  review, and reply with `\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_`.
  One comment sent without it was stored with it appended (comment 7032656088,
  2026-11-19 PST). Four comments that already ended with it were stored with no
  second copy of it (comments 7032949620, 7033812409, 7033840359, and
  7044614711, 2026-11-19 and 2026-11-20 PST); comment 7033812409's long link
  was still wrapped. Whitespace after the footer was not tested.
- **The pull request attribution.** The host instructs agents to end each pull
  request description with a "Generated with Claude Code" line and the session's
  link. The body of #150, created with `create_pull_request` on 2026-11-20
  (PST), was stored with that attribution replaced by the comment footer, and
  every other byte matched what was sent
  ([comment 7046296956](https://github.com/axross/runners/issues/144#issuecomment-7046296956)).
  That is one write, by create. The first two writes by `update_pull_request`,
  to #150's description on 2026-11-20 (PST), were read back through REST search
  at their update times and were byte-identical to the sent files, attribution
  included, with no footer
  ([comment 7047943100](https://github.com/axross/runners/issues/144#issuecomment-7047943100)
  and
  [comment 7048354239](https://github.com/axross/runners/issues/144#issuecomment-7048354239)).
  Another session reported that `update_pull_request` kept the attribution on
  #138 (three writes) and #139, compared byte for byte with what MCP's get
  returned, with no REST read, so the replacement may be specific to creating a
  pull request. A body sent without the attribution, and whitespace after the
  attribution, were not tested.
- **U+200B.** It was removed when an issue was created, the only write path
  tested. MCP reads also drop it; REST reads keep it. U+200C, U+200D, U+2060,
  and U+FEFF were not tested.

No other unintended change was found for the constructs tested. These were
stored byte-identical, and MCP reads returned them unchanged: the character
references `&amp;`, `&#x27;`, `&lt;`, `&gt;`, `&quot;`, and `&#39;`; raw
`& < > ' "`; angle-bracket placeholders; HTML comments; `<details>`; and fence
info strings. The three sanitized-read stages that GitHub Operation's Body
Integrity describes were not reproduced.

Those constructs were tested in issue bodies, so the list does not carry over to
review comments. On 2026-11-20 (PST), an independent review child for #150
posted an inline review comment through the MCP pending-review tool
([review comment 4646748901](https://github.com/axross/runners/pull/150#discussion_r4646748901)).
It sent a single-backtick code span containing the literal placeholder
`<149-character URL>`, not a real URL. MCP's review-comments read returned the
placeholder's angle brackets as `&lt;` and `&gt;`, set in a double-backtick code
span of its own, which splits the original span. The comment was checked only
through MCP, so its exact stored content is unverified, and whether the write
path or the read path changed it is not established.

An open report, `github/github-mcp-server#3299`, was still open on 2026-11-19
(PST), as read through a model-summarized fetch of its public page. It reports
long URLs wrapped in code spans, including link destinations, bare URLs, and
autolinks. It also reports an autolink's angle brackets escaped and the closing
backticks sometimes landing after the link's `)`, a shape seen once here too.
Its threshold of 104 characters and its wrapper shape otherwise differ from what
was measured here, so it may be the same defect, but that is not established.

### Rules Before Writing

The rules apply to every issue body, comment, review, reply, and pull request
body an agent writes from the session. The 149-character cap rests on the
threshold measured for this endpoint, not on the 104 characters the open report
gives. If that lower threshold ever applies here, the read-back catches it.

The probe measured ASCII URLs only, so the cap counts ASCII characters. The URL
check counts UTF-16 code units, which equal characters for ASCII; a
percent-encoded URL is ASCII and is counted as written. For a URL with other
non-ASCII characters no threshold was measured, so keep it well under the cap;
the mandatory read-back catches a rewrite.

**Guidelines:**

- MUST compose the text in a local file that holds exactly what will be sent,
  and keep the file and the sent text in agreement. An issue body may end with a
  newline or not; every stored probe body kept its trailing newline. Keep the
  composed file out of the commit: run every command in this section from the
  session's scratch directory, or from a directory made with `mktemp -d` when the
  session has none, so that `body.md`, `footer.txt`, `stored.json`, and
  `expected.md` never land in the repository.
- MUST keep every URL at 149 characters or fewer, in any form.
- MUST NOT write U+200B, U+200C, U+200D, U+2060, or U+FEFF.
- MUST end each comment, review, and reply with the exact host footer and
  nothing after it. A pull request body ends as the host prescribes, with its
  attribution and no newline after it, not with the footer; the
  attribution-aware comparison below recognises only that ending.

### Checks Before Writing

Run these on the composed file, named `body.md` below. Each prints an explicit
result when it finds nothing, and none reports a pass for a file it cannot read.
The checks need `node`, GNU `grep`, `tail`, `wc`, and `cmp`. The read-back
commands below need `gh`, `jq`, `cmp`, and `diff`, plus `node` for the
attribution-aware comparison, run from `bash`, whose process substitution the
diff command uses. Run outside the checkout, `node` and `jq` are the ones on the
session's `PATH`; mise applies the versions pinned in
[`mise.toml`](../../mise.toml) only inside the checkout.

The URL check lists every `http` or `https` URL of 150 or more characters with
its line number and length. It does not count a closing `)` that belongs to the
link syntax when that `)` ends the match. When punctuation follows the `)`, as
in `[text](URL),`, it counts both the `)` and the punctuation, which errs on the
safe side. It does not check relative or non-`http` destinations, which were not
tested.

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

The footer check applies to a comment, review, or reply. It accepts text that
ends exactly with the footer and rejects text with no footer or with a newline
after it.

```sh
printf '\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_' > footer.txt
tail -c "$(wc -c < footer.txt)" body.md | cmp -s - footer.txt \
  && echo "ok: ends with the footer" || echo "footer missing or not at the end"
```

### Reading Back a Body

The read-back starts from the update time that an MCP get of the item shows
after the write. The stored body is then read as structured data through a route
qualified under
[GitHub Operation](../../.claude/skills/github-operation/references/channel-selection.md):

- **The direct REST read** of the issue or pull request, where the session may
  use it.
- **The REST search read**, otherwise: `is:issue` or `is:pr`, sorted by update
  time, newest first, 100 per page, with the item selected by number. No title
  goes in the query, so quotes or apostrophes in a title cannot break it.

MCP reads are a quick check only: they drop U+200B, and their result reaches the
agent's context but not a command, so they cannot supply a byte comparison.

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

When the write path replaces a pull request body's attribution with the comment
footer, as it did when #150 was created, the comparison above reports a
difference. This command builds the expected stored text from the composed
file, with the attribution replaced by the footer, and compares the stored body
with that, reusing the `stored.json` of the search read. With the direct read,
use `jq -j .body stored.json` in place of the `jq` filter that selects the item
from the search result. It accepts the attribution only in its exact form at the
very end of the file, and otherwise says so without comparing.

```sh
node -e '
const fs = require("fs");
const sent = fs.readFileSync(process.argv[1], "utf8");
const tail = /\n\n🤖 Generated with \[Claude Code\]\(https:\/\/claude\.com\/claude-code\)\n\nhttps:\/\/claude\.ai\/code\/session_\w+$/;
if (!tail.test(sent)) { console.log("no attribution in its exact form at the very end"); process.exit(1); }
fs.writeFileSync("expected.md", sent.replace(tail, "\n\n---\n_Generated by [Claude Code](https://claude.ai/code)_"));
' body.md && jq -j '.items[] | select(.number == <number>) | .body' stored.json \
  | cmp - expected.md && echo "ok: identical except the attribution replacement"
```

**Guidelines:**

- MUST read back each issue or pull request body after writing it and compare
  it with the sent file by command, not by eye, as above.
- MUST treat an item missing from search, or an update time older than the one
  noted, as a read that is not current yet, not as a mismatch. Retry up to three
  times over about five minutes, using only a wait the session itself provides,
  never a scheduler. When the session has no such wait, or the read is still not
  current, report the write as unverified.
- MUST run the plain comparison first, and the attribution-aware one only when
  the plain one reports a difference for a pull request body that ends with the
  host's attribution. MUST record a match from the attribution-aware one as
  verified, naming the replacement. Any other result is a difference to handle
  under Recovering from a Difference.

### Reading Back a Comment

An issue or pull request conversation comment can be checked in two ways; the
guidelines below say which applies. The first compares the comment by
command through the direct REST read of it, as a body is compared. Here
`body.md` is the composed comment, footer included.

```sh
gh api repos/axross/runners/issues/comments/<comment id> > stored.json
jq -r .updated_at stored.json
jq -j .body stored.json | cmp - body.md && echo "ok: stored comment is identical"
```

The second checks the comment as MCP's comments read returns it: each URL is
unwrapped, and the footer appears exactly once, at the end. This check does not
establish the stored content, for the reason given under Reading Back a Body.

**Guidelines:**

- MUST check each issue or pull request conversation comment after writing it,
  by the command above where the session may use the direct REST read of a
  comment, under
  [GitHub Operation](../../.claude/skills/github-operation/references/channel-selection.md)'s
  channel qualification, and otherwise as MCP returns it.
- MUST report a comment checked only through MCP as checked through MCP only,
  with its exact stored content unverified, as GitHub Operation requires.
- MUST check each review and review reply as MCP returns it, and report it as
  unverified, unless the session compares it by command through an exact read.

### Recovering from a Difference

To confirm that the stored text differs from the intended text only by the known
patterns, show the differences line by line. `<` lines come from `body.md` and
`>` lines from the stored text. When the search read was used, select the body
as the search read-back does, with
`jq -j '.items[] | select(.number == <number>) | .body' stored.json`.

```sh
diff body.md <(jq -j .body stored.json)
```

Each known pattern looks like this in the output:

- **A wrapped URL** appears inside a run of backticks with a space on each
  side; the closing run lands before the link's `)` or, as seen once, after it,
  and a wrapped autolink also shows its angle brackets as `&lt;` and `&gt;`.
- **An appended footer** appears as added lines at the end.
- **A replaced attribution** appears as the attribution lines removed at the end
  and the footer lines added in their place.
- **A removed invisible character** makes a line differ with no visible change.
  It can appear only if the file held one despite the check before writing.

For a comment read only through MCP there is no `stored.json`. Compare the text
the read returns with `body.md` by eye, against the same patterns, and record
that the comparison was not byte-exact.

**Guidelines:**

- MUST NOT re-send identical text; the rewrite is deterministic, so identical
  text written again is stored the same way.
- MUST confirm, before any corrective write, that the stored text differs from
  the intended text only by the known patterns listed above. Any other
  difference, including a change someone else made in the meantime, stops the
  operation for reconciliation under GitHub Operation's
  [Body Integrity](../../.claude/skills/github-operation/references/body-integrity.md)
  and is reported as an unverified write, not worked around.
- MUST NOT make a corrective write for an appended footer or a replaced
  attribution alone; neither needs one.
- MUST, for a body on a target the delivery grant in
  [Development Workflow](./development-workflow.md) covers, compose the
  corrected text from the intended text with the URL shortened, write it once,
  and read it back again.
- MUST, for a status comment on the run's tracking issue, append a corrected
  status comment that supersedes the original instead of editing it, because
  Development Workflow keeps run state in append-only status comments.
- MUST, when a corrective body or status comment also differs, report the write
  as unverified and stop that operation.
- MUST, for any other comment, report it as unverified and ask the maintainer,
  because the delivery grant names no corrective comment for it.

### The Plan Body

A plan's identity is derived from its stored bytes, so a rewrite of the plan
body changes what the maintainer approves. An identity mismatch after approval
is handled under Plan Revision Identity in
[Loop Engineering's plan document reference](../../.claude/skills/loop-engineering/references/plan-document.md).

**Guidelines:**

- MUST re-derive the plan's identity from the stored bytes after publishing or
  revising it, through the body read-back above.
- MUST fix a difference found before approval with a corrective write, which
  produces a new revision for plan review.
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

`reviewer.md` is the reader for the advisory pre-flight review. It denies
exactly two things, editing and spawning, which are the Edit, Write, and
NotebookEdit tools and the Agent tool, and nothing else. Widening that deny-list
is the tempting mistake and MUST be resisted: judging a change means confirming
what was asked and not only what was written, which reaches the issue, the
plan's artifacts, and the documentation behind a factual claim. A reviewer that
cannot reach one of those does not fail to start; it returns a report short by
exactly those checks, and an under-equipped review reads exactly like a clean
one. The denial covers editing tools and nested spawning, not every possible
shell write. Without an assignment that names the instance as a round's
independent review and carries that round's token, it returns its report
privately and posts nothing; with one, its prose permits one COMMENT-type
review. No standing route issues that assignment, which only a review session
the maintainer instructs could, so an instance used for the pre-flight is
advisory and is never the independent review.

The limits on a pre-flight `reviewer` instance are of two kinds. Its definition
declares, and Claude Code documents enforcing, one set: the Edit, Write,
NotebookEdit, and Agent denial. No pre-flight round here has confirmed that
enforcement yet. Every other limit is instruction-only:

- no shell write to the repository and no commit;
- no push and no merge;
- no approve or request-changes verdict;
- no GitHub write;
- no starting or messaging of other actors through a route other than the Agent
  tool, such as a session-starting or messaging tool;
- no posting at all, since no standing route issues the independent-review
  assignment that enables the definition's posting permission, and the host does
  not enforce that the condition holds.

The definition leaves GitHub write tools and session-starting and messaging tools
undenied, because a deny-list naming some connectors' tools would not cover the
others and the reader still needs the shell and the tracker to read the change.
The accepted exposures follow from that:

- An instance holds the session's GitHub write access and every tool other than
  the four its definition denies, so the instruction-only limits bind only as far
  as the assignment and the definition instruct.
- An instance raises permission prompts in the authoring session, where an answer
  that lasts for the session also applies to the authoring conversation, and
  where a "don't ask again" answer for a Bash command, a web fetch domain, or web
  search is saved to the repository's `.claude/settings.local.json` and applies to
  later sessions and their subagents anywhere in this repository, as Claude
  Code's [permissions documentation](https://code.claude.com/docs/en/permissions)
  states.

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
external review remain unchanged. Deleting a file never lets the loop skip the
independent-review gate. Without the implementer, the loop delegates to a
generic agent or runs single-agent. Without the reviewer, the pre-flight stage
is skipped rather than performed by the main actor, which is what keeps it from
collapsing into self-review, as
[Record an unavailable role](#record-an-unavailable-role), below, states.
Without the investigator, the main actor reads the payload itself.

The advisory pre-flight review applies after every verified initial
implementation, before the first branch push and draft pull request, whether the
parent or a child implemented it. A compatible fresh reader must be permitted
and available; its findings and round limits follow
[the pre-flight contract](../../.claude/skills/loop-engineering/references/pre-flight-review.md).
If no reader qualifies, record the exact unavailable or prohibited reason and the
resulting delivery restriction. That outcome is not a clean review; the Deliver a
draft when advisory pre-flight is unavailable section of
[Development Workflow](./development-workflow.md) states what it permits,
including that on Claude Code a missing or refused `reviewer` leaves only the
pre-flight unavailable.

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
than replaced with self-approval. On Claude Code, a `reviewer` that is missing,
or whose spawn the host refuses, leaves only the advisory pre-flight
unavailable, under the draft-delivery exception in Development Workflow; the
independent review is the Claude CI review, which does not depend on it. A CLI
agent or persistent thread MUST NOT be used to bypass a missing role or a
prohibited purpose. For unavailable research, plan, or design review, the driver
MUST use
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
