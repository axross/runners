---
type: Procedure
title: "Publish the fixed-source CLI for recovery"
description: "Determine which commits supply the program and its provenance, verify the base tag and collect the evidence required before restoring app publication."
---

# Publish the fixed-source CLI for recovery

This guide is for the maintainer collecting the final CLI installer evidence.
It covers the temporary release route and the CLI procedures kept with its tag,
not app installation. You need GitHub release access, `gh` 2.102 or later,
the existing [release signing prerequisites](../releases/publish.md#before-the-first-release),
and separately approved native hosts. No instruction here authorizes an agent
to push or move a tag, dispatch, sign, change settings or operate a host.

## Release the Fixed CLI

[Temporary CLI Recovery](../security/release-authority.md#temporary-cli-recovery)
governs the temporary release route. Normal app source and build behavior remain
available and tested, but app publication is suspended until the separate
restoration PR. This is not a second supported CLI product.

The build has two source identities:

- The complete program tree is fixed at
  `dc8e1ebcc0722997206487f3dcca2ae7e519cf02`, with its own CLI manifests, lockfile,
  toolchain, embedded runner-image inputs, fixtures and installer identities.
- The workflow and eventual tag use the actual dispatch commit on main. The
  optional download check pins that commit, not the historical program commit.
  Default provenance identifies the workflow source through OIDC; it does not
  independently prove each secondary checkout's input. The build records its
  checked historical HEAD and lock/source identities separately.

The reusable `cli-recovery-build.yaml` produces the unsigned
`marshal-cli-recovery` dry-run artifact, not a signed release.

For this bounded release, follow this order:

1. Verify the [release signing prerequisites](../releases/publish.md#before-the-first-release).
   Identify the native Windows11 host, actual Smart App Control state and safe
   baseline separately; WSL is not that host.
2. Push or confirm the base tag `v0.1.1` on `954d349` yourself, as
   [Version Rules](../releases/versions.md#version-rules) describes. Push it as a
   lightweight tag, as `v0.0.0` was, and read it back. If the tag already exists,
   skip the first two lines:

   ```bash
   git tag v0.1.1 954d349d8c31c30d7b4439aadfea6f75ee210a0f
   git push origin refs/tags/v0.1.1
   git ls-remote --tags origin v0.1.1 'v0.1.1^{}'
   ```

   The last line's SHA is the tag's commit and must be
   `954d349d8c31c30d7b4439aadfea6f75ee210a0f`. A tag on any other commit stops the
   release until you decide.

3. Verify main's merged-PR history as Development Workflow's
   [Each Release](../releases/publish.md#each-release) step 1 describes. Do it
   immediately before the dispatch.
4. Dispatch `release.yaml` on main yourself and use the exact version produced
   by its plan. Do not guess the next version from the historical commit. If the
   plan prints a version Version Rules does not expect, cancel the run before
   approving `sign-macos`.
5. Approve the signing deployment yourself. A failed build, signature,
   notarization or signed-package check prevents publication.
6. Check that the published release has exactly the Windows CLI `.exe`, CLI
   `.msi`, macOS bare CLI, signed/notarized `.pkg` and `SHA256SUMS`, all named
   `marshal-<version>-<platform>`. Record the run, tag/workflow commit, historical
   input, version and file hashes on #71.
7. Use the [CLI installation and rollback procedures](./install.md#installing-updating-and-rolling-back) with that exact version. Publish every required
   #71 step4 items2–4 measurement and native outcome there. Unknown or failed
   results remain blockers; hosted dry runs and signing stand-ins are not native
   evidence.

Only after that successful public evidence does the separate restoration PR
remove the temporary workflow/scripts and restore then-current app routing,
reconciling M2/W4 changes rather than overwriting them. Restoration closes #132,
not #71. A separate documents-only measured signing-record PR alone may close
F0's issue. Keep this tag's CLI procedures valid after restoration.
