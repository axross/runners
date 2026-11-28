---
type: Architecture
title: "Release workflow extension contracts"
description: "Which roadmap slice may extend the release graph, and what else must change with it?"
---

# Release workflow extension contracts

## Extending the Release Workflows

F0 owns the release workflows' structure. W1, W4, M2 and M4 MUST follow the
extension contracts below in their own pull requests. W1 implements the
[three-file, five-job app release](../marshal-cli/release-artifacts.md#releases); M2, W4 and M4 are requirements
for later changes, not jobs already implemented.

The first of M2's and W4's pull requests gives `assemble-release.sh` a mode, as its
first argument: `unsigned` for `release-build.yaml` and `signed` for
`assemble-signed`, which requires the signed-only files. A pull request that changes
the release set changes every place that encodes it in the same change: the
workflows, `assemble-release.sh`, `verify-release.sh` and their tests,
`.releaserc.json`, the shape check and its test, `mise.toml` descriptions, the
documents, REVIEW.md, the glossary, Testing and README.

Every extension keeps these rules:

- only signing jobs declare the Environment;
- a signing job holds `contents: read` and runs no repository code;
- checksums and attestation follow every signing step;
- no pull-request run reaches the Environment;
- F0's and M2's signing jobs use only operating-system signing tools.

W4's `sign-update` job is the one exception to the last rule. It runs `tauri-cli`'s
`signer sign`. A tool job without the Environment builds that binary with `--locked`
from a crate archive whose SHA-256 must equal one committed in the repository. The
signing job checks the binary it receives against the SHA-256 that the tool job
outputs, before running it. W1 owns the `tauri-cli` pin and W4 owns that checksum in
[`app/tauri-cli.crate.sha256`](../../app/tauri-cli.crate.sha256); a change to one MUST
move both in the same pull request, and `mise run test:update-tools` fails while the
two versions differ.

### Release Sets and Job Dependencies

W1's installer pull request MUST merge before M2's disk-image pull request.
M2's disk-image and W4's feed pull requests may then merge in either order. The file names
keep the `marshal-<version>-<target><suffix>` pattern; the version distinguishes a
command-line release from an app release. Each stage's release set is:

| From                         | Release files                                                                                                                                                |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Last command-line release    | `marshal-<version>-windows-x64.exe`, `marshal-<version>-macos-arm64`, `marshal-<version>-windows-x64.msi`, `marshal-<version>-macos-arm64.pkg`, `SHA256SUMS` |
| W1's installer pull request  | `marshal-<version>-windows-x64.exe` (the bytes of `marshal-app.exe`), `marshal-<version>-windows-x64.msi` (W1's MSI), `SHA256SUMS`                           |
| M2's disk-image pull request | adds `marshal-<version>-macos-arm64.dmg`, in the signed set only                                                                                             |
| W4's feed pull request       | adds `latest.json`, in the signed set only                                                                                                                   |

From W1's installer pull request, `marshal-release` is the unsigned build and dry-run
set: the two Windows files and `SHA256SUMS`. `marshal-release-signed` adds the disk
image and feed only when their signing jobs exist. The bare executable MUST retain
the build's bytes, and the MSI MUST install it unchanged. Every release's
`SHA256SUMS` MUST list every other release file, computed after all signing;
`attest` MUST attest every file that list names.

The required job dependencies are:

- **Last command-line release:** `plan` → `build` → `sign-macos` → `verify-macos` → `assemble-signed`
  → `attest` → `release`.
- **After W1's installer pull request:** `plan` → `build` → `assemble-signed`
  → `attest` → `release`. `assemble-signed` needs only `plan` and `build`, checks
  the Windows set and publishes it as `marshal-release-signed`.
- **With M2:** `sign-macos` and `verify-macos` return between `build` and
  `assemble-signed`, which MUST wait for them.
- **With W4:** `update-tool` needs `plan`; `sign-update` needs `plan`, `build` and
  `update-tool`; `assemble-signed` MUST also need `update-tool` and `sign-update`.
  These dependencies coexist with M2's when both extensions are present.
