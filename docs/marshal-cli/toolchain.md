---
type: Procedure
title: "Checking the Rust toolchain and workspaces"
description: "Which toolchain and CI gates exercise the library and app workspaces?"
---

# Checking the Rust toolchain and workspaces

## Toolchain and Gates

The toolchain is one exact stable release in
[`cli/rust-toolchain.toml`](../../cli/rust-toolchain.toml), the minimal profile with
`rustfmt` and `clippy`, and [`cli/Cargo.lock`](../../cli/Cargo.lock) is committed
and used with `--locked`. [Agent Sessions](../agent-work/session-setup.md) describes
provisioning it. The crate forbids `unsafe` and denies `unwrap`, `expect`, `panic`,
indexing, and the debug macros outside tests, because input paths must collect
errors, not abort.

| Task                     | Runs                                                 |
| ------------------------ | ---------------------------------------------------- |
| `format`, `format:check` | `cargo fmt`, and `cargo fmt --check`                 |
| `lint:rust`              | `cargo clippy --locked --all-targets -- -D warnings` |
| `test:rust`              | `cargo test --locked`                                |
| `deny:rust`              | `cargo deny --locked check bans licenses sources`    |

Each Rust task first checks for `cargo`, `rustfmt`, and `clippy` and fails at once
with one line naming `.agents/install-rust`. A task never skips when the toolchain is
missing, because a gate that silently does not run would let a session report a
change as checked when it was not.

