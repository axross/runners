# Documentation

This project's own documentation, which the README links as its index. Which
body answers which question: **what must a change satisfy?** -> `conventions/`. **How is something
run or operated?** -> `operations/`. **What does a project word mean?** ->
[glossary.md](./glossary.md). This project keeps no decision log: a settled
constraint's rationale, including a rejected alternative that would otherwise
leave no trace, is stated inline in whichever document governs its subject. It
has no `specs/` directory yet; one is added when a behavior needs describing
rather than instructing, and each spec then earns a matching glossary heading.

Documents under `conventions/` and `operations/` use MUST, MUST NOT, SHOULD,
SHOULD NOT, and MAY as [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html)
describes.

The agent-host configuration this repository is also for is planned and is not
documented here until it exists; until then
[Security](./conventions/security.md) and [REVIEW.md](../REVIEW.md) govern it. A
change that adds a runner image, host, or agent-host surface adds the operations
document that owns its procedure, an entry below, and a row in
[Documentation](./conventions/documentation.md)'s invalidation table, which
names every other document the change corrects.

## Conventions

- [conventions/security.md](./conventions/security.md) - how actions and
  downloads are pinned, least-privilege workflow permissions, why this public
  repository runs its own jobs on GitHub-hosted runners only, the cache-poisoning
  threat model for shared runner storage, per-repository isolation on a runner
  host, the release record for the CLI's dispatch-only release workflow and its
  signing Environment, the accepted LAN-egress risk, the axmc runtime's per-job Linux
  and macOS VMs, CPU placement in it, Softnet and its SUID check, the accepted host-facing
  surface, the VM-local Docker socket, the pinned Tart, Softnet, golden VM base, and
  runner image Docker client, how the resident agent keeps tokens and receives them,
  what the TOML configuration holds and what its problems quote, what the Windows
  app's web view, capabilities and process may do, the app's dependency policy, what
  is never committed, and why the review gate admits one bot identity by login.
- [conventions/testing.md](./conventions/testing.md) - the check commands, what
  each verifies, which of them CI runs, and what a test does when the kernel
  does not refuse the access it relies on.
- [conventions/directory-structure.md](./conventions/directory-structure.md) -
  the technologies this repository uses, where each listed runner path lives,
  and the table a change that adds, removes, or repurposes one updates.
- [conventions/documentation.md](./conventions/documentation.md) - which
  document a change to each runner surface invalidates, and what the
  documentation checks cover, deferring to
  [Living Project Documentation](../.claude/skills/living-project-documentation/SKILL.md)
  for the general practice.

## Operations

- [operations/development-workflow.md](./operations/development-workflow.md) -
  the change loop, tracking-issue completion, the delivery grant, branch naming,
  bumping the runner, cutting a release and renewing its signing certificates,
  the post-push review route, and how a run waits and watches.
- [operations/agent-sessions.md](./operations/agent-sessions.md) - how to launch Codex in the
  intended checkout, how Claude Code and Amp sessions start, their hooks,
  Amp's and Codex's delivery
  authorization, asking the maintainer (launched sessions and subagents
  included), the plan-gate delivery confirmation, Amp's plan-gate archive
  confirmation for review child threads, the write-fidelity rules and read-back
  for GitHub text written from a Claude Code cloud session, subagents, and
  telemetry.
- [operations/development-setup.md](./operations/development-setup.md) - the
  pinned tools a contributor installs, the Rust toolchain, and the hooks.
- [operations/windows-runner-host.md](./operations/windows-runner-host.md) -
  running ephemeral runners for several repositories on a Windows machine with
  Docker Desktop with Marshal: prerequisites, installing Marshal, the
  per-repository token, the host configuration,
  building the image, running and stopping the pools, diagnostics and their
  recovery, health checks, rebuilding and updating, recovery, and rolling back to
  the previous release.
- [operations/macos-runner-host.md](./operations/macos-runner-host.md) -
  running Linux and macOS runners in Tart VMs on the Mac, the axmc host, with the library's VM
  runtime, the app-release gap until M2's Mac app, and the bounded CLI recovery route: installing
  Tart and Softnet at the tested versions, setting up Softnet's SUID bit, the
  signed-in session, the golden VM contract, building the golden VM, running the
  host, sleep, wake and network changes, the golden macOS VM and macOS pools,
  removing the VMs of a pool no longer configured, and the verification status.
- [operations/cli.md](./operations/cli.md) - the `marshal-cli` library under `cli/`,
  with no command line and its test-only commands helper: what `validate` accepts for
  the axpc and axmc host profiles, the validation rules and
  the shared fixtures, the TOML configuration format (schema version 1, its
  extension rule, its problems, the rule set shared with JSON, the conversion from
  JSON, and the write path that keeps comments) and its fixtures, the runtime layer
  (the container backend, the runtime seam, the Tart backend, JIT registration,
  CPU placement, the slot loop, and the signal source), the
  instance layer (instance state, single-instance lock, and control channel),
  the resident agent (its state, the token store, protocol version 2 with its
  version rules, and the snapshot),
  the image contract (its labels, the image check, and freshness), building the
  runner image with `build`, running a host with `start`, `stop`, `restart`,
  `status`, and `diagnostics` (the log, containment, the stop sequence, and the
  diagnostic containers and VMs and their export), the axmc host's prerequisites,
  its VM shim, golden VM provisioning, and the golden VM build, macOS VM pools (their
  defaults, the runner and base records, the VM names, the two-VM count, the golden
  macOS VM build, the macOS job VM, and macOS status), the toolchain and Rust
  gates, the app's workspace and what the library gives it, the release build and
  its dry run, the last command-line release, the signing contracts and temporary
  CLI recovery route, installing Marshal from its MSI, the optional check of a download, updating,
  uninstalling, and rolling back, the tests of those blocks, the allowed
  dependencies, and why the CLI's name is fixed.
- [operations/app.md](./operations/app.md) - the Windows notification-area app and
  the agent it starts: the one executable and its modes, the debug-only test
  variables, how the app connects to and restarts its agent, the logs, the
  commands and events between the Rust side and the web view, the page, the icons
  and the glyph files, the bundled font and colour values, the licence notices,
  building the app and the `tauri-cli` pin, the install layout, rolling back to the
  command line, what its tests do and do not show, and the sections other children
  add.
- [operations/agent-skills.md](./operations/agent-skills.md) - installing and
  refreshing the skills from `axross/skills`, the drift checks, and the register
  of deviations and gaps.
- [operations/cli-recovery.md](./operations/cli-recovery.md) - the temporary
  fixed-source CLI release route, its two source identities, actual-version CLI
  install/check/migrate/uninstall/rollback procedures, evidence boundaries and
  app-release restoration prerequisite.

## Glossary

- [glossary.md](./glossary.md) - runner, image, host, CLI, golden VM, build VM,
  job VM, Softnet, agent-host, and change-loop vocabulary.

For what this project is and the tools a contributor prepares, see
[`README.md`](../README.md). [`AGENTS.md`](../AGENTS.md) is the working agreement
for agent sessions.
