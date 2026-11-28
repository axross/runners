---
type: Convention
title: "Pinning tools, actions and app dependencies"
description: "Select a reproducible input, check its identity and review the exceptions for first-party actions and app dependencies."
---

# Pinning tools, actions and app dependencies

Resolve inputs before a workflow executes them. The first-party action and app dependency rules below identify which changes require individual review.

## Pinning Convention

A third-party action, anything outside the `actions/` GitHub organization, is
pinned to the full 40-character commit SHA behind its latest release, with that
release's tag kept as a trailing comment for a human to read:

```yaml
uses: owner/action@d34db33fd34db33fd34db33fd34db33fd34db33f # v1.2.3
```

A mutable tag (`@v1`, `@v1.2.3`, `@main`) can be repointed by whoever controls it.
A compromised maintainer account or release pipeline moves the tag, and every
workflow that trusts it runs the new code on its next dispatch without anyone here
changing a line. A commit SHA cannot be repointed: it names one immutable tree.

The SHA MUST be resolved from a release tag with
`git ls-remote URL 'refs/tags/vX.Y.Z^{}'`, never guessed or copied from somewhere
other than that resolution. The `^{}` peel matters because a release tag may be an
annotated tag, for which `git ls-remote` returns the tag _object's_ SHA unless the
peel is applied, a value `uses:` will not resolve as a commit. Of the two
third-party actions pinned today, `anthropics/claude-code-action` uses annotated
tags (its `v1.0.237` names a tag object that peels to the pinned commit) and
`jdx/mise-action` uses lightweight tags, where the peeled and unpeeled refs give
the same SHA. That is a fact about those repositories today, not a property to
rely on, so the peel is always the right thing to write. A SHA that was not
resolved this way MUST NOT be written: a wrong one fails at the run that first
uses it, not at review time.

The same standard applies to anything else a workflow or script fetches and
executes: a `docker://` reference, a reusable workflow, and a downloaded binary
are pinned to an immutable identifier, and a download is verified against a
checksum before it runs. The runner image's base is pinned the same way, to a
version and the digest of that version's image index, resolved from the registry
and never guessed; Dependabot's `docker` entry proposes its refreshes.
[`.agents/setup`](../../.agents/setup) verifies its mise download against a
checksum, and [`mise.toml`](../../mise.toml) pins every tool to an exact version.
[`mise.lock`](../../mise.lock), with the npm dependency locks it references under
`.mise/locks/`, records each tool's download URL and checksum for `linux-x64`;
`locked = true` is deliberately not set, so a platform the lockfile does not cover
still installs. Dependabot's `github-actions` entry refreshes action pins monthly;
the versions in `mise.toml` are refreshed by hand, looked up with `mise latest`,
never recalled. PSScriptAnalyzer is not a mise tool: `lint:powershell` downloads
its package from the PowerShell Gallery and refuses it unless its SHA-256 matches
`PSSCRIPTANALYZER_SHA256` in `mise.toml`, so the version and that hash are
refreshed together, with the hash computed from the real download. A manual
refresh of a mise tool version is: edit `mise.toml`, run
`mise lock --platform linux-x64`, and commit `mise.toml`, `mise.lock`, and
`.mise/locks/` together. When the tool is Node, also set the two `node-version`
pins in [`release.yaml`](../../.github/workflows/release.yaml) to the same version,
since Dependabot does not bump them.

