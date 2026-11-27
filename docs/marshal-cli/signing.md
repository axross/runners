---
type: Specification
title: "Release signing identities"
description: "Which signatures and identities bind the CLI, package and planned app artifacts?"
---

# Release signing identities

## Signing, Held for M2 and W4

The temporary recovery reinstates bare-CLI/pkg signing under CLI Recovery's
bounded exception. The following held contract concerns the app path after
restoration, not that temporary CLI job.

No job of the suspended app baseline signs anything from W1's installer pull request until the
next signing job joins the chain: M2's disk-image pull request brings back
`sign-macos` and `verify-macos`, rewritten for the app and the disk image, and W4's
feed pull request adds `update-tool` and `sign-update`. Each joins before
`assemble-signed`, which then needs it. Until then no job declares the `release`
Environment, and the Environment and its secrets are kept for those jobs.
[Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
records that period.

What W1's installer pull request removed, `sign-macos`, `verify-macos`, the macOS
build leg with its package build, package test and measured-tools step, their
shape-check rules and test cases, and the macOS scripts and install-block suites, is
held at the first parent of that pull request's commit on main, the restoration
point. M2's pull request cites that commit's SHA and brings back what its plan keeps.
The contracts below stay in force through the gap.

## The Signing Contracts

The temporary recovery uses these contracts for the bare CLI/pkg. They also remain
held for M2 and W4's app-signing jobs after restoration. F0 owns these. W1, W4, M2
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

## Reading a Signature

For the temporary CLI recovery, these read-only commands inspect the actual
release package and installed CLI. Replace `<version>` with its real version.
They do not install or sign anything, and expected output is not measured evidence:

```bash
pkgutil --check-signature marshal-<version>-macos-arm64.pkg
spctl --assess --type install -vv marshal-<version>-macos-arm64.pkg
xcrun stapler validate marshal-<version>-macos-arm64.pkg
codesign --verify --strict -vv /usr/local/bin/marshal
codesign -d -r- /usr/local/bin/marshal
```

The package is expected to show a Developer ID Installer chain of the same team
as the CLI, a notarized Developer ID assessment and a valid staple. The bare or
installed CLI's designated requirement must match the existing identifier/team
contract; its notarization is read from the CDHash in the notary log, not
`spctl --type execute`. The actual team, requirement form and native outcomes remain
pending for F0's measured record. App/dmg signature instructions remain M2's work
after restoration, not substitutes for these CLI/pkg subjects.
