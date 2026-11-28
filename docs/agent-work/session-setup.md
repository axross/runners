---
type: Procedure
title: Start and provision an agent session
description: Start Codex in the intended checkout and provision the toolchain used by Amp and Claude Code.
---

# Start and provision an agent session

Claude Code and Amp share the pinned toolchain in [`mise.toml`](../../mise.toml). Each host provisions it through its own entry point. Codex uses the project entry in the intended checkout.

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
  [Retrieve state on resume](./run-state.md#retrieve-state-on-resume)
  from the named issue. Compare its canonical revision, recorded human approval
  and pending phase with the current checkout and existing edits. Preserve
  those edits, revise the plan for the expansion and obtain the maintainer's
  approval before implementing or delegating the new work.
- **Continue unchanged approved scope.** Resume at the recorded pending phase.
  For example, when verification and pre-push review allow a push, an append-only
  push to the run's working branch matches the adopted grant. Hold a request
  to assign an additional person to the pull request until separately
  authorized. Check the actual host and tool conditions for each effect.
  [Codex delivery authorization](./host-permissions.md#codex-delivery-authorization) routes through
  [Amp delivery authorization](./host-permissions.md#amp-delivery-authorization) to the
  [adopted grant](./delivery-grant.md#authorizing-delivery-operations).
- **Ask a standalone read-only question.** For “Where is Amp provisioned?” the
  project entry points directly to [Amp Environment Provisioning](#amp-environment-provisioning)
  and [`.agents/setup`](../../.agents/setup). After loading the applicable
  prerequisites, answer the question directly.

The shell walkthrough captured the working directory and initial task passed to
a stand-in for Codex, then traced the project links. Startup by a live host and
an actor receiving and following the guidance were unobserved.

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
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml),
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) and the staged
[`release-feed.yaml`](../../.github/staged/release-feed.yaml), and the
`version` and `expected_sha256` in `.agents/setup`. Both hold the SHA-256 of the
bare `mise-v<version>-linux-x64` binary, taken from the release's published
`SHASUMS256.txt`; the action hashes the binary it extracts from its archive, so
it does not use the archive's checksum.

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
[the dependency pinning convention](../security/dependency-pins.md#pinning-convention).

A Rust task run without the toolchain fails at once with one line that names this
script, never with a long tool error and never by skipping, so a session cannot
report a change as checked when the Rust gates did not run.