The `rust` job in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs format check,
clippy, and tests on Linux, Windows, and macOS; the axpc checkout-rule cases that
need drive-letter paths run only on Windows. Four more steps run on Linux only: the
base-address, instance-roots, build-definition, release-overrides, and
agent-overrides tests again with `--release` and `MARSHAL_EXPECT_RELEASE=1`, which
makes each fail if built in the wrong profile;
`docker build --target base` of
[`images/actions-runner`](../../images/actions-runner), tagged and never pushed;
the `real_docker` test with `--ignored`, pointed at that tag by
`MARSHAL_REAL_DOCKER_IMAGE`, which also exports a container's evidence through the
exporter child from a real `docker cp` stream; and the `real_docker_build` test with
`--ignored`,
which removes the image it builds. The Windows and macOS hosted runners have no
Linux Docker daemon, and each test fails rather than skips when one is missing.
One more step runs on Linux and macOS, `tests/ci/other-user-lock.sh`, and one on
Windows, `tests/ci/control-pipe-security.ps1` under PowerShell 7; [What the Instance Tests Cover](./foreground-instance.md#what-the-instance-tests-cover) describes the two checks. The `fake-docker`, `fake-tart`, `fake-softnet`,
`fake-macos`, `instance-helper` and `commands-helper` helpers are the package's only
binaries (`cli/helpers/`, `test = false`), which only the tests and those steps run;
the package builds no `marshal` binary. `instance-helper` does nothing in a release
build, and `commands-helper` runs in both profiles, for the release-profile tests. A release MUST NOT ship any
helper; [Releases](./release-artifacts.md#releases) says how the release build keeps them out. Tests find fixtures relative to the
crate's manifest directory, and the checkout-rule tests build a temporary directory
with the marker files at test time. The advisory check, `cargo deny --locked check advisories`,
fetches the RustSec database, so it runs in
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) and not in
`mise run check`; see [Development Workflow](../agent-work/change-delivery.md).

The `rust` job's `Run Tests` step passes `--no-fail-fast`, so a test executable that
fails does not stop the executables after it in that step, and one flaky executable
cannot hide the rest of that step's results; the job still fails when any test fails.
The `test:rust` task, and so the Check job's run of it, stays `cargo test --locked`
and stops at the first failing executable.

## The App Workspace

[`app/`](../../app) is a second Cargo workspace beside `cli/`: the Windows
notification-area app, which is also the executable that runs the resident agent.
[App](../marshal/index.md) owns what it does. This section owns what the library gives it and
the gates that cover it, so a change to the library knows what the app relies on.

**Contract.**

- The workspace depends on the `marshal-cli` library by path, through the crate's
  public items only, so the app's lockfile, `cargo deny` run, licence notices and
  advisory check cover everything its executable links, the library's crates
  included. No `marshal` binary is built: the app's executable runs the library's
  hidden modes itself.
- No file under `cli/src` moves for the app. The library's additions for it are
  small exports: the stop reason codes as `agent::StopReason`, the handshake
  timeout `agent::HANDSHAKE_TIMEOUT`, the registration error's marker of a token
  GitHub rejects as `runtime::registration::REJECTED_MARK`, the start of a pool's reason
  when the token store failed as `agent::tokens::STORE_FAILED`, the failure that stops an
  instance as `instance::restart::STOPPING_FAILURE`, beside the already public
  `WINDOW` and `DELAYS`, whose figures the app's banners quote, and the names of the
  debug-only test variables as `command::TEST_ROOT_VARIABLE`, `TEST_DOCKER_VARIABLE`
  and `TEST_GITHUB_API_VARIABLE`. A later change to a public item the app uses
  breaks the app's build, which `mise run check` and the Windows job show, so the
  change and the app move together.
- The app reads the library for the protocol's types and client, the agent's entry
  and options, the token store's interface and refusals, the instance roots, the
  hidden `export`, `shim` and `vm-shim` commands, the `Secret` type that masks a token,
  the executable's identity (`agent::executable::Executable` and `Look`), the private
  log folder and file mode (`private::current_uid`, `ensure_private_directory` and
  `FILE_MODE`), and the log's timestamp (`runtime::name::stamp`). It adds no message to
  the protocol: a new message follows [Version Rules](./agent-protocol.md#version-rules).
- The client exposes `config_read`, `config_edit` and `config_reload`, with typed
  `ConfigRead`, `ConfigChange` and `PoolEffect` answers. Their status and effect
  names remain strings, so an unknown future name does not break decoding.
  The session preserves structured configuration problems and passes each request
  through the same connection as its other actions. Method negotiation controls
  whether the window offers editing.
- `cli/helpers/fake_docker.rs` is also built as the `fake-docker` binary of the app's
  executable package, so the app's tests can run the exporter child against a fake
  engine. The source stays under `cli/`; the CLI's own tests build it as before.
- The app has its own `Cargo.lock`, [`deny.toml`](../../app/deny.toml) and
  [`rust-toolchain.toml`](../../app/rust-toolchain.toml). The toolchain file names
  the CLI's channel, and Dependabot is set to bump both files in one pull request.
  The app's lockfile resolves every crate `marshal-cli` builds with in the CLI, for
  Windows, macOS and Linux, to the version in the CLI's, so the CLI's tests cover the library
  at the versions the app links; a change to one of those versions changes both
  lockfiles. The CLI's
  `Cargo.lock`, `deny.toml`, and dependency table do not change for the app: the
  table under Dependencies lists the CLI's crates only, and
  [Security](../security/dependency-pins.md#the-apps-dependencies) holds the policy for the
  app's own.
- Every app crate forbids `unsafe_code` and takes the CLI's lint set through
  `[lints] workspace = true`.

**Gates.** `mise run check` covers what builds without the web view's system
libraries, so the Check job on Linux needs none; the Windows `app` job and the macOS
`app-macos` job cover the rest.

| Task                             | Runs                                                                                                                                                                                                 |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `fetch:app`                      | `cargo fetch --locked` in `app/`, which `cargo deny` and `cargo about` read                                                                                                                          |
| `format:app`, `format:check:app` | `cargo fmt --all`, and `cargo fmt --all --check`, in `app/`                                                                                                                                          |
| `lint:app`                       | `cargo clippy --locked --all-targets -- -D warnings` over the `marshal-modes`, `marshal-view`, `marshal-session`, `marshal-tray` and `marshal-feed` packages                                         |
| `test:app`                       | `cargo test --locked` over the same five packages                                                                                                                                                    |
| `test:app-frontend`              | `node --test` over the main window page's own tests, in `app/crates/app/tests/frontend/`                                                                                                             |
| `deny:app`                       | `cargo deny --locked check bans licenses sources` in `app/`, for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`                                                                                 |
| `check:notices`, `notices:app`   | Compare, or write again, the checked-in licence notices against `app/Cargo.lock`; see [App](../marshal/notices.md#licence-notices)                                                                   |
| `check:app-pins`                 | Both toolchain files name one channel, and `app/tauri-cli.version` holds one exact version                                                                                                           |
| `test:tauri-cli-version`         | The script that reads the pin, against files that are not one exact version                                                                                                                          |
| `check:app-lock`                 | `app/Cargo.lock` resolves the crates `cli/Cargo.lock` resolves for `marshal-cli`, dev-dependencies aside, for the three targets to the same versions; crates the app's features add are not compared |

`format:check` and `lint` call the app's formatting and lint tasks, and `check` calls
the rest, so one run covers both workspaces. The Tauri crate, `marshal-app`, needs the
web view and is not in `lint:app` or `test:app`.

The `app` job in [`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs
on a Windows hosted runner: clippy and the tests over the whole workspace, then
`cargo tauri build --no-bundle`, failing if the build changed or left a file. The
`app-macos` job runs clippy and the tests over the whole workspace with the
`fake-engine` feature on a macOS hosted runner. An
`app-modes` job runs clippy and the tests of `marshal-modes` on a macOS hosted runner,
which covers the dispatch of both Unix shims, `shim` and `vm-shim`, there, and runs the
crate's tests again in the release profile, where the debug-only variables are
ignored; it never builds the Tauri crate. Linux runs the dispatch tests in the debug
profile in `mise run check`. The advisory check,
`cargo deny --locked check advisories` in `app/`, runs in
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) beside the CLI's
and is not part of `mise run check`; see [Development Workflow](../agent-work/change-delivery.md).
[Development Workflow](../agent-work/change-delivery.md) describes each job's permissions,
and [App](../marshal/building.md#building-the-app) says how the app is built and which pin its
build tool has.
