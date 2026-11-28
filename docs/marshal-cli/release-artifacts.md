---
type: Architecture
title: "Release artifacts and workflow boundaries"
description: "Which artifacts and jobs belong to recovery and which belong to the suspended app workflow?"
---

# Release artifacts and workflow boundaries

## Releases

The active dispatch route is temporarily the fixed historical CLI, not the app.
The Temporary CLI Recovery section of [Security](../security/release-authority.md#temporary-cli-recovery)
governs that release route. [CLI Recovery](../runner-recovery/index.md) owns its operator
procedure, both source identities, actual-version CLI procedures and the
successful native-evidence prerequisite for a separate app-path restoration.
App source and normal builds remain. The app release descriptions below are the
suspended baseline to reconcile on restoration, not a second active release
graph. No recovery-code PR, dry run or app merge completes F0 or changes its
dependency gates.

A release is a `v<version>` tag on the default branch and the GitHub release that
carries its files, such as `v0.1.0`. The maintainer starts one by dispatching
[`release.yaml`](../../.github/workflows/release.yaml) on main, and semantic-release
works out the version from the commits since the last `v*` tag. Releases are never
prereleases. The suspended app baseline has five jobs, and none declares an Environment or
reads a signing secret: from W1's installer pull request until the next signing job,
M2's or W4's, joins the chain, the maintainer's dispatch is the only decision.
[Development Workflow](../releases/publish.md#cutting-a-release) gives the
maintainer's steps and the version rules, and
[Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
records the controls around it.

The suspended app baseline carries the Windows app, Marshal, and its
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
caller passes the version. One leg builds natively on `windows-latest`, with no
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
run, with the version `0.0.0-dryrun.<run number>`: the build leg and the assemble
job run, the app must carry the product version `0.0.0-dryrun.<run number>`, the MSI
is built and tested, and the `marshal-release` artifact, with all three files, is
uploaded to the run, but nothing is attested or published, and no job references an
Environment. The `Install Blocks` job then runs the documented install blocks against
that artifact, as [What the Install Tests Cover](./install-on-windows.md#what-the-install-tests-cover)
describes. The [current CLI recovery release](../security/release-authority.md#temporary-cli-recovery) has its own signing and verification stages.

The [suspended app publication workflow](https://github.com/axross/runners/blob/eada575bfc391d6053c63a90d86ade48bcba9a1b/.github/workflows/release.yaml) ran on manual dispatch on main. Its five stages are retained here for restoration. Each job after `plan` depends on it directly and runs only when it produces a version:

1. `plan` ran semantic-release as a dry run. The
   [app publication configuration](https://github.com/axross/runners/blob/eada575bfc391d6053c63a90d86ade48bcba9a1b/release/.releaserc.json)
   called the [app verifier](https://github.com/axross/runners/blob/eada575bfc391d6053c63a90d86ade48bcba9a1b/.github/scripts/verify-release.sh)
   through `@semantic-release/exec` to emit the next version. If no release was due,
   it emitted no version and the remaining jobs were skipped.
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

In the suspended workflow, the publish job could tag a version only after the preceding jobs and its release verifier passed. The
[app workflow shape check at that revision](https://github.com/axross/runners/blob/eada575bfc391d6053c63a90d86ade48bcba9a1b/.github/scripts/check-release-shape.mjs)
checked the five jobs and their dependencies. The [W4 and M2 extension guidance](./release-extensions.md#extending-the-release-workflows)
describes how roadmap [#72](https://github.com/axross/runners/issues/72) would extend that baseline after restoration.

During CLI recovery, [`mise run check:release-shape`](../../.github/scripts/check-release-shape.mjs)
checks the seven-stage CLI recovery workflow and the retained unsigned app dry run.
The [active publication configuration](../../release/.releaserc.json) calls the
[CLI recovery verifier](../../.github/scripts/cli-recovery/verify-release.sh) and publishes the five CLI release files.

The runner image's definition is embedded in the library (see [Build](./build-images.md#build)), so a
change to it, such as a runner bump, reaches a host only through a new release and
an update.

### The Update Feed's Tools and Checks

[Updates](../marshal/updates.md#updates), under The Update-Feed Key, owns the installed
app's behavior and the key procedure.
[Security](../security/app-trust.md#updates-come-only-from-the-signed-feed) owns the
release-verification trust rules, and
[the extension contracts](./release-extensions.md#w4s-feed-pull-request-and-m4s-archive)
own what the feed adds to the release graph.

The release run does not publish `latest.json` yet, because no release job signs or
writes it. The jobs that will are staged in
[`.github/staged/release-feed.yaml`](../../.github/staged/release-feed.yaml), outside
`.github/workflows/`, so GitHub never runs it and its `environment: release` reaches
no Environment. W4's last pull request moves the staged text into `release.yaml` after
the app's publication is restored, and deletes the file. The file holds:

- **`update-tool`**, which runs on `ubuntu-latest` with `contents: read`, no
  Environment, no secret and no cache. It runs
  [`build-update-tools.sh`](../../.github/scripts/build-update-tools.sh), which
  downloads the `tauri-cli` crate archive at the pin and refuses it unless its SHA-256
  equals the one [`app/tauri-cli.crate.sha256`](../../app/tauri-cli.crate.sha256)
  commits, runs `cargo deny check advisories` on the archive's own lockfile with the
  advisories the maintainer accepted in
  [`app/tauri-cli.deny.toml`](../../app/tauri-cli.deny.toml), builds `cargo-tauri`
  from it with `--locked`, and builds `marshal-feed-check` from the app's workspace.
  It hands both binaries' SHA-256 on as job outputs, apart from the `update-tools`
  artifact. Its timeout of 40 minutes is a first estimate, replaced by twice the
  job's time in its first run, as the build leg's was.
- **`sign-update`**, which runs on `ubuntu-latest` with `contents: read` under the
  `release` Environment, with no checkout; its only actions download and upload
  artifacts. Each script starts with a `# sign-update: <name>` marker. Shell built-ins
  and base utilities run by name; `awk`, `sha256sum` and `unshare` use
  `$SYSTEM_BIN/<tool>`. The check step refuses unless the MSI's SHA-256 equals the
  build's `SHA256SUMS` line and `cargo-tauri`'s equals `update-tool`'s output. The sign
  step refuses unless both secrets are set, then runs
  `unshare --map-root-user --net "$RUNNER_TEMP/update-tools/cargo-tauri" signer sign --app-version <version>`
  on a copy of the unchanged MSI, with no network. The secrets reach only that step,
  as `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in its
  `env:`. It uploads only the detached signature, as `update-signature`.
- **`assemble-signed`'s feed steps**, which check `marshal-feed-check` against
  `update-tool`'s output, write `latest.json` with
  [`write-update-feed.sh`](../../.github/scripts/write-update-feed.sh), one
  `<platform>=<file>=<signature>` argument per signed file, and run the feed check on it
  against the committed release-verification public key before `SHA256SUMS` is written.
  The platform entries are a list, so M4
  ([#90](https://github.com/axross/runners/issues/90)) adds `darwin-aarch64` as one
  more argument, with `sign-update` signing the Mac archive beside the MSI. These steps
  call `assemble-release.sh` in a `signed` mode that does not exist yet; the last pull
  request adds it, with `latest.json` in the signed set.

**The round trip in the merge checks.** The `Update Feed Round Trip` job of
`merge-checks.yaml` needs the release build's dry run. It runs on a push to main and on
pull requests that change the release workflows, the staged file, the scripts, the
`tauri-cli` files, `app/Cargo.lock`, the feed crate or the app's updater; on others it
reports success without building. It reaches no Environment and holds no secret. It builds
the tools with `build-update-tools.sh`, the script `update-tool` runs, then
[`feed-round-trip.sh`](../../.github/scripts/tests/feed-round-trip.sh) makes a throwaway
key for the run, and
[`run-sign-update.sh`](../../.github/scripts/tests/run-sign-update.sh) runs `sign-update`'s
own scripts, extracted from the staged file with
[`extract-run-scripts.mjs`](../../.github/scripts/tests/extract-run-scripts.mjs), on the
dry run's MSI with that key. The feed check passes the feed `write-update-feed.sh`
writes, and refuses, with the diagnostic for each cause, a changed MSI, another key's
signature, a signature for another version, and the throwaway signature against the
committed key; the job's output is searched for the key and its password. The
staged `update-tool` and `assemble-signed` text is not extracted: the round trip runs
the scripts those steps call. `Update Feed In The App On Windows` then runs the app's
`tests/update_feed.rs`, which gives the same feed and MSI to the updater plugin's own
`Update::verify` under the app's updater configuration with the throwaway key: it accepts
them, refuses a changed MSI and another key as signature check failures, and refuses
another version as a signed version mismatch.

### The Last Command-Line Release

The required last CLI installer release did not precede W1's installer merge.
That missed ordering is not satisfied by the app merges. The temporary recovery
instead releases the fixed complete program tree at
`dc8e1ebcc0722997206487f3dcca2ae7e519cf02`, W1's first parent, through main's
current workflow. Semantic-release selects the actual version from main history;
it is not a hard-coded next version. Its five files include the legacy CLI MSI
and signed/notarized pkg, not an app or test helper.

Open `docs/operations/cli-recovery.md` at the recovery tag and follow its Windows or macOS installation route. The instructions at that tag distinguish the workflow commit from the historical program input and carry the verifier pins for that release. For an app release, use the [Windows app installation procedure](./install-on-windows.md#installing).
The historical tree keeps the CLI runtime documentation and embedded inputs.
No public recovery signing measurement or native outcome is implied by these
instructions; the final record stays pending until the actual evidence exists.

Moving a host from the command line to the app is that release's uninstall procedure
for the command line, then [the Windows app installation procedure](./install-on-windows.md#installing). The command line's MSI and
this one are separate products that share `%LOCALAPPDATA%\Programs\marshal\`, so
uninstalling the command line removes `marshal.exe` and its `PATH` entry and leaves
Marshal's files, and `where.exe marshal` then finds nothing.

### What the Release Tests Cover

[Testing](../conventions/testing.md) owns the release-check commands, their current
coverage and CI routes for both the temporary CLI recovery and the retained app
tooling. `mise run test:verify-release` tests the retained app publication gate;
`mise run test:cli-recovery` checks the active five-file CLI publication globs in
`release/.releaserc.json`. The shape check and its mutation tests cover the
temporary seven-stage CLI route and the retained unsigned app builds. Signing
stand-ins and unsigned dry runs are not real signing or native-host evidence.