- **Always:** every job except `plan` MUST need `plan` directly and carry
  `if: github.ref == 'refs/heads/main' && needs.plan.outputs.version != ''`.
  `attest` and `release` MUST download only `marshal-release-signed`. Neither
  release workflow uses a cache, and no dry run reaches an Environment.

Every extension MUST keep `verify-macos` bounded at 20 minutes when present and
`assemble-signed` at 10 minutes. Build-time tool downloads are limited to:

- the pinned WiX 7.0.1;
- `tauri-cli` and its locked dependencies through Cargo;
- `update-tool`'s checksum-verified crate archive.

W1 MUST change `assemble-release.sh` to take the version, the directory and the
build's two SHA-256 values. The first of M2's and W4's pull requests MUST give it an
`unsigned` or `signed` mode as its first argument. `release-build.yaml` uses the
unsigned mode, which refuses signed-only files; `assemble-signed` uses the signed
mode, which requires every listed signed-only file and refuses unlisted files.
Each extension adds its file to that list. The disk image's expected SHA-256 comes
from `sign-macos`, checked by `verify-macos`; `assemble-signed` writes `latest.json`
itself and checks it before writing `SHA256SUMS`.

A change to the release set MUST update every place that encodes it in the same
pull request: `release-build.yaml`, `release.yaml`, `assemble-release.sh`,
`verify-release.sh`, `set-release-version.sh` where its version checks change,
their tests, `release/.releaserc.json`, `check-release-shape.mjs` and its test,
`mise.toml`'s task descriptions, the affected CLI, Security, Development Workflow
and Windows Runner Host text, REVIEW.md's asset list, the glossary, Testing and README.
The release-phase checks and semantic-release asset list MUST name the signed set.

### W1's Installer Pull Request

The Windows leg MUST install `tauri-cli` with
`cargo install tauri-cli --version =<pin> --locked` under `$RUNNER_TEMP`, without a
cache, reading the same single pin as W1's app CI. It builds with
`tauri build --no-bundle` and makes the MSI with F0's WiX 7.0.1 fetch and pin. W1's
pull request MUST record this release-build use as an exception under Security's
Pinning Convention, rather than silently treating it as a checksum-pinned binary.
Cargo checks the crate archive and locked dependencies against the crates.io
index checksums. This leg does not use W4's later committed archive checksum.

W1 MUST replace the binary's `--version` check with the built `marshal-app.exe`'s
product version matching the stamped version, including in dry runs; W4 does not
add that check later. The leg's timeout MUST be twice its first successful dry
run's wall-clock time, rounded up to a whole five minutes. W1 MUST keep these
checks, adapting them to its app build:

- the `vcruntime` import check;
- the version stamp before compilation;
- the compiler host-target check;
- the release-profile tests of the crates the leg still builds;
- an MSI test as `test-msi-installer.ps1` provides today.

W1's pull request MUST remove:

- the macOS build leg, its package build and test, its `MEASURED: signing tools`
  step, and the macOS Install Blocks legs;
- `sign-macos` and `verify-macos`, updating `assemble-signed`'s dependencies;
- `checkSigningJob`, `checkPackageArguments`, `checkMeasuredTools`, their constants,
  the `build-pkg.sh` existence rule and their cases in `test-release-shape.sh`;
- `build-pkg.sh`, `test-pkg-installer.sh`, `test-sign-macos.sh`,
  `test-signed-pkg.sh`, `test-signed-pkg-refusals.sh`, the macOS install-block suites
  and their `mise` tasks.

The generic shape-check rules MUST remain, with the new job chain and no job
allowed to declare an Environment. W1 MUST re-anchor their test cases on text
that remains, stop copying `build-pkg.sh` and `test-signed-pkg.sh` into the test
tree, and add a case refusing any job that declares an Environment. A removed
required check is a request to the maintainer to change branch settings, never
permission for an agent to change them.