The documentation check verifies its OKF validator against a pinned commit and SHA-256. Its JavaScript parsers come from the installed skill’s [npm lock](../../.claude/skills/living-project-documentation/scripts/package-lock.json). The [PyYAML requirements](../../.github/scripts/okf-requirements.txt) pin the release and accepted wheel hashes. The [checkout setup](../operations/development-setup.md#documentation-checks) describes the local cache.

The CLI under [`cli/`](../../cli) is pinned the same way. Its Rust toolchain is
one exact stable release in
[`cli/rust-toolchain.toml`](../../cli/rust-toolchain.toml), and
[`cli/Cargo.lock`](../../cli/Cargo.lock) is committed and used with `--locked`
everywhere, so every build resolves the crates the lock records and Cargo checks
each against the lock's checksum. `cargo-deny` is a mise tool, pinned in
`mise.toml` and locked in `mise.lock` like the others, and its
[`cli/deny.toml`](../../cli/deny.toml) allows crates.io only, a reviewed licence
allowlist, and one version of each crate. `serde_derive` and the crates it uses
(`syn`, `quote`, `proc-macro2`) run code at build time, as do `tokio-macros`,
`displaydoc`, `yoke-derive`, `zerofrom-derive`, `zerovec-derive`, `doctest-file`,
`thiserror-impl`, `windows-implement`, and `windows-interface`, which the async
runtime, the HTTPS client's URL parser, the Windows pipe crate, and the Windows Job
Object crate bring. They are locked by `Cargo.lock` checksums and covered by
`cargo-deny` like the other crates, and a new procedural-macro crate needs the
maintainer's review, as does a second version of a crate beyond the two that
`deny.toml` lists. The TOML configuration reader and its write path add `toml_edit`
and the crates it uses, `toml_parser`, `toml_writer`, `toml_datetime`, `winnow`,
`indexmap`, `hashbrown`, and `equivalent`. All come from crates.io under `MIT` or
`Apache-2.0`, so the allowlist does not change, and none runs code at build time. The
HTTPS client's TLS
stack also needs the `ISC` and `BSD-3-Clause` licences on the allowlist, and the
Windows pipe crate needs `0BSD`; the maintainer approved all three, and [CLI](../marshal-cli/dependencies.md#dependencies) records why
each crate is there. `rustup` itself is
installed by [`.agents/install-rust`](../../.agents/install-rust) from a pinned
`rustup-init` release for `x86_64-unknown-linux-gnu`, refused unless its SHA-256
matches. The release and its hash are refreshed by hand together, the hash
computed from the real download, and are not covered by Dependabot. Dependabot
does cover the CLI's own dependencies: its `cargo` and `rust-toolchain` entries
for `/cli` propose the crate and toolchain refreshes as pull requests.

The app under [`app/`](../../app) is pinned the same way, with its own
[`app/Cargo.lock`](../../app/Cargo.lock), used with `--locked` everywhere, its own
[`app/deny.toml`](../../app/deny.toml), and
[`app/rust-toolchain.toml`](../../app/rust-toolchain.toml), which names the CLI's
channel; `mise run check:app-pins` fails when the two files differ, and Dependabot's
`rust-toolchain` entry bumps both in one pull request. `mise run check:app-lock`
fails when the app's lockfile resolves a crate `marshal-cli` builds with to another
version than the CLI's. Its `cargo` entry covers the
app's crates, and the weekly advisory check covers its lockfile. [The App's
Dependencies](#the-apps-dependencies) holds the rest of its policy.
`cargo-about`, which writes the app's licence notices, is a mise tool like
`cargo-deny`: pinned in `mise.toml` and installed as the project's prebuilt
release, with its download URL and checksum in `mise.lock`, so it needs no compile
in CI and no exception here. [App](../marshal/notices.md#licence-notices) describes how
the notices check holds that version.

The Linux x64 editor rendering gate adds only the test controller `playwright-core`
under `app/crates/app/tests/render/`, not a dependency of the bundled page or the
app's Rust workspaces. Its npm lock records the package integrity; preparation
uses `npm ci --ignore-scripts`. `browser.json` in that directory pins the Chromium
archive's version, HTTPS URL and SHA-256, computed from the real download.
Preparation MUST verify that checksum before extracting or running the browser.
Refresh the controller, browser pin and lock together, then rerun the render gate.
To regenerate the lock after changing the manifest, use
`npm install --package-lock-only --ignore-scripts --prefix app/crates/app/tests/render`.
OS libraries are prepared only on the disposable GitHub-hosted Check runner;
the worktree preparation task installs no system package and the gate downloads
nothing. The fake bridge has no credential, live agent or configuration-file access.

The Windows installer's tool is pinned the same way. Besides the Rust toolchain and
`tauri-cli`, whose exceptions are recorded below, WiX is the one tool the release
build fetches.
[`fetch-wix.sh`](../../.github/scripts/fetch-wix.sh) downloads the NuGet package
`wix` 7.0.1 from
`https://api.nuget.org/v3-flatcontainer/wix/7.0.1/wix.7.0.1.nupkg`, refuses it
unless its SHA-256 is
`25209a1f73b20106621f11abe5e1a4e87c1d9f5c596795d246de7bb64446df17`, and only then
installs it with `dotnet tool install --tool-path`. `dotnet` reads a NuGet.Config
that clears every source and lists one folder holding the checked package, so no
other source is consulted, and the package has no dependencies. `mise run
test:fetch-wix` feeds the script a package with another SHA-256 and shows the
refusal. No WiX extension is used, so nothing else is downloaded.

WiX 7 is released under the Open Source Maintenance Fee (OSMF) EULA, and
`wix build` refuses to run without `-acceptEula wix7`, which
[`build-msi.ps1`](../../.github/scripts/build-msi.ps1) passes. The EULA asks a
maintenance fee of users with revenue-generating use whose annual gross revenue
reaches its threshold. The maintainer accepted the EULA for this project's use and
assesses that fee themself, in approving the installers' plan
([#71](https://github.com/axross/runners/issues/71)). Dependabot does not cover
WiX. To bump it:

1. Look up the latest WiX release, never recall it.
2. Download `wix.<version>.nupkg` from the URL above with the new version, and
   compute its SHA-256 from that download.
3. Change `version` and `sha256` in `fetch-wix.sh` together, and run
   `mise run test:fetch-wix`.
4. If the major version changed, read the new EULA, have the maintainer accept it
   again, and change the number in `-acceptEula wix7`.

The change needs the maintainer's review, since it changes the supply-chain
surface (see [REVIEW.md](../../REVIEW.md)).

The temporary CLI signing job uses the existing certificate pin, which also
remains the contract for M2 and W4 after app restoration. Apple's Developer ID
G2 intermediate certificate is embedded in `sign-macos` as base64, because the runner
image's trust in it is unknown and the job fetches nothing. The job refuses it
unless its SHA-256 is
`f16cd3c54c7f83cea4bf1a3e6a0819c8aaa8e4a1528fd144715f350643d2df3a`, and that check
comes before the first `security` call. Its source is
`https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer`, fetched on
2026-11-03 PST, and it expires on 2031-09-17. To replace it, fetch the certificate
again from that URL, compute its SHA-256 from the download, and change the embedded
text and the pin together, with the maintainer's review. The job downloads only the
run's own artifacts.

The Rust toolchain is one of three downloads in this repository that are not
checksum-pinned, and the maintainer accepted that exception when approving the
CLI's plan. `rustup` fetches a channel manifest over HTTPS and checks each
component's hash against that manifest, but it does not verify the manifest's
signature, so the trust rests on the HTTPS connection to the Rust project's
distribution server. Recording the manifest's hash here would need a refresh for
every release without adding a signature check. A step that installs the toolchain
`cli/rust-toolchain.toml` or `app/rust-toolchain.toml` names through `rustup` is this
recorded exception.

The second is the Node that `actions/setup-node` installs in `release.yaml`, at the
exact version in `mise.toml`. It takes Node from the runner's tool cache or from
GitHub's own builds, listed in the `actions/node-versions` manifest, and this
repository pins no checksum for it. That path is trusted under the joint-trust
decision in [`actions/*` Stays on Major Tags](#actions-stays-on-major-tags): GitHub
owns the action, the builds, and the runner that runs them. The action receives the
job's token by default, to fetch that manifest. When the manifest lacks the version
or cannot be fetched, `actions/setup-node@v7` falls back to downloading Node from
nodejs.org, again with no checksum. That path is outside the joint-trust argument,
since the Node.js project is a separate party, and it is accepted as a residual
risk in the [release record](./release-authority.md).

The third is `tauri-cli`, which builds the app, and the maintainer accepted it when
approving the app's plan ([#73](https://github.com/axross/runners/issues/73)). It is installed from source, not
downloaded as a binary: `cargo install tauri-cli --version "=<pin>" --locked`, with
`<pin>` read from [`app/tauri-cli.version`](../../app/tauri-cli.version), the one
place the version is set, through
[`tauri-cli-version.sh`](../../.github/scripts/tauri-cli-version.sh), which refuses
a file that is anything but one exact `X.Y.Z`. `--locked` makes Cargo build the
tool's own published `Cargo.lock`, and Cargo checks the crate archive and each
locked dependency against the checksums in the crates.io index, so the trust rests
on that index over HTTPS, as for any crate. These installs record no hash of the
tool's own archive, so they are not checksum-pinned like the tools above. The update
signer is the exception: W4's `update-tool` job, staged in
[`.github/staged/release-feed.yaml`](../../.github/staged/release-feed.yaml) until the
release run takes it, builds it from the archive whose SHA-256
[`app/tauri-cli.crate.sha256`](../../app/tauri-cli.crate.sha256) commits, refuses any
other, builds with `--locked`, and runs the pinned `cargo-deny` advisories check on the
archive's own lockfile with the pinned `jdx/mise-action`, `cache: false` and only
`cargo-deny` selected. The checksum moves with the pin, and a check in `mise run check`
fails when their versions differ. The
`cargo install` goes to `$RUNNER_TEMP`, uses no cache, and so compiles on every run: a cache
would be a trust boundary for a job that builds code and runs the tool. It runs in
the Windows and macOS app jobs of `merge-checks.yaml` and, since W1's installer pull
request, in the Windows leg of [the retained app build](../../.github/workflows/release-build.yaml).
Each holds `contents: read`, reads no secret and publishes nothing, and each installs
the tool the same way, at the same pin read through the same script, under
`$RUNNER_TEMP` with no cache. The macOS job, which M2 adds, signs nothing and keeps
the unsigned Mac app and disk image only as a workflow artifact. `tauri-cli`'s build
scripts and procedural macros run code at build time under those jobs' accounts,
which is why the jobs bound the exception. The app build still runs for merge
checks. During [CLI recovery](./release-authority.md#temporary-cli-recovery), the
release workflow publishes only artifacts from the historical CLI build. The
[app publication baseline](./release-authority.md#retained-app-release-baseline-suspended)
records how the tool’s output was attested and published before recovery. Nothing else installs the tool. To move the pin, see
[App](../marshal/building.md#the-tauri-cli-pin); the change needs the maintainer's
review, since it changes the supply-chain surface (see
[REVIEW.md](../../REVIEW.md)). Any other unpinned download remains a finding.

## `actions/*` Stays on Major Tags

GitHub's own `actions/` organization is excluded from the [SHA-pinning rule](./dependency-pins.md#pinning-convention)
and kept on a plain major-version tag (`actions/checkout@v7`), across every
workflow in this repository. This is a recorded decision, not an oversight.

The reasoning is a joint-trust argument: GitHub itself owns both the `actions/`
organization and the GitHub-hosted runner that executes it, so trusting
`actions/*`'s own tag is trusting the same party this project already trusts to
run the workflow at all, a materially different position from trusting an
independent third-party maintainer's tag. Every job here runs on a GitHub-hosted
runner, so the argument holds for all of them.

The argument does not hold for a job that runs on a machine the maintainer owns.
If a workflow ever dispatches onto a self-hosted runner, this decision has to be
revisited for that workflow on its own terms; see
[Self-Hosted Runners Stay Out of This Repository's Own Jobs](./workflow-permissions.md#self-hosted-runners-stay-out-of-this-repositorys-own-jobs).
A review finding this file's `actions/*` references unpinned is seeing this
decision working as intended, not a gap.

## The App's Dependencies

The app's workspace under [`app/`](../../app) links the `marshal-cli` library and the
Tauri framework, so its tree is far larger than the CLI's, and its policy differs from
the CLI's where that size makes the CLI's rule unworkable. The pinning of the lockfile,
the toolchain and the build tools is in [Pinning Convention](#pinning-convention).
[`app/deny.toml`](../../app/deny.toml) is the app's `cargo-deny` configuration, and
`mise run deny:app` runs its bans, licences and sources checks for the two targets the
app is built for, `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`.

**App crate.**

- **Registry and licences.** crates.io only, and the CLI's licence allowlist: `MIT`,
  `Apache-2.0`, `Unicode-3.0`, `ISC`, `BSD-3-Clause` and `0BSD`, an `OR` expression
  accepted when one alternative is on the list. Two more licences are accepted one
  crate at a time, each as its own exception in `deny.toml`, so a new crate under
  either fails the check: `MPL-2.0` for `cssparser`, `cssparser-macros`,
  `dtoa-short`, `selectors` and `option-ext`, and `Zlib` for `foldhash`. `MPL-2.0` is
  copyleft at the level of the file, and the crates are linked unmodified, so the
  obligation is to keep the licence text and say where the source is published, which
  the licence notices do. The exceptions match what `cargo deny` reports, and it
  reports an exception that no crate uses. Any other crate under another licence
  returns to the maintainer.
- **Duplicates are reported, not denied.** `deny.toml` sets `multiple-versions` to
  `warn`, because Tauri's tree carries many duplicate versions and each would need a
  skip that the next Tauri release changes. This is the app only: the CLI's
  duplicates stay denied beyond its two listed skips. Wildcard version requirements
  are denied except for the path dependencies the app's crates and `marshal-cli` have
  on each other.
- **Procedural macros are accepted as a class, in the app only.** Tauri's tree holds
  many, and a review of each crate would add nothing the lockfile's checksums and
  `cargo-deny` do not already give. `app/Cargo.lock` resolves every crate
  `cli/Cargo.lock` resolves for `marshal-cli`, for Windows, macOS and Linux, to the same
  version, with nothing more among them, and `mise run check` fails otherwise
  (`check:app-lock`), so a procedural-macro crate that `marshal-cli`'s own dependencies
  bring changes `cli/Cargo.lock`, and the CLI's rule, which needs the maintainer's
  review, applies to it there. A crate that a feature the app turns on adds to a crate
  both trees share is the app's own and falls under this class rule: features the
  app's other dependencies turn on in `futures-util`, `reqwest` and `tracing` add `futures-macro`,
  `tracing-attributes`, `tokio-util`, `futures-io`, `futures-sink` and `syn` 2 that way.
- **Marshal's own crates forbid `unsafe_code`.** Tauri's macros expand into the app's
  crates under that lint, and they build, so the macros emit none; Tauri's and the
  platform crates' own code is outside the claim.
- **Features are named.** Tauri's default features are off and the manifest lists each
  kept one with its reason: `wry` for the web view, `tray-icon` for the icon and menu
  without `image-png` or `image-ico`, so the app links no image decoder, and
  `common-controls-v6` for the Windows menu. `compression`, `dynamic-acl`, `x11` and
  `dbus` are left out, which drops brotli, the D-Bus and X11 crates from the tree and
  means no capability can be added while the app runs. A new feature, or one of
  those four, is a change to this list.
- **Direct dependencies.** `tauri` 2.12, `tauri-build` 2.7 and
  `tauri-plugin-single-instance` 2.5 for the app, `tauri-plugin-opener` 2.7.2 to open a
  running job's page in the default browser from the Rust side (on Windows through the
  `open` crate's `ShellExecuteExW`, so the address never passes through another
  program's command line; it adds `open`, its build helper `tauri-plugin`, and on Linux
  `is-wsl` and `is-docker`, and needs no licence exception), and `embed-resource` 3.0 to build
  it, the resource compiler `tauri-build` already builds with, which `build.rs` calls
  to link the common controls manifest into the integration tests, so it adds no crate
  to the tree and runs only when the target is Windows with the MSVC environment;
  `serde`, `serde_json` and `tokio` for the session and the view, at versions the
  library already builds, so they add no crate; `windows-registry` 0.6 on Windows, which reads the taskbar's mode and the
  time zone's bias without `unsafe` code; `core-foundation` 0.10 on macOS, Core
  Foundation's safe wrapper, which reads the time zone's offset, at the version
  Tauri's window crate already links there, so it adds no crate; and `keepawake` 0.6 on Windows, without its
  default features, which asks Windows not to idle-sleep while the agent's runners run
  and which the library's policy does not admit, so the executable gives the request to
  the agent. Its `derive_builder` macros bring a second version of the `darling`
  crates beside the one the tree already has. Its development-only dependencies are
  Tauri's `test` feature, and on Windows `interprocess`, which a test uses to squat on
  the agent's channel. For updates (see [Updates Come Only From the Signed
  Feed](./app-trust.md#updates-come-only-from-the-signed-feed)): `tauri-plugin-updater`, pinned to
  `=2.14.1`, on Windows only, with its defaults off and only `rustls-tls` on, so it
  links no zip reader and reads no system proxy; `winsafe` 0.0.29's `kernel` feature on
  Windows, the crate the library already uses, to wait on the agent's process without
  `unsafe` code; `tokio`'s `time` feature on Windows for that wait; and `serde_json`,
  at the version the session builds, which the code `generate_context!` writes for the
  updater's configuration needs. The
  `marshal-feed` crate takes `minisign-verify`, `base64`, `semver`, `url` and `sha2`,
  each at the version the updater plugin or the library already links on Windows, so
  that its feed check verifies exactly as the plugin does, and `serde` and
  `serde_json`, at the versions the session builds, to read the feed and the app's
  configuration; on macOS they add
  `minisign-verify` to the tree. The token store's crates come through the library, and
  [CLI](../marshal-cli/dependencies.md#dependencies) lists them. A new direct dependency of an
  app crate MUST be added here with its reason, and needs the maintainer's review.
- **No crate sends telemetry or a crash report.** Marshal sends none. The app's only
  network requests are the update check and the update's download, both over HTTPS
  to `github.com`; a dependency or a feature that makes any other is a finding.
- **Refreshes.** Dependabot's `cargo` entry for `/app` proposes the crate refreshes,
  and a red pull request is fixed there, as for the CLI. A refresh of a crate
  `marshal-cli` uses fails `check:app-lock` until `cli/Cargo.lock` takes the same
  version in the same change. The licence notices must be written again whenever the
  lockfile changes; see [App](../marshal/notices.md#licence-notices).

**Front end.** The web view's pages are plain files in
[`app/crates/app/frontend/`](../../app/crates/app/frontend):

- **No npm package.** There is no `package.json`, no lockfile, no bundler and no
  build step. The pages use the global API Tauri's configuration turns on, so
  nothing is installed from npm and nothing needs refreshing; Dependabot has no
  entry for it.
- **No remote asset.** Nothing is downloaded at run time: no font service, no
  script from a CDN, no image from a server. The [app content security policy](./app-trust.md#the-app-surface)
  would refuse it too.
- **The font is a recorded copy.** IBM Plex Sans is bundled as three WOFF2 files
  under the SIL Open Font License 1.1.
  [`fonts/README.md`](../../app/crates/app/frontend/fonts/README.md) records the
  release, its source, the archive's SHA-256 and each file's, and the files are kept
  byte for byte by `.gitattributes`. A change to a font file changes that record and
  the licence notices in the same commit.
- **The colour values are a recorded copy.** The values in `tokens.css` are copied
  from Radix Colors 3.0.0, licensed MIT; the file's header names the package and the
  roles, and the package's notice is `colors-LICENSE.txt` beside it. They are
  copied, not imported, so the package is not a dependency, and a change to them
  changes the header and the licence notices in the same commit.
