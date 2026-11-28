---
type: Specification
title: "Release signing identities"
description: "Which signatures and identities bind the CLI, package and planned app artifacts?"
---

# Release signing identities

## Signing, Held for M2 and W4

No job of `release.yaml` signs anything from W1's installer pull request until the
next signing job joins the chain: M2's disk-image pull request brings back
`sign-macos` and `verify-macos`, rewritten for the app and the disk image, and W4's
feed pull request adds `update-tool` and `sign-update`. Each joins before
`assemble-signed`, which then needs it. Until then no job declares the `release`
Environment, and the Environment and its secrets are kept for those jobs.
[Signing the Mac App (Staged)](#signing-the-mac-app-staged) describes M2's two jobs,
which pull request 2 stages outside `.github/workflows/`.
[Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
records that period.

What W1's installer pull request removed, `sign-macos`, `verify-macos`, the macOS
build leg with its package build, package test and measured-tools step, their
shape-check rules and test cases, and the macOS scripts and install-block suites, is
held at the first parent of that pull request's commit on main, the restoration
point. M2's pull request cites that commit's SHA and brings back what its plan keeps.
The contracts below stay in force through the gap.

## The Signing Contracts

These are held for M2 and W4: no job uses them until M2's or W4's signing job
joins the chain, and they stay in force through the gap. F0 owns these. W1, W4, M2
and M4 consume them. Each is a child of roadmap
[#72](https://github.com/axross/runners/issues/72), as F0 is:

- **F0**, "Installers and release signing", is the child this section describes.
- **W1**, "Windows tray app, first release", has an installer pull request that
  replaces the release's files. Its app's bundle identifier is the identifier
  below, because the app and its agent are one code object. W1 owns that use.
- **W4**, "Automatic updates, on Windows", signs the update archive. The archive's
  signature is W4's contract. W4 makes it in W4's own update-signing job, under the
  rules in [Extending the Release Workflows](./release-extensions.md#extending-the-release-workflows). M4
  may also sign the Mac's update archive there.
- **M2**, "Mac app", signs the Mac app with the same identifier and team, and
  notarizes and staples its disk image, in F0's signing job. The commands and the
  layout are M2's. M2 also holds the Keychain store. Its Keychain items trust the
  signed Marshal executable, which is the code object that carries the identifier in
  the first signing contract, and must be created by F0's stable Developer ID identity.
- **M4**, "Mac app: login, power and updates", updates the installed Mac
  app. Its updates keep M2's Keychain tokens working only while the identifier and the
  team stay the same.

The contracts F0 owns are these:

- **The identifier** is `app.axross.marshal`, and it never changes. One code object
  carries it: the `marshal` binary until W1's installer pull request, and from M2 on
  the Mac app bundle's signature, through its main executable. F0 proposes that from
  M2 on `verify-macos` reads the designated requirement from the signed app, and M2's
  Keychain trust matches it. F0 also asks M2's plan to compare, at M2's first release, the
  app's requirement with the one recorded for `marshal`, and to record any difference.
  M2's plan accepts or answers both. The package's own identifier is the same
  string. On Windows nothing is signed, so the identifier binds only the Mac. Changing
  the identifier or the team breaks the Keychain trust that M2 relies on, so a
  change goes through a roadmap revision.
- **The designated requirement** is the default one for a Developer ID signature: this
  identifier, `anchor apple generic`, the Developer ID intermediate and leaf markers,
  and `certificate leaf[subject.OU]` equal to the team ID. Two forms of it are known,
  and `verify-macos` accepts exactly these, after it drops `/* … */` comments,
  collapses whitespace and unquotes the team ID:
  - **Form A**, the expected one, is the plain `codesign` default: the conjunction of
    `anchor apple generic`, `identifier "<id>"`,
    `certificate 1[field.1.2.840.113635.100.6.2.6]`,
    `certificate leaf[field.1.2.840.113635.100.6.1.13]` and
    `certificate leaf[subject.OU] = <team>`, in any order, compared as a set of
    clauses. It is inferred, not yet observed, because TN3127 says plain `codesign`
    applies a different default from Xcode's and does not print it.
  - **Form B** is the requirement Xcode writes for Developer ID code, from TN3127,
    compared as a whole string:
    `anchor apple generic and identifier "<id>" and (certificate leaf[field.1.2.840.113635.100.6.1.9] or certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = <team>)`.

  The script prints a `MEASURED:` line naming the form that matched, then the
  requirement, so the first signed release records which one it was. Keychain items
  that trust the requirement are expected to keep trusting later releases signed under
  the same identifier and team, including after a certificate renewal; that is
  inferred, not observed. After the first signed release, a later change records the
  team ID, the form that matched, and the exact requirement printed for the bare
  binary here, as the text that M2 consumes.

- **The team** is pinned by the Environment variable `APPLE_TEAM_ID`. `sign-macos`
  refuses a certificate of another team, and `verify-macos` refuses a designated
  requirement that is neither form A nor form B above, each built from the identifier
  and the team ID that `sign-macos` output after checking it.
- **The Environment** is named `release`. The maintainer creates it and its secrets,
  and agents never change them or read the settings:
  - deployment branches: selected branches, `main` only, no tags;
  - required reviewers: the maintainer, with "Prevent self-review" off, so the
    maintainer can approve a run they started;
  - "Allow administrators to bypass": off.
- **The secrets** are Environment secrets only, and no repository secret is added.
  Both certificates come from the Developer ID G2 intermediate.

  | Secret                         | Holds                                                             |
  | ------------------------------ | ----------------------------------------------------------------- |
  | `APPLE_APPLICATION_P12_BASE64` | The Developer ID Application identity as a `.p12`, base64-encoded |
  | `APPLE_INSTALLER_P12_BASE64`   | The Developer ID Installer identity as a `.p12`, base64-encoded   |
  | `APPLE_P12_PASSWORD`           | The password of both `.p12` files                                 |
  | `APPLE_NOTARY_KEY_P8`          | The App Store Connect team API key's `.p8` text                   |
  | `APPLE_NOTARY_TEAM_KEY_ID`     | The key's ID                                                      |
  | `APPLE_NOTARY_ISSUER_ID`       | The issuer ID                                                     |

  The team ID is the Environment variable `APPLE_TEAM_ID`, not a secret.

- **A missing Environment.** A dispatch that finds no `release` Environment makes
  GitHub create one with no protection rules and no secrets. `sign-macos` then
  refuses on the empty secrets, before it imports anything.
  [Development Workflow](../releases/publish.md#before-the-first-release) has the
  maintainer create the Environment first.
- **Certificate lifetime.** The Developer ID certificates are expected to last one
  year, and the maintainer confirms the dates from the certificates. Software
  signed with a secure timestamp keeps working after its certificate expires.
  Whether an older release's package still installs after its Installer certificate
  has expired is unconfirmed, and bears on rolling back to it.
  [Development Workflow](../releases/signing-renewal.md#renewing-the-certificates) says how
  to renew.

## Signing the Mac App (Staged)

**Staged until M2's pull request 3.** The text below describes jobs that no
workflow runs yet. M2's disk-image work is split in two, as
[M2's Disk-Image Pull Request](./release-extensions.md#m2s-disk-image-pull-request)
records. Pull request 2 stages `sign-macos` and `verify-macos`, brought back from the
restoration point and rewritten for the app and the disk image, in
[`.github/staged/release-macos-app.yaml`](../../.github/staged/release-macos-app.yaml),
outside `.github/workflows/`. GitHub never runs that file, so its
`environment: release` reaches no Environment, and the shape check and `actionlint`
do not read it. Pull request 3 moves the jobs into `release.yaml`, makes
`assemble-signed` need them again and deletes the staged file; until then no job of
any workflow signs anything.
[`test-staged-sign-macos.sh`](../../.github/scripts/tests/test-staged-sign-macos.sh)
and
[`test-signed-dmg-refusals.sh`](../../.github/scripts/tests/test-signed-dmg-refusals.sh)
test the staged jobs against stand-ins, and neither shows what Apple's tools answer.

| Job            | Needs                         | Runs on, timeout           | Permissions      | Environment | What it holds                                                                                                                                                                                                                                                                                                                |
| -------------- | ----------------------------- | -------------------------- | ---------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sign-macos`   | `plan`, `build`               | `macos-latest`, 60 minutes | `contents: read` | `release`   | `APPLE_APPLICATION_P12_BASE64`, `APPLE_P12_PASSWORD`, the three notary secrets and `APPLE_TEAM_ID`; not `APPLE_INSTALLER_P12_BASE64`. Downloads `marshal-macos-app`; uploads `marshal-macos-arm64-signed` and `notary-log-macos`. It checks nothing out.                                                                     |
| `verify-macos` | `plan`, `build`, `sign-macos` | `macos-latest`, 20 minutes | `contents: read` | none        | No secret and no signing identity. Checks out the repository with `persist-credentials: false`, downloads three artifacts, checks the app archive against `app_sha256` and the disk image against `sign-macos`'s hash, and runs `test-signed-dmg.sh` with the team ID and the notary submission ID that `sign-macos` output. |

The jobs name `plan` and `build`, which `release.yaml` has: `build` calls
`release-build.yaml`, whose workflow output `app_sha256` comes from its `app-macos`
job. The kept uploads, the signed disk image and the notary log, hold no secret.
The workflows ask GitHub to keep them for 14 days; GitHub keeps them for the
repository's artifact retention limit instead when that is shorter.

`sign-macos` is the one staged job that declares the Environment or reads a signing secret.
[Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
owns what it may hold and run: `contents: read`, no checkout, no repository file, no
package manager, no fetch, and no action but `actions/download-artifact` and
`actions/upload-artifact`. Its steps are the job's own definition in the staged file, and in `release.yaml` once wired,
and its only network use is the artifact transfer and Apple's timestamp, notary and
ticket services. Each signing tool is called by its absolute system path under
`/usr/bin/`: `security`, `codesign`, `openssl`, `plutil`, `shasum`, `xcrun`, `ditto`
and `hdiutil`, so nothing earlier on `PATH` can stand in for one. The scripts read the
prefix from `SYSTEM_BIN`, whose default is that literal path and which only the
stand-in test overrides; pull request 3 adds the shape-check rules for the wired
job. Its steps run in this order:

1. It downloads `marshal-macos-app`, the app job's unsigned app archive and unsigned
   disk image.
2. It refuses, before importing anything, unless:
   - every secret it reads and `APPLE_TEAM_ID` is non-empty, and `APPLE_TEAM_ID` is
     10 characters of A-Z and 0-9;
   - the version is `X.Y.Z`, and the app job's `app_sha256` is a lowercase SHA-256;
   - the downloaded files are exactly `marshal-<version>-macos-arm64-app.zip` and
     `marshal-<version>-macos-arm64.dmg`, and the archive is a regular file;
   - the archive's SHA-256 equals `app_sha256`, so it signs what the app job built;
   - every tool exists at its path, and `xcrun --find` finds `notarytool` and
     `stapler`.
3. It creates a temporary keychain with a random password that the log masks. It
   refuses the embedded Developer ID G2 intermediate unless its SHA-256 equals the pin
   in [Security](../security/dependency-pins.md#pinning-convention), and that check comes
   before any `security` call. It then imports the intermediate and the Application
   `.p12`. The intermediate is embedded because the runner image's trust in it is
   unknown and the job fetches no certificate. It expires on 2031-09-17, and replacing
   it is a change to the job and to the pin.
4. It lists the keychain's identities and refuses unless there is exactly one
   Developer ID Application identity, and it must be valid. It prints the subject,
   team and expiry date of the certificate whose SHA-1 hash equals the identity's,
   found with `security find-certificate -a -Z -c <name>`, and refuses unless the name
   matches exactly one certificate, so what it checks is what `codesign` signs with.
   It refuses a certificate whose team differs from `APPLE_TEAM_ID` and an expired
   one, and it warns with a workflow warning annotation when the certificate expires
   within 90 days.
5. It unpacks the archive with `ditto -x -k` and refuses unless it holds only
   `Marshal.app`, `Contents/MacOS` holds only `marshal-app`, the app has none of
   `Contents/Frameworks`, `PlugIns`, `Helpers`, `XPCServices` and `Library`, and its
   `Info.plist` names `app.axross.marshal` and the release version.
6. It signs the app with the Application identity, selected by its SHA-1 hash,
   `--options runtime`, `--timestamp` and `--identifier app.axross.marshal`, without
   `--deep`: the app's one executable is its main executable, so signing the bundle
   signs inside-out. It then runs `codesign --verify --strict`, reads the designated
   requirement, and refuses unless it is form A or B of
   [The Signing Contracts](#the-signing-contracts).
7. It copies the signed app with `ditto` into a staging folder beside a link
   `Applications` to `/Applications`, and builds the disk image with
   `hdiutil create -volname Marshal -srcfolder <stage> -format UDZO -ov`, the app
   job's arguments, which the stand-in test compares with the app job's now and the
   shape check will once the job is wired. `hdiutil create` gets three
   attempts 10 seconds apart, because it has failed on hosted runners with "Resource
   busy". It signs the image with `--timestamp` and
   `--identifier app.axross.marshal.dmg` and verifies it.
8. It notarizes the disk image with
   `xcrun notarytool submit … --wait --timeout 30m --output-format json`, and reads
   the result with `plutil`. It refuses unless the status is `Accepted`, whatever
   `notarytool` exited with, and a timeout is therefore a failure. Whenever a
   submission ID exists it fetches the notary log, which Apple says to read even
   after a success.
9. It staples the disk image, runs `stapler validate` and `codesign --verify --strict`
   again.
10. It outputs the disk image's SHA-256 as `macos_dmg_sha256`, the team ID and the
    submission ID, and uploads the disk image.

Whether the job passes or fails, the clean-up step deletes the keychain, the `.p12`
file and the key file, and the notary log is uploaded when one exists. These two
steps run with `if: always()`, and no other step in the job may.

No secret is printed. Secret values reach the scripts only through `env:` and files
written with mode 0600 under the runner's temporary directory, and each line of the
`.p8` is masked as well. Two exceptions to "never passed on a command line" are
recorded, the password arguments of `security` and the `--key-id` and `--issuer`
arguments of `notarytool`, and the `.p8` private key reaches `notarytool` only as a
file path.
[Security](../security/disclosure.md#nothing-identifying-or-secret-is-committed)
records both and the reasons.

`verify-macos` first checks the app archive against `app_sha256` and the disk image
against `macos_dmg_sha256`, then runs
[`test-signed-dmg.sh`](../../.github/scripts/tests/test-signed-dmg.sh) on a runner
that holds no signing identity, with the team ID and the submission ID that
`sign-macos` output. The script refuses unless:

- the disk image verifies with `codesign --verify --strict` under
  `app.axross.marshal.dmg`, signed by a Developer ID Application certificate of the
  team with a secure timestamp, and not ad hoc;
- `stapler validate` passes, and `spctl` assesses the image for opening, by its
  primary signature, as notarized Developer ID;
- the notary log's `jobId` is the submission `sign-macos` made;
- mounted read-only, its volume is named `Marshal` and holds only `Marshal.app` and a
  link `Applications` to `/Applications`, apart from `.DS_Store`, `.fseventsd` and
  `.Trashes`;
- the app's files are the unsigned archive's, by path and type, apart from
  `Contents/_CodeSignature/`, and its `Info.plist` values are the archive's;
- the app verifies with `codesign --verify --strict` under `app.axross.marshal`,
  signed by a Developer ID Application certificate of the same team with the
  hardened runtime and a secure timestamp, and its designated requirement is form A
  or B, nothing else being accepted;
- `spctl` accepts the app for execution.

It prints `MEASURED:` lines for both `spctl` answers, which form of the designated
requirement matched and the requirement itself, and outputs the requirement as
`designated_requirement`. That is the first time the signed disk image is checked,
and it happens in the release run: a dry run cannot sign. At M2's first release the
maintainer compares that requirement with the one recorded for `marshal`.

Nothing here has run with a real signing identity, and nothing runs it before pull
request 3. The dry run builds the unsigned app, its archive and an unsigned disk image
with the same `hdiutil create` arguments, and measures the tools.
`test-staged-sign-macos.sh` runs the staged signing scripts against stand-ins, and
checks that no workflow names the staged file, that its `hdiutil create` arguments
equal the app job's and that the app job measures every tool the scripts call.
`test-signed-dmg-refusals.sh` runs the checking script against stand-ins. The first
signed release is therefore the first real run of `sign-macos` and `verify-macos`. A
defect stops that release before its tag, and a later pull request fixes it.

## Reading a Signature

No file of a release is signed until M2's disk-image pull request 3. The commands
that read the command line's signed macOS package are in this section of this document at
the last command-line release's tag. [The Last Command-Line Release](./release-artifacts.md#the-last-command-line-release)
says how to find that tag's version and open the file there. The commands for the
Mac app's files are below, staged with its jobs.

### Reading a Mac Signature (Staged)

These commands show what a release's Mac files carry once the staged signing jobs
are wired into the release, in pull request 3; until then no release carries a disk
image, and the command line's package is read at the last command-line release's tag. They
are information, not steps of the install procedure, and the answers below are what
the documentation leads to expect, not what a Mac has shown.

```bash
codesign --verify --strict -vv marshal-<version>-macos-arm64.dmg
spctl --assess --type open --context context:primary-signature -vv marshal-<version>-macos-arm64.dmg
xcrun stapler validate marshal-<version>-macos-arm64.dmg
codesign --verify --strict -vv /Applications/Marshal.app
codesign -d -r- /Applications/Marshal.app
spctl --assess --type execute -vv /Applications/Marshal.app
```

- `codesign --verify` on the disk image is expected to pass, and `codesign -dv` to
  show the identifier `app.axross.marshal.dmg` and a Developer ID Application
  certificate of the maintainer's team. An operator who wants the team checked
  compares the team ID it shows with the one this document records once the first
  signed release has fixed it. Gatekeeper assesses only a file that carries the
  download mark, which a disk image fetched with `gh` or `curl` lacks.
- `spctl --assess --type open` is expected to print `accepted` with
  `source=Notarized Developer ID`, and `xcrun stapler validate` passes for a stapled
  image.
- `codesign -d -r-` on the installed app prints the designated requirement of the
  identifier contract, and `spctl --type execute` is expected to accept the app as
  notarized Developer ID.

The command line's signed macOS package is read with the commands in the
Reading a Signature section of this document at the last command-line release's tag.