The optional Windows check block MUST keep the checksum, tag-commit and
attestation checks, but stop running the binary. It compares the SHA-256 of
`marshal-app.exe`, extracted from the MSI without elevation, with the executable's
line in `SHA256SUMS`. W1 owns the wording and choice of install, update, uninstall
and rollback procedures, provided every documented block keeps a test, the
standard-user Windows test continues for W1's MSI, and the documents point to the
last command-line release's tag for both operating systems' old procedures and
the migration from command line to app.

W1 MUST update the CLI document's Releases and Installers sections, Security's
release record, Development Workflow's Each Release section, REVIEW.md, the
glossary and README alongside the scripts, tests and task descriptions. Between
this pull request and the next signing job, M2's or W4's, no job declares `release`
and there is no signing approval step. The maintainer's dispatch remains the
decision; the Environment and its secrets stay in place.

### The Restoration Point and Documents Through the Gap

The restoration point is the first parent of W1's installer commit on `main`.
Find that commit from W1's merged pull request, then use its first-parent revision,
`<W1-installer-commit>^`; `git rev-parse <W1-installer-commit>^` gives the restoration
SHA. W1's PR body MUST list what it removes. M2's body MUST cite that SHA and show
its diff against it. M2 reapplies the jobs, rules and tests onto `main` as it then
is, rather than replacing newer shared rules with an old file wholesale.

The following documents MUST remain through the gap, marked as held for M2 and W4:

- The Signing Contracts: identifier, designated requirement, team, Environment,
  secrets and certificate lifetime;
- Security's record of the Environment, secrets and G2 pin;
- Development Workflow's Before the First Release and Renewing the Certificates.

W1 removes the job descriptions in Signing the macOS Files and Security's lines
on `sign-macos` and `verify-macos`, and points Reading a Signature to the last
command-line release's tag. M2 MUST restore those descriptions for the app and
disk image. The signing identity record stays even while no job uses it, because
M2's Keychain trust depends on that identity.

### M2's Disk-Image Pull Request

M2 MUST restore `sign-macos`, `verify-macos`, the measured-tools step, their
shape-check rules and test cases from the restoration point. It leaves out the
command-line signing and package steps, the Installer identity's check, import
and exported name, the `pkgbuild` argument and `build-pkg.sh` existence rules,
and package-only tools such as `pkgbuild`, `pkgutil`, `lsbom` and `installer`.
The restored job MUST NOT read `APPLE_INSTALLER_P12_BASE64`.

Its app job in `release-build.yaml` MUST run on `macos-latest` with a 45-minute
timeout, install `tauri-cli` by the Windows leg's route, build the unsigned app,
archive it with `ditto`, and upload `marshal-macos-app` separately from
`marshal-release`. It exposes `app_sha256` as a reusable-workflow output. The
measured-tools step moves into that job, and `checkMeasuredTools` follows it.

The restored `sign-macos` MUST check the app archive against `app_sha256`, sign
inside-out without `--deep`, use the hardened runtime and a timestamp, and put
`app.axross.marshal` on the main executable. It builds the disk image with the app
job's `hdiutil create` arguments, signs the image as `app.axross.marshal.dmg`, then
notarizes and staples it. Steps keep the `# sign-macos: <name>` marker, context
values reach `run:` through `env:` only, and the job keeps its 60-minute bound.
`hdiutil` and `ditto` join the check, measured and shape-check tool lists; `ln` and
`sleep` join the allowed commands. The shape check compares the disk-image
creation arguments with the app job's.

`verify-macos` MUST read the designated requirement from the signed app. At M2's
first release, M2 compares it with the recorded `marshal` requirement. The disk
image's identifier is not the app's Keychain identity; that trust uses only
`app.axross.marshal` on the main executable.

If W4 merged first, M2 MUST retain W4's Environment rule, secret rule, job lists
and chain rules, adding `sign-macos` alongside `sign-update`. Each shared rule
keeps a case for each job. M2 also updates the signed release set and restores the
documents named above, without restoring the command-line package.

