---
type: Convention
title: "Testing permission refusals"
description: "Decide what a permission-dependent test must assert on a privileged local host and in CI."
---

# Testing permission refusals

## Tests That Rely on a Refused Write

A test that needs the kernel to refuse access because of permission bits, such
as a write into a read-only folder, MUST first try that access. The kernel does
not refuse it for every process: root with `CAP_DAC_OVERRIDE` writes into a
read-only folder, and Claude Code cloud sessions run as root with that
capability. A test that assumes the refusal fails there, and `mise run check`,
which the opt-in `Stop` hook runs, fails with it.

When the access succeeds, the test MUST do one of two things:

- It keeps asserting what still holds and leaves the strict check to a CI step
  that runs as a non-root user, as the lock-file test in
  `cli/tests/instance_lock.rs` does with its step that runs the helper as
  `nobody`.
- It skips only the dependent step and prints a note that says why, as the
  read-only-folder step of the agent configuration test in
  `cli/tests/agent_config.rs` does. Every other step still runs.

On GitHub Actions, where `GITHUB_ACTIONS` is `true`, the test MUST fail instead
of skipping. CI runs as a non-root user, so a skip there would mean the kernel's
refusal is no longer exercised and CI would pass without it. The note of a
passing test is captured, so it shows only with `--nocapture`.

The test MUST try the access instead of reading the user ID, because the user ID
does not say what the process may do: a root process that has dropped
`CAP_DAC_OVERRIDE` is refused, and a non-root process can hold it. An attempt
that would change the file system if it succeeded, such as creating, replacing,
or removing a file, MUST run in a folder the test creates for it, never in one
the code under test reads, and MUST remove that folder afterwards: under root
the attempt succeeds, and its change would appear where the code under test is
reading.

The `Stop` hook does not drop `CAP_DAC_OVERRIDE` for its check instead, for
example by running it through `setpriv`: that would fix only the hook, and a
direct `cargo test` or `mise run check` as root would still fail.
