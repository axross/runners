---
type: Architecture
title: "Release artifacts and workflow boundaries"
description: "Which artifacts and jobs does the release workflow build, check and publish?"
---

# Release artifacts and workflow boundaries

## Releases

A release is a `v<version>` tag on the default branch and the GitHub release that
carries its files, such as `v0.1.0`. The maintainer starts one by dispatching
[`release.yaml`](../../.github/workflows/release.yaml) on main, and semantic-release
works out the version from the commits since the last `v*` tag. Releases are never
prereleases. The run has five jobs, and none of them declares an Environment or
reads a signing secret: from W1's installer pull request until the next signing job,
M2's or W4's, joins the chain, the maintainer's dispatch is the only decision.
[Development Workflow](../releases/publish.md#cutting-a-release) gives the
maintainer's steps and the version rules, and
[Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
records the controls around it.

Since W1's installer pull request a release carries the Windows app, Marshal, and its
installer, and no command line and no macOS file. The releases before it carry the
command line; [The Last Command-Line Release](#the-last-command-line-release) identifies
where their procedures are.

The version is never committed. `cli/Cargo.toml`, `app/Cargo.toml` and their
lockfiles on main hold the placeholder `0.0.0-dev`, so a local build of the app
carries the product version `0.0.0-dev`. The release build writes the version over
the placeholder in the runner, with
[`.github/scripts/set-release-version.sh`](../../.github/scripts/set-release-version.sh),
before it compiles: the `[package]` version of `cli/Cargo.toml`, the
`[workspace.package]` version of `app/Cargo.toml`, and in each lockfile the version
of every package without a source, which are the workspace's own crates and
`marshal-cli` by path. The script refuses, changing no file, unless the version is
`X.Y.Z` or `X.Y.Z-<prerelease>`, each manifest has exactly one such version line,
each lockfile lists `marshal-cli` (and the app's lists `marshal-app`), and every
line it would change holds the placeholder. It writes through a temporary file, so
it keeps the CRLF line endings of a Windows checkout. `--locked` then fails if a
manifest and its lockfile disagree, and the app executable's product version is the
version from the same field. `mise run test:set-release-version` tests the script.

Each release carries three files, and nothing else:

| File                                | Built on         | Compiler host target     |
| ----------------------------------- | ---------------- | ------------------------ |
| `marshal-<version>-windows-x64.exe` | `windows-latest` | `x86_64-pc-windows-msvc` |
| `marshal-<version>-windows-x64.msi` | `windows-latest` | -                        |
| `SHA256SUMS`                        | `ubuntu-latest`  | -                        |

The `.exe` is the bytes of the built `marshal-app.exe`, a bare file, not an archive,
and the `.msi` is the installer that holds it unchanged. `SHA256SUMS` is in the
`sha256sum` format, one line for each of the other two files. Neither file is
code-signed: Windows signing is deferred until a need arises, such as Smart App
Control being On, so the MSI raises the prompts that [Installing](./install-on-windows.md#installing)
describes. A file name keeps the pattern `marshal-<version>-<target><suffix>`, and the
version tells an app file from a command-line one.

[`release-build.yaml`](../../.github/workflows/release-build.yaml) builds them. It
is a reusable workflow with `contents: read` and no publish or attest step, and its
caller passes the version. Its Windows leg builds natively on `windows-latest`, with no
cross-compilation and no cache action, and:

1. fails unless `rustc`'s host target is `x86_64-pc-windows-msvc`, so a change of
   runner image fails the build instead of shipping the wrong architecture;
2. writes the version with `set-release-version.sh`, and maps the installer's
   version, as [Installers](#installers) describes;
3. runs the CLI's base-address, instance-roots, build-definition, release-overrides,
   and agent-overrides tests and the app's mode-dispatch tests with `--release` and
   `MARSHAL_EXPECT_RELEASE=1`, the same tests the merge checks run in the release
   profile, on the target itself;
4. installs `tauri-cli` with `cargo install tauri-cli --version =<pin> --locked`
   into a directory under `$RUNNER_TEMP`, with the pin read from
   [`app/tauri-cli.version`](../../app/tauri-cli.version) by
   [`tauri-cli-version.sh`](../../.github/scripts/tauri-cli-version.sh), the one place
   the merge checks' `app` job reads it from too, so one edit moves both
   ([the app’s `tauri-cli` pin](../marshal/building.md#the-tauri-cli-pin));
5. builds the app with `cargo tauri build --no-bundle` in `app/crates/app`, and fails
   if the build changed `app/Cargo.lock`;
6. fails unless the built `marshal-app.exe`'s product version, read from its version
   resource by
   [`check-product-version.ps1`](../../.github/scripts/check-product-version.ps1), is
   the version its caller passed, in a dry run too, and runs the same check with a
   version that differs, which must fail, so a check that cannot fail does not pass;
7. copies the app to its release name, and fails if it imports a `vcruntime` DLL;
8. builds the MSI from the app and the licence notices, as the Installers section
   describes, and runs the installer test there;
9. hands the SHA-256 of the app and of the MSI to the next job as job outputs, and
   uploads those two files by name.

The leg's timeout is 50 minutes: twice the leg's wall-clock time in its first
successful dry run, 22 min 56 s in pull request #117, rounded up to whole 5
minutes.

An assemble job then downloads the leg's files and runs
[`assemble-release.sh`](../../.github/scripts/assemble-release.sh) with the version,
the directory and the two handed-over SHA-256 values. The script fails unless the
files are exactly the two expected ones and each SHA-256 matches its handed-over
value, writes `SHA256SUMS` with a line for each in a fixed order, checks it with
`sha256sum --check --strict`, and the job uploads the three files as one artifact,
`marshal-release`. That artifact is the unsigned build set: the dry run uploads it,
and a release's `assemble-signed` job starts from it and calls the same script, so
one implementation writes every `SHA256SUMS`. `mise run test:assemble-release` tests
it.

A second job, `app-macos`, builds the unsigned Mac app on `macos-latest`. It holds
`contents: read`, reads no secret and declares no Environment. It fails unless
`rustc`'s host target is `aarch64-apple-darwin`, writes the version with
`set-release-version.sh` before anything compiles, builds the app with
`cargo tauri build --no-sign` at the installer version, packs it in a `ditto` archive,
makes an unsigned disk image with `hdiutil`, and uploads both as `marshal-macos-app`,
apart from `marshal-release` and so outside the unsigned set. The archive's SHA-256
is the workflow output `app_sha256`. [Mac](../marshal/mac-app.md) owns the layout.
The job runs in the dry run and, until M2's pull request 3 wires `sign-macos`, in a
release dispatch too, where nothing reads its output and a failure of it blocks the
release.

Tauri links the Visual C++ runtime statically, so `marshal-app.exe` needs no Visual
C++ redistributable on the host, and it uses the Universal C Runtime that Windows 11
includes. [`cli/.cargo/config.toml`](../../cli/.cargo/config.toml) still sets
`+crt-static` for the library's own tests and helpers, built in `cli/`, which no
release ships. The import check runs `dumpbin /dependents`, found through `vswhere` among
the MSVC tools the build links with. It fails unless `dumpbin` listed the app's
dependencies, `KERNEL32.dll` among them, so a run that read nothing cannot pass.

A release never ships a helper: `fake-docker`, `fake-tart`, `fake-softnet`,
`fake-macos`, `instance-helper` or `commands-helper`. The release-profile tests build them into
`cli/target/release`, and the leg copies only `app/target/release/marshal-app.exe`
out, the leg's artifact holds that file and its MSI, and the assemble job, the attest
job and the release gate refuse a set of files that holds anything else.

### Installers

The MSI holds the app unchanged and is not signed. The installed `marshal-app.exe` is
byte-identical to the attested bare executable, so it matches its `SHA256SUMS` line.
The MSI's own version is the release's `X.Y.Z`, and a dry run's is `0.0.0`, while
its file name keeps the full dry-run version.
[`installer-version.sh`](../../.github/scripts/installer-version.sh) maps the
version: it keeps a release `X.Y.Z`, maps `0.0.0-dryrun.<n>` to `0.0.0`, and refuses
any other prerelease form and an `X.Y.Z` beyond a Windows Installer
`ProductVersion`'s limits (major and minor at most 255, patch at most 65535).
`mise run test:installer-version` tests it.

The MSI is built by [`build-msi.ps1`](../../.github/scripts/build-msi.ps1) from
[`marshal.wxs`](../../.github/scripts/marshal.wxs) with WiX 7.0.1, which
[`fetch-wix.sh`](../../.github/scripts/fetch-wix.sh) installs from a pinned and
checked package; [Security](../security/dependency-pins.md#pinning-convention) records
the pin. It is a per-user package named `Marshal`, manufacturer `axross`, so
Settings, Apps lists `Marshal` with the publisher `axross`. It needs no
administrator rights, and:

- installs `marshal-app.exe` and `THIRD-PARTY-NOTICES.txt`, the app's checked-in
  licence notices for Windows
  ([`app/notices/THIRD-PARTY-NOTICES-windows.txt`](../../app/notices/THIRD-PARTY-NOTICES-windows.txt)),
  into `%LOCALAPPDATA%\Programs\marshal\`, and a Start menu entry named Marshal that
  starts the app. It adds nothing to `PATH`.
- has its own upgrade code, `{F53C5F78-BA20-4941-9091-988AFDFD656B}`, which never
  changes, and its own component codes. The command line's MSI has another upgrade
  code, so neither installer upgrades or removes the other, and when both are
  installed they share the folder with different files: removing one leaves the
  other's.
- replaces an installed version that is lower or equal, a build of the same version
  included, and refuses a higher one with the message "A newer version of Marshal is
  already installed.", leaving the installed version in place.
- starts the app after an install or an upgrade only when the `msiexec` line sets
  `AUTOLAUNCHAPP=True`, with the `LAUNCHAPPARGS` property as its arguments. W4's
  update runs
  `msiexec.exe /i <msi> /passive /promptrestart AUTOLAUNCHAPP=True LAUNCHAPPARGS="<its own arguments>"`.
  The action runs the installed `marshal-app.exe` after the install is committed,
  does not wait for it, and runs in the installing user's own process, so for this
  per-user package the app runs as that user and is not elevated by the installer.
  Without `AUTOLAUNCHAPP=True` it starts nothing, and an uninstall, a refused
  downgrade, and the removal of the earlier version during an upgrade never start it.
  Reinstalling the very same MSI over itself is a maintenance run, which starts
  nothing either.
- on an uninstall that is not part of an upgrade, deletes the HKCU Run value
  `Marshal` (under `Software\Microsoft\Windows\CurrentVersion\Run`), which W3's
  sign-in start writes. It runs `reg.exe delete` from the system folder as the user,
  last in the uninstall's script and only when the value exists, so an absent value is not an error, and a failure to delete it does not
  fail the uninstall. An upgrade keeps it.
- keeps the configuration, the tokens in Credential Manager, any old token files,
  the logs in `%LOCALAPPDATA%\marshal\` and the web view's data folder,
  `%LOCALAPPDATA%\app.axross.marshal\`, when it is uninstalled.

It has no dialogs and no UI extension. Its custom actions are the downgrade message,
which runs no code, the start of the installed app, and the `reg.exe` deletion of
the one Run value.

[`test-msi-installer.ps1`](../../.github/scripts/tests/test-msi-installer.ps1) runs
on the build leg with the real `msiexec`. It installs and uninstalls the release MSI
without `AUTOLAUNCHAPP`, so the app it holds never runs, and checks its version,
upgrade code, name, publisher, the per-user context, the installed app's and the
notices' SHA-256, the Start menu entry and its target, an unchanged user `PATH`, that
no `marshal-app.exe` was started, and that the uninstall keeps the logs, the
configuration folder and the web view's data folder. It then builds throwaway MSIs at
fixed test versions from a stand-in, compiled in the test, that records its path, its
arguments and its user and exits, and checks:

- an install with `LAUNCHAPPARGS` but without `AUTOLAUNCHAPP`, and one with
  `AUTOLAUNCHAPP=1`, start nothing;
- an upgrade and a same-version rebuild with `AUTOLAUNCHAPP=True`, and an upgrade
  run with W4's line, `/passive /promptrestart AUTOLAUNCHAPP=True LAUNCHAPPARGS="..."`,
  start the installed app with `LAUNCHAPPARGS` as its arguments, as the installing
  user, and keep the Run value;
- a lower version over a higher one fails with the message, keeps the higher one, and
  starts nothing;
- an uninstall with `AUTOLAUNCHAPP=True` on its line starts nothing, removes the Run
  value `Marshal` and leaves another Run value, and an uninstall without the value
  passes;
- the extraction the optional check relies on, which holds `marshal-app.exe` and the
  notices and nothing else;
- that the MSI's custom actions are those three, with the deletion started from the
  system folder, that it writes no environment
  variable, and that no table names a machine, account or workspace;
- an upgrade while a copy of a stock waiting console program, packed as
  `marshal-app.exe`, runs from the install path.

The upgrade code is checked against the command line's, and no component may share
the command line's component code. Each measured outcome is a log line that starts
with `MEASURED:`, and the installer logs are kept as the
`installer-test-logs-windows-x64` artifact of the run. A failed check fails the leg.

### The Dry Run and the Release Workflow

[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) calls the build
workflow on every pull request and every push to the default branch, as the dry
run, with the version `0.0.0-dryrun.<run number>`: the build leg, the Mac app job and
the assemble job run, the app must carry the product version
`0.0.0-dryrun.<run number>`, the MSI is built and tested, and the `marshal-release`
artifact, with all three files, is uploaded to the run, as is `marshal-macos-app`,
but nothing is attested or published, and no job references an Environment. The `Install Blocks` job then runs the documented install blocks against
that artifact, as [What the Install Tests Cover](./install-on-windows.md#what-the-install-tests-cover)
describes. [`release.yaml`](../../.github/workflows/release.yaml) runs only on a
manual dispatch on main, in five jobs on the dispatched commit. Every job but `plan`
needs `plan` directly and runs only when the plan job handed out a version:

1. `plan` runs semantic-release as a dry run. Its gate,
   [`.github/scripts/verify-release.sh`](../../.github/scripts/verify-release.sh),
   which `@semantic-release/exec` runs from
   [`release/.releaserc.json`](../../release/.releaserc.json), hands out the next
   version. With nothing to release, it hands out none, and the other jobs are
   skipped.
2. `build` is the build workflow, which builds and assembles the unsigned files at
   that version into `marshal-release`.
3. `assemble-signed`, where every file of the release meets, needs `plan` and
   `build`. On `ubuntu-latest` with `contents: read` and no Environment, it checks
   each file against its single line in the build's `SHA256SUMS`, writes
   `SHA256SUMS` again with `assemble-release.sh`, and uploads the files as
   `marshal-release-signed`. Until a signing job joins, that set is the build's.
4. `attest` downloads `marshal-release-signed` and fails unless it holds exactly
   that version's three files, `SHA256SUMS` lists exactly the app and the MSI, and
   `sha256sum --check --strict` passes. It then attests the build provenance of every
   file `SHA256SUMS` lists with `actions/attest`. It checks nothing out and installs
   no npm package.
5. `release` downloads `marshal-release-signed` and runs semantic-release. The gate
   makes the same check for the version semantic-release worked out in that run,
   before anything is tagged. semantic-release then pushes the tag and its note, and
   `@semantic-release/github` creates a draft release with the generated notes,
   uploads the three files, and publishes it. `mise run test:verify-release` tests
   the gate.

A failure of any job stops the run before any tag is pushed. The jobs and their
`needs` chain are the contract that W4 and M2, other children of roadmap
[#72](https://github.com/axross/runners/issues/72), extend
([Extending the Release Workflows](./release-extensions.md#extending-the-release-workflows)), and
`mise run check:release-shape` fails if they drift from it.

The runner image's definition is embedded in the library (see [Build](./build-images.md#build)), so a
change to it, such as a runner bump, reaches a host only through a new release and
an update.

### The Last Command-Line Release

The last command-line release is the CLI recovery release. It was cut after W1's
installer pull request merged, not before it as F0's plan required, and the app
merges that followed do not satisfy that ordering. It releases the complete program
tree at `dc8e1ebcc0722997206487f3dcca2ae7e519cf02`, W1's first parent, through
the release workflow main had at the time. The version is the one semantic-release
worked out from main's history, not a version this document fixes. Its five files
are the Windows CLI executable, the CLI's MSI, the bare macOS CLI, the signed and
notarized macOS package and `SHA256SUMS`, and none is an app or a test helper.

To find its version, open the [releases page](https://github.com/axross/runners/releases)
and take the only release that carries a macOS `.pkg`,
`marshal-<version>-macos-arm64.pkg`. `v0.1.0` has only bare binaries and
`SHA256SUMS`, and an app release has no package. Its tag is `v<version>`.

The tag's `docs/operations/cli-recovery.md` opens the command-line guide, which
links to the procedures for installing and checking the command line on Windows and
macOS, migrating from a manual install, updating it, uninstalling it and rolling
back, at the same tag. Open that tag's document on GitHub, at
`https://github.com/axross/runners/blob/v<version>/docs/operations/cli-recovery.md`,
replacing `<version>` with the CLI recovery release's version without the `v`, and use
the procedures there rather than the app's blocks in this document. The same form
reaches any other file at that tag, such as
`docs/marshal-cli/signing.md#reading-a-signature`, which holds the commands that
read the command line's signed macOS package. The recovery guide
distinguishes the release's workflow commit from the program commit it built and
carries the optional check's pins for that release. Its program tree keeps the
command line's runtime documentation and embedded inputs. The commit before W1's
installer pull request on main, its first parent, holds the command line and
everything that pull request removed.

Moving a host from the command line to the app is that release's uninstall procedure
for the command line, then [the Windows app installation procedure](./install-on-windows.md#installing). The command line's MSI and
this one are separate products that share `%LOCALAPPDATA%\Programs\marshal\`, so
uninstalling the command line removes `marshal.exe` and its `PATH` entry and leaves
Marshal's files, and `where.exe marshal` then finds nothing.

### What the Release Tests Cover

Each of these runs in `mise run check`, on Linux, with no signing identity and no
secret:

| Task                                | What it covers                                                                                                                                                                                                           |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `mise run check:release-shape`      | Reads the `.yaml` and `.yml` workflows and fails unless the rules listed under this table hold.                                                                                                                          |
| `mise run test:release-shape`       | Runs that check against the unchanged workflows, then against copies altered one change at a time, and expects each to fail.                                                                                             |
| `mise run test:assemble-release`    | Runs `assemble-release.sh` against temporary directories: the two lines in order, and a refusal for another file set or a changed file.                                                                                  |
| `mise run test:verify-release`      | Runs the release gate against temporary directories, and checks that `.releaserc.json`'s asset globs match exactly the three files the gate passes.                                                                      |
| `mise run test:set-release-version` | Runs the version stamp against copies of the CLI's and the app's manifests and lockfiles, and the committed ones: only the versions they own change, and a file that does not hold the placeholder is refused unchanged. |

`mise run check:release-shape` enforces these rules. They apply to every `.yaml` and
`.yml` workflow, unless a rule names a file or job. Each failure names its rule, and
a workflow that cannot be read or a missing `release.yaml` or `release-build.yaml`
fails it too.

- **The Environment.** No job and no step declares an `environment`, until the next
  signing job's pull request allows its own.
- **Triggers.** `release.yaml` is triggered by `workflow_dispatch` alone. A workflow
  that `pull_request` or `pull_request_target` triggers calls only local workflows
  (`./.github/workflows/...`), so the rule above covers what it calls.
- **The signing secrets.** No workflow reads one, whether by `secrets.APPLE_*`,
  `secrets[...]`, `toJSON(secrets)` or `secrets: inherit`.
- **The chain.** Every job of `release.yaml` but `plan` needs `plan` directly and
  carries `if: github.ref == 'refs/heads/main' && needs.plan.outputs.version != ''`.
  `assemble-signed` needs `plan` and `build`, `attest` needs `plan` and
  `assemble-signed`, and `release` needs `plan` and `attest`.
- **`assemble-signed`.** It runs on `ubuntu-latest`, holds exactly `contents: read`,
  sets `timeout-minutes` to at most 10, checks out with `persist-credentials: false`,
  calls `assemble-release.sh`, and no step of it carries an `if`.
- **`attest` and `release`.** `attest` holds exactly `id-token: write` and
  `attestations: write`. Each downloads exactly one artifact,
  `marshal-release-signed`, and no step of `release` reads `needs.build`.
- **No way past a failure.** No job or step of `release.yaml` sets
  `continue-on-error`, and no job-level or step `if` uses `always()`, `failure()` or
  `cancelled()`, whatever their case and inner spaces. A step condition of `attest`
  or `release` that skips a step, such as `if: false`, is not seen.
- **The environment of `release.yaml`.** None of `PATH`, `BASH_ENV` and `ENV` is an
  `env:` key at any level.
- **The dry run's `assemble`.** Its checkouts set `persist-credentials: false`, a
  script calls `assemble-release.sh`, and none writes `SHA256SUMS` itself, which means
  a `>` or `>>` redirection to it, `tee`, or `cp` or `mv` with it as the destination;
  reading it, as `cat` or `sha256sum -c` does, is not a write.
- **The Mac app job.** `app-macos` in `release-build.yaml` runs on `macos-latest` with
  a 45-minute timeout and uses no action but `actions/checkout` and
  `actions/upload-artifact`. It uploads `marshal-macos-app` once and no other artifact,
  and its `app_sha256` output is the workflow's. A step with no condition checks that
  `rustc`'s host target is `aarch64-apple-darwin`, and another writes the version with
  `set-release-version.sh "$VERSION"`, from `inputs.version`, before any step that runs
  cargo. It builds with `--locked` and `cargo tauri build ... --no-sign`, and no step
  runs `codesign` to sign, `security import`, `notarytool` or `stapler`. `assemble`
  does not need it. Its `Check The Signing Tools` step has no condition, fails when a
  tool is missing, measures `ditto` and `hdiutil`, looks up `notarytool` and `stapler`
  with `xcrun --find`, and runs the openssl decode the staged jobs use.