### W4's Feed Pull Request and M4's Archive

W4's feed work lands in two pull requests. The first adds the tool script, the signing
scripts, the script that writes `latest.json` and the merge checks' round trip, and
stages the text of `update-tool`, `sign-update` and `assemble-signed`'s feed steps in
[`.github/staged/release-feed.yaml`](../../.github/staged/release-feed.yaml). It
changes no release workflow, `release/.releaserc.json`, `verify-release.sh` or release
rule of the shape check, and nothing in it reaches an Environment. The last pull
request moves the staged text into `release.yaml` as it is after restoration, deletes
the staged file and points the merge checks' extraction at `release.yaml`. The
requirements below are that last pull request's, and it reviews whatever the release
graph then needs.
[The update feed's tools and checks](./release-artifacts.md#the-update-feeds-tools-and-checks)
describes what exists now.

W4 MUST add `update-tool` on `ubuntu-latest`, with `contents: read`, no Environment,
no secret or cache, and a checkout with `persist-credentials: false`. It runs W4's
tool script, also run by the merge checks, and uploads the built `tauri-cli` and
feed checker with their SHA-256 outputs. Its timeout is twice its first
successful run's wall-clock time, rounded up to a whole five minutes.

`sign-update` MUST run on `ubuntu-latest`, with `environment: release`,
`contents: read`, no checkout and only the download and upload artifact actions.
Every `run:` starts with `# sign-update: <name>`, with no `${{ }}` inside it.
Before signing, the job MUST refuse unless all three conditions hold:

- the MSI matches its `SHA256SUMS` line in `marshal-release`;
- the tool matches `update-tool`'s SHA-256 output;
- both feed secrets are set.

It runs the tool by its absolute path under `$RUNNER_TEMP`, passes
the two secrets through `env:` only to that signing step, uploads only the
signature and has a 15-minute timeout.

`assemble-signed` MUST write `latest.json` from the signature, with a URL naming
`marshal-<version>-windows-x64.msi` under that release's tag. Before writing
`SHA256SUMS`, it checks the feed checker against `update-tool`'s output and runs
it. W4's pull request updates the signed set and its checks, as well as Security's
secret statement and Before the First Release: the maintainer adds
`UPDATE_FEED_SIGNING_KEY` and `UPDATE_FEED_SIGNING_KEY_PASSWORD` to `release`
before the first release with the feed.

W4 MUST extend the shape check to allow `release` on `sign-update` and close the
secret-detection gap: in `release.yaml` and `release-build.yaml`, any secret read
outside a signing job fails; `sign-update` may read its secrets only in the
`signer sign` step. `sign-macos` stays checked per job because more than one step
reads its secrets. Across every workflow, the signing-secret rule MUST also
recognize both feed secret names. Other workflows retain the name-specific rule,
so the review workflow's unrelated token still passes. The new rules
MUST enforce the job requirements above for `update-tool` and `sign-update`, with
a test case for each rule and each signing job when M2 is already present.

M4's Mac update archive's app MUST be signed and notarized in `sign-macos` using
M2's steps. Its feed signature is made in `sign-update`; if M4 signs that archive
there, M4's plan MUST add `sign-macos` to `sign-update`'s dependencies. M4 owns the
archive commands, and its `darwin-aarch64` feed entry follows W4's contract.

### Extending the Shape Check

An extension MUST add its jobs or steps to `check-release-shape.mjs` and its
mutation tests in the same pull request, under REVIEW.md's release lens and the
maintainer's review. [CLI recovery release authority](../security/release-authority.md#temporary-cli-recovery) owns the active release
boundary, including the permitted signing Environment. The check guards against
mistakes, not a hostile pull request that edits the check itself. It does not see
a write to `SHA256SUMS` in either build's `assemble` job other than by a
redirection, `tee`, `cp` or `mv`.
