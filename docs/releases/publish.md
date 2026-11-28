---
type: Procedure
title: Publish or recover a release
description: Prepare signing, check the planned release version, dispatch from main and recover a partial publication.
---

# Publish or recover a release

## Cutting a Release

The maintainer releases the app by dispatching
[`release.yaml`](../../.github/workflows/release.yaml) on main. semantic-release
works out the version from the commits, as [Version Rules](./versions.md#version-rules) describes.
[Release artifacts and workflow boundaries](../marshal-cli/release-artifacts.md#releases) describes what the workflow builds, and the
[release policy](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
records why each control is there. Until M2's pull request 3 wires `sign-macos`, the
dispatch's build also runs the Mac app job, and a failure of that job blocks the
release; nothing in the release reads its output. The maintainer takes every step in this
section. The [standing delivery grant](../agent-work/delivery-grant.md#authorizing-delivery-operations) lets an explicit human instruction cover release,
deployment, and settings effects, but the release rule in Security is stricter: an
agent never dispatches `release.yaml`, never approves a deployment of it, never
pushes, moves, or deletes a `v*` tag, and never changes release settings, including
the `release` Environment, even when the maintainer asks in the session or another
agent relays it.

### Before the First Release

The `release` Environment's part of this section, step 2, is held for M2 and W4: from
W1's installer pull request until M2's or W4's signing job returns, no job declares
the Environment, and the Environment, its secrets and its variable are kept for that
job, which then needs them as this step sets them. The other steps were the first
dispatch's prerequisites. Step 3 records how the base tag `v0.0.0` was pushed for the
first release. The CLI recovery release had its own base tag, `v0.1.1`, which
[Version Rules](./versions.md#version-rules) describes.

Do these once, before the first dispatch:

1. Turn on immutable releases, under Settings, General, Releases.
2. Create the `release` Environment, under Settings, Environments, with its secrets
   and variable. Do this before the first dispatch: a dispatch that finds no `release`
   Environment makes GitHub create one with no protection rules and no secrets, and
   a signing job then refuses on the empty secrets. Set it as
   [CLI](../marshal-cli/signing.md#the-signing-contracts) and
   [Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
   record it:
   - **Deployment branches:** selected branches, `main` only, no tags.
   - **Required reviewers:** the maintainer, with "Prevent self-review" off,
     because the maintainer is the only reviewer and approves runs they started.
   - **Allow administrators to bypass:** off.
   - **Secrets**, Environment secrets and not repository secrets:
     `APPLE_APPLICATION_P12_BASE64`, `APPLE_INSTALLER_P12_BASE64`,
     `APPLE_P12_PASSWORD`, `APPLE_NOTARY_KEY_P8`, `APPLE_NOTARY_TEAM_KEY_ID` and
     `APPLE_NOTARY_ISSUER_ID`.
   - **Variable**, not a secret: `APPLE_TEAM_ID`, the team ID of both certificates:
     10 characters of A-Z and 0-9.

   What the secrets hold:
   - The two Developer ID certificates, Application and Installer, both from the
     Developer ID G2 intermediate. Only the Account Holder of the Apple Developer
     Program creates them. Export each identity as its own `.p12` with the same
     password, since Keychain Access exports one identity at a time, and base64-encode
     each file for its secret.
   - A team API key for notarization, from App Store Connect, with its key ID and
     the issuer ID. Which role the key needs rests on forum reports, not on Apple's
     pages, and the first signed release is the first to show it.

   Never write a secret value into a file, an issue, a pull request or a session.
   Record each certificate's expiry date and the membership's renewal date in the
   tracking issue, because the [certificate renewal procedure](./signing-renewal.md#renewing-the-certificates) starts from them.

3. Push the base tag `v0.0.0` on `d33d097`:

   ```bash
   git tag v0.0.0 d33d097
   git push origin refs/tags/v0.0.0
   ```

   semantic-release counts from the last `v*` tag, and a bare tag with no release
   is enough. `d33d097` is the only commit that works. On an earlier commit, the
   breaking `refactor(cli)!` of #54 would come after the tag, and the first release
   would be `1.0.0`. The next commit, `82f74ca`, holds the tag-triggered
   `release.yaml` of #53, so a tag there would start that workflow and give a red
   run. A tag on any later commit would put this flow's own `feat(release)` commit
   inside `v0.0.0`, so it would not count toward a release. On `d33d097`, the first
   release is `0.1.0`.

4. Read the setting and the tag back, and record the result where the plan for the
   first release names:

   ```bash
   gh api repos/axross/runners/immutable-releases
   gh api repos/axross/runners/environments --jq '[.environments[].name]'
   gh api repos/axross/runners/git/ref/tags/v0.0.0 --jq .object.sha
   ```

   Also report the Environment's settings from the Settings page: the branch rule, the
   reviewer, the self-review setting and the administrator-bypass setting. Agents
   cannot read them. Report the six secret names, never their values, both
   certificates' expiry dates and the membership's renewal date.

### Each Release

1. Before dispatching, check that every commit on main since the last release
   came from a merged pull request:

   ```bash
   git fetch origin main --tags
   last="$(git describe --tags --abbrev=0 --match 'v*' --exclude 'v0.1.1' origin/main)"
   gh release view "$last" --repo axross/runners --json tagName,isDraft \
     --jq 'if .isDraft then "NO PUBLISHED RELEASE: \(.tagName)" else "start: \(.tagName)" end' \
     || echo "NO PUBLISHED RELEASE: ${last}"
   for sha in $(git rev-list --first-parent "${last}..origin/main"); do
     pr="$(gh api "repos/axross/runners/commits/${sha}/pulls" --jq '[.[] | select(.merged_at != null and .base.ref == "main") | "#\(.number)"] | join(" ")')"
     printf '%s %s\n' "$(git log -1 --format='%h %s' "$sha")" "${pr:-NO MERGED PULL REQUEST}"
   done
   ```

   `git describe` returns the nearest tag, and `v0.1.1` is the base tag of the
   recovery release, which has no release behind it. Without `--exclude`, the
   check would start at `v0.1.1` and skip every commit from `v0.1.0` up to and
   including `954d349`. With it, the check starts at `v0.1.0` until the recovery
   release is published, and at that release afterwards. Once a release exists after
   `v0.1.1`, the exclusion has no effect and may be dropped.

   The `start:` line names where the check starts. A line that starts with
   `NO PUBLISHED RELEASE` instead means the start is a `v*` tag other than
   `v0.1.1` with no published release, so the check would not start at the last
   release. Do not dispatch: find out who pushed that tag and why, as for a
   commit with no merged pull request.

   Each line names a commit and the pull request that merged it. A line that ends
   in `NO MERGED PULL REQUEST` is a commit pushed to main directly, which the
   ruleset on main does not block. Do not dispatch: find out who pushed it and why.
   [Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
   records this as a residual risk of the npm tree's token.

2. Dispatch `release.yaml` on main, from Actions, Release, Run workflow, or with
   `gh workflow run release.yaml --repo axross/runners --ref main`. A dispatch made
   while another release runs is queued behind it. The `release` concurrency group
   holds one queued run, so a later dispatch cancels an earlier one that is still
   queued, and only the latest of them runs.
3. When there is nothing to release, the plan job hands out no version, and the
   other jobs are skipped. The run is green and releases nothing.
4. When there is something to release, read the version in the plan job's log
   ("The next release is <version>."). If [Version Rules](./versions.md#version-rules) does not expect
   it, cancel the run before the release job tags it. Otherwise the run builds,
   assembles, attests and releases without waiting for an approval: from W1's
   installer pull request until M2's or W4's signing job returns, no job declares
   the `release` Environment, so step 1's check at the dispatch is the only one. The
   signing job that returns brings back an approval here, with its own checks. A
   failure in `build`, `assemble-signed` or `attest` stops the run before any tag is
   pushed; fix it in a pull request and dispatch again.
5. After a green run, check that the release exists and carries exactly the app
   executable, the MSI and `SHA256SUMS`:

   ```bash
   gh release view v<version> --repo axross/runners --json isDraft,assets --jq '{isDraft, assets: [.assets[].name]}'
   ```

   A green run can still release nothing. The plan and release jobs both check out
   the commit main was on at the dispatch, and each checks that main has not moved
   since. If main moved before the plan job ran, the plan job logs "The local
   branch main is behind the remote one, therefore a new version won't be
   published.", hands out no version, and the other jobs are skipped. If main moved
   after the plan job, the release job logs the same line and ends without a tag or
   a release. Either way, run step 1 again and dispatch again.

6. Check the release with the optional check block in
   [Checking and installing the Windows app MSI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back) before opening its
   installer on a host. This is the maintainer's release verification, not a step
   an operator must take. The first release of the app is also the first run of
   `gh attestation verify` against its MSI and of the check block's comparison of the
   extracted `marshal-app.exe` with its `SHA256SUMS` line, so for that release run
   the check block before opening the MSI on a host. A failure there is a release
   defect, and it leaves the check unusable for that release. The WiX pin is
   recorded under [Pinning Convention](../security/dependency-pins.md#pinning-convention).
   The MSI and the app are not signed, as
   [Security](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main)
   records.

### Cleaning Up a Partial Release

semantic-release pushes the tag `v<version>` and its note,
`refs/notes/semantic-release-v<version>`, before `@semantic-release/github` creates
a draft release, uploads the files, and publishes it. If that last part fails, the
tag is left without a published release. It still counts as the last release, so
the next dispatch would skip that version. The [release policy](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main) reserves cleanup and dispatch to the maintainer. Before deleting a tag, confirm that the release was never published and that the tag is not the deliberate `v0.1.1` base tag. Then:

1. Delete the draft release, if there is one, from the repository's Releases page
   or with `gh release delete v<version> --repo axross/runners`.
2. Delete the tag and the note:

   ```bash
   git push origin --delete refs/tags/v<version> refs/notes/semantic-release-v<version>
   ```

3. Dispatch `release.yaml` again.

A published release is immutable, so this applies only to one that was never
published.

`v0.1.1` is not a partial release. It is a deliberate base tag with no release, so
do not delete it. [Version Rules](./versions.md#version-rules) says what it changes and why it stays.
