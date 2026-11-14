# Security

This project's supply-chain and runner-trust conventions: how a `uses:` reference
under [`.github/`](../../.github) is pinned, what a workflow may be granted, how a
release of the CLI is published and its
macOS files are signed, where
a self-hosted runner may and may not run, what a runner's shared storage exposes,
what the Windows app's web view and process may do, and what is never committed. It
does not cover general application-level security, such as input validation or the
OWASP-style concerns the installed `application-security` capability owns.

The runner image under [`images/actions-runner/`](../../images/actions-runner) and
the host configuration under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) exist and are
reviewed against the sections on runner trust and shared storage below, and the
macOS host's Linux VM runtime ([#74](https://github.com/axross/runners/issues/74))
against the sections on a per-job VM, the VM-local Docker socket, Softnet and the
pinned programs. The
agent-host configuration this repository is also for does not exist yet; those
sections are the constraints it will be reviewed against.

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
Windows pipe crate needs `0BSD`; the maintainer approved all three, and [CLI](../operations/cli.md#dependencies) records why
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
in CI and no exception here. [App](../operations/app.md#licence-notices) describes how
the notices check holds that version.

The Windows installer's tool is pinned the same way. Besides the Rust toolchain,
whose exception is recorded below, WiX is the one tool the release build fetches.
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

The macOS signing job pins the one certificate it embeds. Apple's Developer ID G2
intermediate certificate is embedded in `sign-macos` as base64, because the runner
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
risk of the release workflow, below.

The third is `tauri-cli`, which builds the app, and the maintainer accepted it when
approving the app's plan ([#73](https://github.com/axross/runners/issues/73)). It is installed from source, not
downloaded as a binary: `cargo install tauri-cli --version "=<pin>" --locked`, with
`<pin>` read from [`app/tauri-cli.version`](../../app/tauri-cli.version), the one
place the version is set, through
[`tauri-cli-version.sh`](../../.github/scripts/tauri-cli-version.sh), which refuses
a file that is anything but one exact `X.Y.Z`. `--locked` makes Cargo build the
tool's own published `Cargo.lock`, and Cargo checks the crate archive and each
locked dependency against the checksums in the crates.io index, so the trust rests
on that index over HTTPS, as for any crate. What this repository does not record is
a hash of the tool's own archive, so it is not checksum-pinned like the tools above;
W4's tool job, which signs update archives, commits one that moves with the pin once
it lands. The
install goes to `$RUNNER_TEMP`, uses no cache, and so compiles on every run: a cache
would be a trust boundary for a job that builds code and runs the tool. It runs in
the Windows job of `merge-checks.yaml`, which holds `contents: read`, reads no
secret and publishes nothing. `tauri-cli`'s build scripts and procedural macros run
code at build time under that job's account, which is why the job is what bounds
them. The release build that installs the app is to follow the same rule and read
the same pin; until it exists, nothing else installs the tool. To move the pin, see
[App](../operations/app.md#the-tauri-cli-pin); the change needs the maintainer's
review, since it changes the supply-chain surface (see
[REVIEW.md](../../REVIEW.md)). Any other unpinned download remains a finding.

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
  time zone's bias without `unsafe` code; and `keepawake` 0.6 on Windows, without its
  default features, which asks Windows not to idle-sleep while the agent's runners run
  and which the library's policy does not admit, so the executable gives the request to
  the agent. Its `derive_builder` macros bring a second version of the `darling`
  crates beside the one the tree already has. Its development-only dependencies are
  Tauri's `test` feature, and on Windows `interprocess`, which a test uses to squat on
  the agent's channel. On macOS, `smappservice-rs` 0.1.3 (MIT), which registers
  the app's own bundle as its login item through Apple's `SMAppService` main-app item
  (M4, [#90](https://github.com/axross/runners/issues/90)); it wraps
  `objc2-service-management` 0.3, whose calls are `unsafe`, behind a safe interface,
  so the app's own code stays without `unsafe`, and it adds that crate and
  `objc2-security` 0.3, which are MIT, Zlib or Apache-2.0. It adds no helper, launch
  agent or property list to the bundle. For updates (see [Updates Come Only From the Signed
  Feed](#updates-come-only-from-the-signed-feed)): `tauri-plugin-updater`, pinned to
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
  [CLI](../operations/cli.md#dependencies) lists them. A new direct dependency of an
  app crate MUST be added here with its reason, and needs the maintainer's review.
- **No crate sends telemetry or a crash report.** Marshal sends none. The app's only
  network requests are the update check and the update's download, both over HTTPS
  to `github.com`; a dependency or a feature that makes any other is a finding.
- **Refreshes.** Dependabot's `cargo` entry for `/app` proposes the crate refreshes,
  and a red pull request is fixed there, as for the CLI. A refresh of a crate
  `marshal-cli` uses fails `check:app-lock` until `cli/Cargo.lock` takes the same
  version in the same change. The licence notices must be written again whenever the
  lockfile changes; see [App](../operations/app.md#licence-notices).

**Front end.** The web view's pages are plain files in
[`app/crates/app/frontend/`](../../app/crates/app/frontend):

- **No npm package.** There is no `package.json`, no lockfile, no bundler and no
  build step. The pages use the global API Tauri's configuration turns on, so
  nothing is installed from npm and nothing needs refreshing; Dependabot has no
  entry for it.
- **No remote asset.** Nothing is downloaded at run time: no font service, no
  script from a CDN, no image from a server. The content security policy below
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

## `actions/*` Stays on Major Tags

GitHub's own `actions/` organization is excluded from the SHA-pinning rule above
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
[Self-Hosted Runners Stay Out of This Repository's Own Jobs](#self-hosted-runners-stay-out-of-this-repositorys-own-jobs).
A review finding this file's `actions/*` references unpinned is seeing this
decision working as intended, not a gap.

## Workflow Permissions Start From Read-Only

Every workflow declares a top-level `permissions:` block that starts from
`contents: read`. A broader scope is granted on the job that needs it, and never
on a job that runs pull-request-controlled code. `actions/checkout` sets
`persist-credentials: false` wherever the job does not itself push or call the
API with the checkout's credential. A workflow triggered by `issue_comment` checks
out the default branch, never the pull request's head, so no code a commenter
could have introduced runs in a job that holds a secret.
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml),
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml), and
[`release-build.yaml`](../../.github/workflows/release-build.yaml) hold only
`contents: read`. [`release.yaml`](../../.github/workflows/release.yaml) grants write
scopes to its plan, attest, and release jobs; the release record below says which
and why. [`claude-review.yaml`](../../.github/workflows/claude-review.yaml)
holds `contents: read` at the top and, on its one job, `contents: read` plus
`id-token: write`. It needs no pull request, issue, or check write scope: at the
pinned version the action exchanges the job's OpenID Connect token for a Claude
GitHub App token and uses that token for every write to the pull request, and
the job token's only other use is an optional CI-status server that stays off
unless `actions: read` is granted and requested. Do not add a write scope to that
job without re-reading the action at the pinned SHA and recording why.

An untrusted context value (a comment body, a pull request title, a branch name)
is passed into a step through an `env:` variable and quoted, never expanded inside
`run:`. The workflow `if:` expression is not a substitute for that.

## A Release Is Published Only From a Dispatch on Main

[`release.yaml`](../../.github/workflows/release.yaml) releases the CLI with
semantic-release and signs the macOS files in the `release` Environment, and
[`release-build.yaml`](../../.github/workflows/release-build.yaml) builds the binaries
and their installers; [CLI](../operations/cli.md#releases) owns the steps. This is the
record a change to either workflow, to the build call in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml), to
[`release/`](../../release), or to the scripts under
[`.github/scripts/`](../../.github/scripts) is checked against:

- **A manual dispatch is the only trigger.** `release.yaml` runs on
  `workflow_dispatch` and on nothing else: no push, tag, pull request, schedule, or
  `workflow_run` trigger. The dispatch is the maintainer's decision to release.
  `sign-macos` then waits for the maintainer's approval in the `release`
  Environment, which follows the commit check in
  [Each Release](../operations/development-workflow.md#each-release). No other job
  declares an Environment or waits for an approval.
- **The branch guard is advisory.** Every job in `release.yaml` has
  `if: github.ref == 'refs/heads/main'`. A dispatch on another branch runs that
  branch's own copy of the file, which could drop the guard, so it binds only the
  copy on main. The controls are the optional check's `--source-ref refs/heads/main`
  and its compare-API check that the commit is on main, for an operator who runs it.
- **The build is split from the release.** The build is a reusable workflow that
  `merge-checks.yaml` calls on every pull request as the dry run, with the version
  `0.0.0-dryrun.<run number>`, and `release.yaml` calls with the version its plan
  job worked out. On `pull_request` the workflow file comes from the pull request
  itself, so a job-level `if:` or `environment:` in a workflow a pull request
  triggers is not a control: the pull request could delete it. Every job with a
  write scope therefore sits in a workflow no pull request triggers. One release
  workflow with both a pull request and a release trigger, guarded by an `if:`, was
  rejected for that reason.
- **No job holds both kinds of write.** `release.yaml` starts from
  `contents: read`, and each job's `permissions` replaces that with exactly this:
  - the plan job holds `contents: write`, for the `git push --dry-run` that
    semantic-release's dry run makes, and runs the npm tree;
  - the build job holds `contents: read`;
  - `sign-macos` holds `contents: read` and declares the `release` Environment, and
    `verify-macos` and `assemble-signed` hold `contents: read` alone and declare no
    Environment. None of the three runs the npm tree, and the signing record below
    says what `sign-macos` may do;
  - the attest job holds `id-token: write` and `attestations: write`, and nothing
    else. It checks nothing out and installs no npm package: it downloads the
    signed set from `assemble-signed`, checks it, and runs `actions/attest`;
  - the release job holds `contents: write`, to push the tag and its note and to
    create the release, and runs the npm tree.

  A job that runs the npm tree can push tags and edit releases, and its token can
  fast-forward main, but it cannot mint an attestation, so a file it substitutes
  fails `gh attestation verify` for an operator who runs the optional check before
  opening it. An operator who skips the check opens the substituted file, as the
  residual risks below say. A change it pushes to main is another matter, as they
  also say. One release job with all three scopes
  was rejected: any package in the npm tree could then attest any binary as
  `release.yaml` on `refs/heads/main`. No job needs `artifact-metadata: write`,
  which is for storage records in a registry. `@semantic-release/github`'s comments
  and labels are off, so the release job needs no issue or pull request scope.
  Every job in `release-build.yaml` and the dry-run call in `merge-checks.yaml` hold
  `contents: read` alone, and neither has a publish or attest step.

- **The files are checked twice before they are used.** The attest job, and the
  release gate [`verify-release.sh`](../../.github/scripts/verify-release.sh) that
  semantic-release runs before it tags anything, each refuse unless the files are
  exactly the two binaries and the two installers named for the version and
  `SHA256SUMS`, `SHA256SUMS` lists exactly those four, and
  `sha256sum --check --strict` passes. The attest job attests the four files
  `SHA256SUMS` lists, after signing. The installers are an MSI and a flat package
  that the build legs pack the release binary into unchanged and test; the MSI is
  not signed, the package is signed and notarized by `sign-macos`, and neither
  carries a script or a custom action that runs code. One script,
  [`assemble-release.sh`](../../.github/scripts/assemble-release.sh), writes every
  `SHA256SUMS`. The gate checks against the version semantic-release worked out in
  its own run.
  `@semantic-release/github` skips an asset that is missing rather than failing, so
  the gate is what holds the file set.
- **Signing happens in one job, in one Environment.** `sign-macos` is the only job
  that declares an Environment or reads a signing secret;
  [CLI](../operations/cli.md#signing-the-macos-files) gives its steps and the
  contracts. The record it is checked against:
  - **The Environment `release`** is created and maintained by the maintainer.
    Deployment branches are selected branches, `main` only and no tags. Required
    reviewers are the maintainer, with "Prevent self-review" off, because the
    maintainer is the only reviewer and
    [the roadmap](https://github.com/axross/runners/issues/72) requires
    self-approval. "Allow administrators to bypass" is off. The maintainer reports
    the settings on the tracking issue, because agents cannot read them.
  - **Its secrets** are the six Environment secrets `MACOS_APPLICATION_P12_BASE64`,
    `MACOS_INSTALLER_P12_BASE64`, `MACOS_P12_PASSWORD`, `MACOS_NOTARY_KEY_P8`,
    `MACOS_NOTARY_KEY_ID` and `MACOS_NOTARY_ISSUER_ID`, and its variable is
    `MACOS_TEAM_ID`, which is not secret. A job reads them only if it declares the
    Environment, and only after the reviewer approves it. No repository secret is
    added for signing.
  - **`sign-macos` holds `contents: read`**, checks nothing out, runs no repository
    file and no package manager, and uses no action but `actions/download-artifact`
    and `actions/upload-artifact`. It fetches no tool or certificate. Each signing
    tool it calls is called by its absolute system path, so nothing earlier on
    `PATH` can stand in for one; its other commands are shell built-ins and macOS
    base utilities, such as `base64` and `mktemp`, called by name. The Developer ID
    G2 intermediate it imports is embedded and checked against the pin under
    [Pinning Convention](#pinning-convention). Every context value reaches its
    scripts through `env:`. The job's inline steps are not repository code: they are
    the job's own definition in `release.yaml`, which the maintainer's approval
    covers, and the maintainer confirmed that reading at the plan gate.
  - **Only `sign-macos` reads the signing secrets.** `verify-macos` runs a
    repository script on a runner that holds no signing identity and no secret, and
    `assemble-signed` and every other job hold none either. That script also checks
    that the unsigned files are the ones the build's `SHA256SUMS` lists and that the
    notary log is of the submission `sign-macos` made.
  - **The signing identifier** `app.axross.marshal` and the team are fixed and
    checked at each release, so the designated requirement of the code object that
    carries it stays the same. Changing either breaks the Keychain trust that later
    work relies on and goes through a roadmap revision.
  - **A signing failure stops the release before any tag.** Every signing step
    comes before `release`, no job continues past a failure, and the notarization
    result is read from the status, not the exit code.
  - **Pull requests never sign or reach the Environment.** The dry run builds
    unsigned, and no workflow a pull request triggers declares an Environment.
    [`check-release-shape.mjs`](../../.github/scripts/check-release-shape.mjs) in
    `mise run check` fails if the signing jobs drift from this record, including if
    any job in any workflow but `sign-macos` in `release.yaml` reads a signing
    secret.
    [CLI](../operations/cli.md#what-the-signing-tests-cover) lists its rules. A pull
    request can edit it, so it guards against mistakes, not against a hostile pull
    request, and [CLI](../operations/cli.md#extending-the-release-workflows) names
    what it does not read. What keeps pull requests away is the branch rule on the
    Environment and that `release.yaml` runs only on a dispatch.
  - **No secret is printed.** Secret values reach the scripts through `env:` and
    files written with mode 0600 under the runner's temporary directory, and each
    line of the `.p8` is masked. The keychain and those files are deleted whether the
    job passes or fails, and the notary log, which holds no secret, is uploaded.
    The certificates' subjects name the maintainer's Apple developer account and
    team, as every Developer ID signature does; nothing of that is committed, except
    that a later documentation change records the team ID and the exact designated
    requirement in the CLI document, as the exception under [Nothing Identifying or
    Secret Is Committed](#nothing-identifying-or-secret-is-committed) describes.
  - **Extensions** by later work add their signing jobs or steps to the shape check
    and keep these rules: only signing jobs declare the Environment, a signing job
    holds `contents: read` and runs no repository code, checksums and attestation
    follow every signing step, no pull-request run reaches the Environment, and
    F0's and M2's signing jobs use only operating-system signing tools. W4's
    update-signing job is the one exception to that last rule: it runs `tauri-cli`'s
    `signer sign`, built with `--locked` in a tool job without the Environment from a
    crate archive whose SHA-256 equals one committed in the repository. The signing
    job checks the binary it receives against the SHA-256 that the tool job outputs,
    before it runs it.
    [CLI](../operations/cli.md#extending-the-release-workflows) names the points.
- **The documented install blocks run against the dry run's installers on hosted
  runners.** The `Install Blocks` job in
  [`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) needs the dry run,
  downloads its artifact, and holds `contents: read`. It stubs `gh` and runs the
  blocks of [CLI](../operations/cli.md#installing-updating-and-rolling-back) with
  the real installers, so it installs and removes the dry run's own MSI and pkg on
  a runner that is discarded afterwards. On Windows it also creates a local user
  outside the Administrators group, runs the check block and the install as that
  user through a secondary logon with its profile loaded, and removes the user at
  the end. Nothing changes a user right. The test sets the Windows Installer
  policy `DisableMSI` to 0 for the run, because the GitHub-hosted Windows Server
  image blocks a non-elevated per-user install, and restores it at the end. The
  runner is discarded after the job either way.
  It uses no secret, attests nothing, and adds no action beyond the `actions/*`
  already used. This job and the build legs upload their installer logs, from
  `msiexec /l*v` and `installer -dumplog`, as workflow artifacts. They ask for 14
  days. The repository's artifact retention limit applies instead when it is
  shorter, and it cut the build legs' installer logs to 7 days in a pull request's
  dry run.
  The logs may hold the ephemeral GitHub-hosted runner's machine and account names,
  which the job log's "Set up job" section already shows, and they hold no secret.
- **No credential stays in a checkout.** Every checkout sets
  `persist-credentials: false`. Under GitHub Actions, semantic-release builds an
  `x-access-token:` URL from `GITHUB_TOKEN` for its own git commands.
- **No cache.** No job uses a cache action, and `actions/setup-node` is given no
  `cache` input and `package-manager-cache: false`, because a release run could
  restore a cache that another ref saved. Each build compiles from `Cargo.lock` with
  `--locked`, and `npm ci` installs from `release/package-lock.json`.
- **Context values reach a script through `env:`**, never expanded into `run:`.
- **Pins.** `actions/checkout`, `actions/setup-node`, `actions/upload-artifact`,
  `actions/download-artifact`, and `actions/attest` stay on major tags, under
  [`actions/*` Stays on Major Tags](#actions-stays-on-major-tags). Neither release
  workflow uses a third-party action, and the one tool the build fetches besides
  the recorded Rust toolchain exception is WiX, pinned under
  [Pinning Convention](#pinning-convention). `jdx/mise-action` was rejected for Node
  here: at this repository's pin it caches by default, installs every mise tool, and
  receives the job token. `actions/setup-node` installs Node `24.21.0`, the version
  in `mise.toml`, and the Node download is trusted under the same joint-trust
  decision, not by a checksum; see [Pinning Convention](#pinning-convention).
  `release/package.json` names an exact version of semantic-release and of each
  plugin and preset, `release/package-lock.json` records each package's integrity
  hash, and `npm ci --ignore-scripts` installs exactly that and runs no install
  script.
  Dependabot's `npm` entry proposes the refreshes. `release/` keeps these packages
  out of the repository root and out of mise's tool locks.
- **A release run is never cancelled.** Only the two calling workflows declare
  `concurrency`, because inside a called workflow `github.workflow` is the caller's
  name. `release.yaml`'s group is `release`, and it does not cancel in progress, so
  a run is never stopped between pushing the tag and publishing.
- **The version comes from the commits, and main keeps a placeholder.**
  semantic-release picks the version from the Conventional Commit subjects since
  the last `v*` tag. Its `releaseRules` add a patch for `build(deps)`, the prefix
  Dependabot writes for the runner image and the CLI's crates and toolchain, and
  Dependabot's `npm` entry writes `chore`, which releases nothing.
  [Cutting a Release](../operations/development-workflow.md#cutting-a-release)
  states the rules. Nothing is committed back: `cli/Cargo.toml` and `cli/Cargo.lock`
  on main hold `0.0.0-dev`, and
  [`set-release-version.sh`](../../.github/scripts/set-release-version.sh) writes
  the version in each build leg. It refuses unless both files hold the placeholder
  exactly once, `--locked` fails if they disagree, and the leg's `--version` check
  compares the binary with the version.

What protects a release is outside the workflow files:

- Only accounts with write access can dispatch a workflow or push a tag here. On
  this user-owned repository that is the maintainer, and the agents that share the
  maintainer's identity.
- Install-time verification is optional. Opening the installer is the main install
  procedure, and the check block that goes with it is recommended, not required, so
  nothing stops an operator who skips it from opening a file that nothing has
  checked. The block pins `--signer-workflow` to `release.yaml`, `--source-ref` to
  `refs/heads/main`, and `--source-digest` to the tag's commit, denies self-hosted
  runners, and refuses a commit the compare API does not place on main, a checksum
  that `SHA256SUMS` does not list, and an installer whose binary is not the
  version's. An attestation from any other workflow, branch, or commit is refused
  there; see
  [CLI](../operations/cli.md#installing-updating-and-rolling-back). The
  attestation's role is to make that check possible. The bare-binary blocks that
  roll back to `v0.1.0` keep every one of those checks as a requirement of their
  procedure.
- Immutable releases are turned on for the repository before the first dispatch,
  so a published release's assets cannot be replaced and its tag cannot be moved
  or deleted.

Immutable releases are a repository setting, not a file here;
[Cutting a Release](../operations/development-workflow.md#cutting-a-release) gives
the setting and its read-back.

An agent never dispatches `release.yaml`, never approves a deployment of it,
including the `release` Environment's approval of `sign-macos`, never pushes,
moves, or deletes a `v*` tag, and never changes release settings, such as
immutable releases or an Environment or ruleset that applies to releases. This rule
is absolute, and it is
stricter than the
[delivery grant](../operations/development-workflow.md#authorizing-delivery-operations).
The grant lets an explicit human instruction cover release, deployment, and
settings effects. These release effects have no such path: an agent never performs
one, even when the maintainer asks in the session or another agent relays it.
Agent sessions share the maintainer's GitHub identity, so to GitHub a dispatch by an
agent and one by the maintainer are the same, and so are an approval by each. With
no tag ruleset, this rule is the only control on agents.

Residual risks, accepted:

- **The npm tree runs with `contents: write`.** A compromised package in the plan
  or release job can push, move, or delete tags; create, edit, or delete draft and
  published releases; and push to any branch that no rule protects. It cannot mint
  an attestation, so a file it substitutes fails `gh attestation verify` for an
  operator who runs the optional check. An operator who skips it opens the
  substituted installer, which the next risk below describes. It can deny a
  release, or publish one whose files operators who check them then refuse.
- **The npm tree can push to main.** The ruleset on main blocks only deleting the
  branch and pushes that are not fast-forwards. The plan and release jobs'
  `GITHUB_TOKEN` can therefore fast-forward main with a source change that touches
  no workflow file, and a push made with that token starts no workflow run. The
  next dispatch would build that change and attest it as genuine. The control is
  the check in
  [Each Release](../operations/development-workflow.md#each-release), before
  each dispatch, that every commit on main since the last `v*` tag came from a
  merged pull request. A stronger option is a rule on main that requires a
  pull request, with only the maintainer able to bypass it. That is the
  maintainer's choice, and it is not in place.
- **The attest job attests whatever passes its checks.** `SHA256SUMS` comes from
  the same build artifact, so the check catches transport damage, not a compromised
  build leg.
- **No tag ruleset.** Any account with write access can dispatch a release, and
  dispatching needs only that access. That is the account the agents run as. The rule
  above is the only control on agents. The maintainer accepted this in choosing a
  manual dispatch with no ruleset.
- **A hostile change on main gets signed if it is approved.** `sign-macos` signs
  whatever the dispatched commit builds, with the maintainer's Developer ID. The
  control is the check in
  [Each Release](../operations/development-workflow.md#each-release), before each
  dispatch and before the approval, that every commit on main since the last `v*`
  tag came from a merged pull request. A stronger option is a rule on main that
  requires a pull request, which is the maintainer's choice and is not in place.
- **One identity can dispatch and approve.** The Environment allows self-approval,
  because the roadmap requires it and the maintainer is the only reviewer, and agents
  share the maintainer's GitHub identity. A session could therefore dispatch and
  approve a signed release. The rule above, which now covers approving a deployment,
  is the control, and a second reviewer is not an option here.
- **A certificate or membership lapse stops signing.** The Developer ID certificates
  are expected to last one year, which the maintainer confirms from the certificates'
  dates, and `sign-macos` refuses an expired one and warns within 90 days.
  Signatures made with a secure timestamp stay valid after the certificate expires.
  Whether an older release's package still installs after its Installer certificate
  expires is not confirmed, and Apple's pages disagree, which bears on rolling back
  through that package. A membership lapse leaves signed software working but allows
  no new certificate. [Development
  Workflow](../operations/development-workflow.md#renewing-the-certificates) records
  the dates and the renewal.
- **An early dispatch creates an empty Environment.** A dispatch before the maintainer
  creates the `release` Environment makes GitHub create one with no protection rules
  and no secrets. `sign-macos` refuses on the empty secrets before it imports
  anything, and [Before the First
  Release](../operations/development-workflow.md#before-the-first-release) creates the
  Environment first. Until the maintainer reports its settings, nothing in the
  repository shows that its branch rule and reviewer are in place.
- **The first signed release is the first run of signing.** Nothing before merge can
  exercise the Environment, the certificates or notarization, and `verify-macos`
  first runs for real there. The stand-in suites and the shape check cover the flow,
  not Apple's answers. A failure stops that release before its tag.
- **Unprotected tags change the next version.** Any account with write access can
  push, move, or delete a `v*` tag that has no published release, `v0.0.0`
  included, and that changes the version semantic-release works out next.
  Immutable releases lock only the tags of published releases.
- **A weaker install pin.** `--source-ref refs/heads/main` proves that the build
  ran from main, not from a particular tag. `--source-digest` and the reachability
  check still tie the file to one commit on main, but no longer to a version. The
  optional check therefore runs the binary inside the downloaded installer and
  refuses unless it prints the version's `--version`, so an older genuine installer
  under a newer version's name is refused by an operator who runs the check. The
  bare-binary blocks make the same check of the downloaded binary before they
  replace the installed one, and refuse without touching the install path. CI runs
  the check block's refusals against stand-ins and the dry run's installers, and the
  bare-binary blocks' refusals against stand-ins. Neither has run on axpc or the
  Mac ([#56](https://github.com/axross/runners/issues/56)); the refusal checks of the
  bare-binary block were not run on axpc.
- **A green run can release nothing.** If main moves after the dispatch, the
  release job logs that the local branch is behind the remote one and ends without
  a tag or a release, after the binaries are attested.
  [Cutting a Release](../operations/development-workflow.md#cutting-a-release)
  says to check that the release exists after a green run.
- **A partial release.** semantic-release pushes the tag and its
  `refs/notes/semantic-release-<tag>` note before `@semantic-release/github`
  creates the draft release. If the upload fails, the tag still counts as the last
  release, so the next dispatch skips that version. The clean-up is the
  maintainer's, in
  [Cutting a Release](../operations/development-workflow.md#cutting-a-release).
- **Versions follow commit subjects.** A mistitled squash merge releases the wrong
  bump, or none, and no check catches it.
- **Node can come from nodejs.org.** When the `actions/node-versions` manifest
  lacks `24.21.0` or cannot be fetched, `actions/setup-node` downloads Node from
  nodejs.org with no checksum, outside the joint-trust argument in
  [Pinning Convention](#pinning-convention). That Node runs the npm tree with
  `contents: write`.
- **Advisories in an npm that is never loaded.** `npm audit` reports advisories,
  undici (high) and ip-address (moderate), only in the npm CLI that
  `@semantic-release/npm` bundles. That plugin is installed as a hard dependency of
  semantic-release but never loaded, because `release/.releaserc.json` replaces the
  plugin list. No CI job audits `release/`; Dependabot's `npm` entry covers
  `/release` for version updates.
- **No pull request runs semantic-release.** Neither `mise run check` nor Merge
  Checks loads `release/.releaserc.json`, so a Dependabot bump in `release/` that
  breaks the plan job merges green and fails only on the next dispatch, before any
  tag or release is made.
- **An installer opened without the check runs before anything has checked it.**
  The maintainer chose opening the installer as the main install procedure and the
  check as optional. The release job's npm tree can replace published files (the
  first risk above), and an operator who skips the check opens whatever the release
  page then holds. On Windows that is a file nothing has checked. On macOS,
  Gatekeeper is expected to assess a package that carries the download mark, but it
  is not expected to tie the package to the maintainer's team, as the next entry
  says. Signing the MSI, or making the check required again, is the remedy in a later
  change.
- **A replaced installer can run code.** A replaced MSI could carry a custom action
  that runs code as the user, or ask for elevation behind a prompt the procedure
  has taught operators to pass. A replaced pkg could carry scripts that run as root
  once the administrator password is entered. Gatekeeper is expected to block a
  replaced pkg that is unsigned or not notarized, or whose signature or notarization
  is revoked, but only when it assesses the file, which it does for a file that
  carries the download mark (`com.apple.quarantine`), as a browser download does. A
  pkg fetched with `gh` or `curl` has no mark. Gatekeeper is also expected to accept
  a pkg signed with any Developer ID and notarized, so it does not tie the pkg to the
  maintainer's team. That is a residual risk: until the CLI document records the team
  ID after the first signed release, nothing in the install procedure compares a
  downloaded package's team with the maintainer's. An operator who runs the optional
  check block verifies the attestation, which ties the file to `release.yaml`'s build
  on `main`, the build that signed it; the attestation does not tie the file to the
  team. An operator who wants the team checked reads `pkgutil --check-signature` and
  compares the team ID once it is recorded. All of this is expected, not yet observed on a
  Mac, and no control is added for it here. The genuine installers carry neither a
  script nor a custom action that runs code, and the build tests that they do not.
- **On axpc, code running as the operator can read what the operator can**,
  including the token files `marshal start` reads. A replaced installer runs as the
  operator, so it can read them; the per-file access control step in
  [Windows Runner Host](../operations/windows-runner-host.md) restricts those files
  to other accounts, not to this one.
- **The pkg's downgrade behaviour is only what the macOS build leg measured.** The
  MSI refuses a lower version, which stops an older genuine MSI under a newer name
  from replacing a newer installed version on Windows even when the operator skips
  the check. The pkg has no such refusal that this project designed; what a lower
  pkg does over a higher one is the outcome that
  [CLI](../operations/cli.md#installing-updating-and-rolling-back) states, and the
  documented rollback does not depend on it.
- **The Windows procedure asks operators to pass prompts for unsigned files.** The
  Windows installer and binary are not code-signed, so Windows SmartScreen and the
  browser warn or block, and the procedure tells the operator to choose "Run anyway"
  and to keep the download. An operator who has learned to pass those prompts passes
  the same prompt for a replaced file. The macOS package is signed and notarized, so
  its procedure teaches no prompt to pass. The prompts are
  described as expected until the first install on each host records them. On axpc, a
  binary fetched with `gh release download` had no `Zone.Identifier`
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211)), and
  SmartScreen did not prompt, with "Check apps and files" On and Smart App Control Off
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511120441)). The
  bare-binary blocks remove the macOS quarantine attribute from what they download;
  the installers' procedure removes no mark.
- **Smart App Control On may block the installers.** It blocks unsigned code unless
  its cloud service predicts the file safe, whether or not the file carries a Mark
  of the Web, and whether it evaluates an unsigned `.msi` is not verified. It is Off
  on axpc. Signing the MSI is the remedy in a later change.
- The runner image's definition is embedded in the binary, so a runner bump reaches
  a host only through a new release and an update.

## The Review Plugin Marketplace Is Not Pinned (Accepted Risk)

`claude-review.yaml` installs the `code-review` plugin from the
`https://github.com/anthropics/claude-code.git` marketplace, at that repository's
default branch at run time. The command it runs is model instructions that the job
executes with shell tools, a repository secret, and an OpenID Connect token in
reach, so this is an exception to the pinning convention above. It is an accepted
risk. The action and Claude Code block the two direct routes to pinning it:

- A ref suffix on the URL. At its pinned SHA the action validates each URL entry
  against a pattern that must end in `.git`, so a `#ref` or `?ref=` suffix is
  rejected, and it has no ref input.
- A local copy under the marketplace's own name. The action accepts a local path,
  but the marketplace's manifest names it `claude-code-plugins`, and Claude Code
  reserves that name for GitHub sources in the `anthropics` organization.
  `claude plugin marketplace add <local clone at 525d3b35>` exits 1 with "The name
  'claude-code-plugins' is reserved for official Anthropic marketplaces and can
  only be used with GitHub sources from the 'anthropics' organization." This was
  reproduced with Claude Code 2.1.286, and reported with 2.1.285 during review.

Two indirect routes can pin the command:

- Rename the marketplace in a pinned checkout at run time, then add that local
  path. The reviewer reproduced this with Claude Code 2.1.285; it was not
  reproduced here. The maintainer was shown this route, as the option "rename and
  keep the plugin pinned", and chose to stay unpinned.
- Check the plugin out at a pinned commit and load its directory with
  `--plugin-dir` in `claude_args`. At the action's pinned SHA,
  `base-action/src/parse-sdk-options.ts` passes flags it does not recognize
  through to the CLI as extra arguments, and Claude Code 2.1.286 lists a
  `--plugin-dir` option. That was checked by reading the source and the help text
  only; no run was made. This route was identified after that decision; when it
  was put to the maintainer, they again chose to stay unpinned.

The maintainer accepted the risk. Its reach is wider than the workflow's
`--allowedTools` list suggests, because a command's own `allowed-tools`
frontmatter pre-approves tools in addition to that list, and it can change
without notice. As an example, not a guarantee, the command at commit
`525d3b35312636cf8c001ecb3df2be0324a48b43` pre-approved `Bash(gh issue view:*)`,
`Bash(gh search:*)`, `Bash(gh issue list:*)`, `Bash(gh pr list:*)`,
`Bash(gh pr comment:*)`, `Bash(gh pr diff:*)`, `Bash(gh pr view:*)`, and the
inline-comment tool. The first four are beyond the workflow's own list.

The controls that remain are these, and none of them pins the command:

- the subprocess environment scrub and the isolation step in the workflow, which
  installs bubblewrap and socat and lifts Ubuntu's AppArmor restriction on
  unprivileged user namespaces for the job's ephemeral VM;
- the least-privilege job permissions described above;
- the author-association gate below, which limits who can start a run;
- the default branch's ruleset, outside this repository's files, which blocks
  deleting the branch and pushes that are not fast-forwards, but not a direct
  fast-forward push;
- the vendor's own trust: a change to that repository's default branch is made by
  the organization that also supplies the action and the model.

Revisit the exception if the maintainer wants the command pinned; either route
above does it without a new action or Claude Code feature.

## Self-Hosted Runners Stay Out of This Repository's Own Jobs

This repository is public. GitHub advises against self-hosted runners on public
repositories, because a pull request from a fork can run arbitrary code on the
runner, and a persistent runner then lets that code outlive the job. Every
workflow in this repository therefore runs on a GitHub-hosted runner, its CI and
its review alike: a hosted label such as `ubuntu-latest`, `ubuntu-24.04-arm`,
`windows-latest`, or `macos-latest`. The CLI's tests run on three of those so each
operating system a runner host can be is exercised. A `runs-on` naming
`self-hosted` is a review finding.

That rule is about this repository's own jobs. A runner image or host script kept
here is a product the repository ships for other repositories to use, and this
repository does not run its own jobs on it. Whichever repository registers such a
runner owns that decision, and the rules for a runner's design are below.
`axross` is a User account, not an organization, so org-level runners are
unavailable and registration is per repository.

## Shared Runner Storage Is a Cache-Poisoning Surface

GitHub Actions isolates a job into its own runner: a job sees only the secrets
and permissions its own workflow grants it. A self-hosted runner that mounts
storage shared between jobs, such as a named Docker volume holding a package
cache, breaks that isolation in one direction. Whatever a job writes into the
volume, a later job reads, and a later job may hold a secret or a write
permission the earlier one did not.

The threat model for any runner image or host script added here is therefore:

- a job that runs untrusted code, such as a pull request's scripts or a
  dependency's install hook, can plant a file in a shared cache;
- a later job that restores that cache executes or links the planted file with its
  own, possibly broader, privileges;
- the attacker never needs the later job's secret; they need only one write to
  storage the later job trusts.

Design rules that follow, which REVIEW.md applies:

- A shared volume is scoped to one repository and one trust level. A volume MUST
  NOT be shared across repositories, between a public and a private repository,
  or between runs of a pull request from a fork and runs on the default branch.
  The bounded exception below does not reach a fork's runs.
- A runner is ephemeral: it takes one job through a just-in-time registration,
  then its container is removed. A long-lived registration token is not stored.
- Volumes hold only content the tool re-verifies, such as a content-addressed
  package cache. A volume does not hold credentials, configuration, or toolchains
  the job later executes, except as the bounded exception below allows.
- **Bounded exception: tool-cache and language-toolchain volumes on a runner
  host.** A volume of installed toolchains that a job later executes, such as
  `/opt/hostedtoolcache`, `~/.cargo`, and `~/.rustup`, is allowed when all three
  conditions hold: it is scoped to one target repository and never shared
  between host-configuration entries, no job that holds a deployment secret
  runs on that entry's labels, and no pull request from a fork runs on that
  entry's labels. The accepted risk is that any job on those labels, including a
  run of a pull request from the same repository, can poison a toolchain that a
  later job of the same repository executes, so every job on one entry's labels
  is one trust level. A fork's code would write binaries that a later job on the
  default branch or with a deployment secret executes, so a repository that
  accepts fork pull requests lists no volumes for its entry, or runs those
  pull requests' workflows on a GitHub-hosted runner. The Cache Volumes section
  of [Windows Runner Host](../operations/windows-runner-host.md) states each
  volume's reason and writers and owns the Android storage exclusions, which
  this exception does not reach. Any other volume of content a later job
  executes or links, or one that fails a condition, stays a finding.
- A job that holds a deployment secret does not mount a volume that a less
  trusted job could have written.
- A container does not run privileged, does not mount the host's container socket
  or home directory, and has its egress limited where the host's network reaches
  anything sensitive. A justified exception is stated where it is declared;
  the LAN egress section below is one, and [The VM-Local Docker Socket](#the-vm-local-docker-socket)
  is another: the one socket a container may mount is that of the per-job VM it runs
  in, never a host's.
- Each shared mount is declared with the reason it exists and the trust level of
  every job that can write to it.

## Per-Repository Isolation on a Runner Host

A runner host serves several repositories from one machine, so each repository's
registration, storage, and containers are kept apart from every other's. The rules
for [`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) and any
later host:

- One token per target repository for a repository pool, a fine-grained personal
  access token limited to that repository's Administration permission, and Actions:
  read where [A Token Carries Only Its Pool Kind's
  Permissions](#a-token-carries-only-its-pool-kinds-permissions) allows. A
  repository pool's token MUST NOT cover several repositories or be stored as a
  GitHub Actions secret. A token that covers
  several repositories, or a classic token, would let one leak register runners
  for all of them, and a secret that can register runners for the automation that
  reads it is not a boundary. The token is read from a file on the host on every
  registration, and the file is restricted to the host user. On macOS the CLI
  refuses a token file that is not a regular file owned by the current user with
  mode `0600`, and skips its pool with a warning. On Windows the CLI does not check
  the token file's access control list, because no safe call reads it, so the
  restriction rests on the operator: the explicit step under Restricting a Token
  File in [Windows Runner Host](../operations/windows-runner-host.md#restricting-a-token-file)
  restricts each file at install and at every rotation. That step's effect is not
  yet verified on axpc ([#56](https://github.com/axross/runners/issues/56)): its
  `icacls` evidence was not collected there.
- Multiple execution pools for one repository MUST use the same token file;
  entries for different repositories MUST NOT share it. Extra pools do not expand
  the token's repository authority.
- A registration is made with the entry's own token and carries `self-hosted`, the
  host's platform labels, and the entry's `labels`, so a job's runner is
  identifiable. An axpc pool carries `self-hosted`, `linux`, and `x64`, and its
  `labels` default to `axpc` when absent or empty. An axmc pool carries
  `self-hosted`, its operating system's labels (`linux` and `ARM64`, or `macOS`
  and `ARM64`), and its `labels`, which default to `axmc` the same way. A job is
  assigned to a runner whose labels include every label in the job's `runs-on`,
  so a job naming only `self-hosted` also lands on the host; the label selects the
  host, it does not keep jobs off it. What keeps a repository's jobs off another
  repository's runner is the entry's token, which scopes the registration to one
  repository, so an entry needs no label of its own and a label shared across
  entries cannot route one repository's job to another's runner. Platform labels
  alone do not isolate same-repository pools; consumers MUST include the intended
  pool's routing label. Same-repository pools on one host MUST NOT share a
  non-platform label unless their operating system labels differ: pools that
  differ only in architecture are not distinguished, and a pool that differs in
  operating system is, provided the consumer also names the operating system label.
  A bare `runs-on: axmc` is therefore ambiguous on a host with a Linux pool and a
  macOS pool, and a consumer of a shared label names the operating system label
  too. A non-empty `labels` list replaces the default and MUST NOT include a
  platform label. General pools explicitly list the host's routing label alongside
  any aliases; dedicated pools omit it. Adding a label while retaining the routing
  label does not reserve a pool.
- Pools for one repository on different hosts MUST NOT share a custom label unless
  their operating system labels differ. `validate` reads one host's configuration
  and cannot see across hosts, so this is a rule for the maintainer and for
  review, not a check.
- A registration reaches the runner through the `ACTIONS_RUNNER_INPUT_JITCONFIG`
  environment variable, set only in the process that starts the container and
  passed to `docker run` by name. It MUST NOT be placed on a command line, in a log
  line, or in an image layer.
- The CLI's registration request (see [CLI](../operations/cli.md#registration)) is
  one HTTPS `POST` to `https://api.github.com`, and the agent's GitHub status reads
  (see [CLI](../operations/cli.md#github-status)) are `GET`s there through the same
  client; neither follows a redirect, so a redirect cannot carry the token to
  another host, and a status read follows a next page only on the same origin and
  endpoint path. It fails a connection after
  10 s, a request after 30 s, and a response over 1 MiB. The token lives in a type
  with no `Display` and a redacted `Debug`, the header that carries it is marked
  sensitive, and error text has the token replaced before it is logged. Only a
  debug build can move the base address, and only to `http://127.0.0.1:<port>`, for
  the loopback tests; a release build always uses GitHub, which a CI step checks in
  the release profile. Likewise only a debug build can name the `docker` client
  `start` and `build` run, and a release build always runs `docker`. The JIT
  variable is removed from the environment of every `docker` client but the one
  that starts the container.
- Volume and container names start with the entry's `name`, which the operator
  sets. An entry's volumes are mounted only into its own containers. Two entries
  whose names are equal, or where one is the other followed by a hyphen, are
  rejected, because one would claim the other's containers and volumes.
  Stale-container cleanup removes only containers matching the entry's own name
  pattern.
- A repository that can run pull requests from forks on a host's labels MUST
  require approval for outside collaborators' workflow runs. A self-hosted runner
  executes whatever a workflow checks out, so an unapproved fork's run would
  otherwise execute arbitrary code on the host before anyone reviews it. The
  Repository Settings section of
  [Windows Runner Host](../operations/windows-runner-host.md) gives the operator
  step.
- `docker run` uses `--pull never`, so a missing local image fails instead of
  pulling a same-named public image.

## A Token Carries Only Its Pool Kind's Permissions

What a token may do is set when the operator creates it, and GitHub cannot report a
token's permissions to Marshal, so limiting each token to its pool kind's permissions
is an operator rule and a review rule, not a check.

- A repository pool's token, whoever owns the repository, has that repository as
  its only repository and carries Administration: Read and write, which registration
  needs. It MAY add Actions: read, so the window can show the job a busy runner runs.
  It carries nothing else.
- Actions: read costs this: a leaked token can also read the repository's workflow
  runs, their logs and their artifacts. Without it the window still shows whether
  each runner is busy.
- A 403 from the workflow-runs endpoint, or from a run's jobs, that is not a
  rate-limit response means Actions: read is missing; Marshal hides job names for
  that repository rather than failing, and logs GitHub's message and its
  `X-Accepted-GitHub-Permissions` header.
- Organisation pools are planned (X2) and no release configures one yet. An
  organisation pool's token is created by an owner of the organisation, has the
  organisation as its resource owner, and carries the organisation permission
  Self-hosted runners: Read and write. It carries no repository permission, and its
  repository access is public repositories only. One token serves one
  organisation.
- A leaked organisation token lets its holder register their own runner into any
  runner group of the organisation, including the default group; set any group's
  repositories to any repository the organisation owns, or open a group to public
  repositories; and move runners between groups, change their labels and delete
  runners. A rogue runner then receives the jobs, checked-out code, `GITHUB_TOKEN`
  and secrets of every repository its group admits. The token cannot delete a
  repository or change branch protection.
- The GitHub status reads keep the registration's outbound limits and secret
  handling, and no `github` value, view state or log line carries a token. The
  workflow and job names GitHub returns are untrusted text: the agent removes
  control and Unicode bidirectional formatting characters and cuts them before it
  publishes them, and a reader shows them as plain text, never as markup or an
  address. A reader builds a job's address itself from its configured repository
  and the two numbers it was given.

## One Instance per Machine, Reached Only by Its User

The CLI's single-instance lock and control channel decide who can start, stop, or
query the process that holds every pool's token, so who can reach them is a
security rule. [CLI](../operations/cli.md#the-instance-layer) owns how each is
built: its names, modes, and checks.

- The lock is machine-wide and no child process inherits it. Every local user can
  try it, and anything planted at its path is refused rather than opened or waited
  on. The lock does not say who holds it, so another local user can hold it or take
  its name first and so keep `start` from running. On Unix the lock file's owner
  can also replace the file while an instance holds it, so a second instance can
  start: the [Unix residual case](../operations/cli.md#single-instance-lock). Both
  are accepted on a single-user host, and `start` reports a held lock.
- The control channel is per user: only the instance's own user can reach it, and
  no remote client can. Another user who takes this user's pipe name first stops
  the channel from opening, which is also accepted on a single-user host.
  On Windows the agent's client does not rely on the name alone: it checks that the
  process serving the pipe runs as the current user before it sends a byte, and a
  server it cannot show to be that user is refused. `start`'s commands do not
  check, and a squatted name there is the accepted cost above.
- No request carries a configuration path for the instance to read, and no
  response carries a token or a JIT configuration. A resume names a path only so
  the instance can compare it with the one it started with.
- The resident agent takes the same lock and opens the same channel with the same
  access rules, so only its user reaches it, and it serves protocol version 2
  there; [CLI](../operations/cli.md#the-agent) owns the protocol. A first line
  that is not a `hello` is refused and the connection closed, so no version 1
  request reaches the agent. A silent or stalled client cannot hold the agent:
  every connection limit is a timer, a client that stops reading is closed after
  5 s, and the agent ends and frees its lock without waiting for the pipe crate's
  blocking flush thread that a stalled Windows client can keep alive (a process
  test shows it). Such a thread is at most one for each final message to a client
  that never reads and never closes, which is accepted on a single-user host.
- Both Windows channels, `start`'s and the agent's, take their pipe name, byte
  mode, refusal of remote clients and the only-user descriptor from one function,
  `instance::windows::control_pipe_options`, so the two cannot differ. A test on
  Windows shows a second pipe of the same name is refused as taken.
- Only a debug build can move the lock, the channel, the log directory, or their
  roots, for the tests; a release build always uses the host's fixed locations,
  and a CI step checks in the release profile that it ignores the request. The
  `instance-helper` binary that the tests and CI run does nothing in a release
  build, including its mode that runs the agent.

## The Agent Keeps Tokens in the Operating System's Store

The resident agent reads each token from the operating system's
credential store, not from a file, and it receives a token from a client over its
control channel. [CLI](../operations/cli.md#the-token-store) owns how the store is
named and used.

- On Windows a token is a generic credential in the user's Credential Manager,
  named `app.axross.marshal:github.com/<owner>/<name>` for a repository or
  `app.axross.marshal:github.com/<org>` for an organization, with **local** persistence.
  The crate's default persistence is the enterprise one, which roams with the user's
  profile and so would carry a repository's token to other machines of a domain
  account; the store asks for the local one, and a test on Windows reads the
  persistence back. Any process of the same user can read the credential, as any
  can read a token file the user owns: the Credential Manager is not a boundary
  against the user's other programs.
- On macOS and Linux the agent has no store, so it skips every pool with "no token
  store" rather than falling back to a file; it never reads `tokenPath`.
- The `token_set` request is the first message in which a secret travels, from the
  client to the agent. The token is held as a `Secret` once the request is parsed,
  is masked in the log from then on, and is never repeated: not in the
  response, an event, a snapshot, an error, or a log line. An error about a token
  names its cause, such as an empty or too long token, and never the params. A
  store failure names its kind and the platform's code, which hold no token. The
  request is only as private as the channel: only the agent's user can send it, and
  on Windows the client sends it only after it has checked that the pipe's server
  runs as that user, so a squatter of another account receives nothing and a failure
  to verify is a refusal. The check is made once at connection, before any request,
  so it covers every request on the connection. A method added under protocol
  version 2's version rules that carries a secret is handled as `token_set` is. The
  residual risks are a squatter of the same user, which
  is accepted on a single-user host, and the tiny window in which a process ID is
  reused between the connection and the check.
- A token longer than 1280 UTF-16 code units or empty is refused before it reaches the
  store, so each token's size is bounded. The repository is not limited to the
  configuration's: `token_set` accepts any well-formed `owner/name`, because a token
  may be entered while the configuration is invalid.

Residual risks, accepted on a single-user host:

- A same-user client can create store entries for any well-formed key, one token of
  at most 1280 units each, and the agent never removes one it did not hear of. The
  agent's in-memory record of which keys have a token also gains an entry for each
  new name a request names, until the next configuration read replaces it; each key
  is bounded by its form, `owner/name` for a repository or a login for an
  organization.
- Another process of the same user that connects to the channel can set or remove
  a token, stop the runners or quit the agent, as it can for `start`'s
  commands.
- The agent holds a token in memory for as long as a registration or a GitHub status
  check needs it, and a
  JIT configuration a runtime-core panic's message carried would reach the log
  unmasked, as for `start`.

## The App Surface

The Windows app is a notification-area icon, a menu and a web view that shows the
runners and takes each repository's token. It runs as the signed-in user, beside an
agent of the same executable that holds the pools' tokens, so the question is what
the app may do with that reach. [App](../operations/app.md) owns how it works.

- **The app process is a client only.** It never holds the instance lock, reads the
  configuration, touches the token store or runs Docker: it talks to the agent in
  protocol version 2 and passes a newly entered token to it in `token_set`, then drops
  it. One executable runs as both, so the operating system cannot enforce the rule
  for the app mode. Only the `marshal-modes` crate reaches the library's agent entry,
  and only when the executable is started in its agent mode; the app's session
  reaches the agent through the library's client alone. A review checks that no
  other app crate takes the lock, opens the store or starts `docker`.
- **The web view loads only bundled content.** Its pages, styles, scripts and fonts
  are the files the app embeds. It never navigates away from them, never opens a
  window from page content, and the release build has no developer tools. The policy
  is set in `tauri.conf.json`, in full:

  ```text
  default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none'
  ```

  Tauri adds the scripts' hashes to `script-src`. The pages use no inline script, no
  `style` attribute and no `eval`. The page's element builder refuses the `style`,
  `href`, `src`, `srcdoc`, `action` and `formaction` attributes and every `on*`
  attribute; any other way an element could load or run something is refused by the
  content security policy above. Text from the configuration, Docker or the agent is set as text,
  never as markup. A change to the policy, to the files it governs or to the element
  builder needs the maintainer's review.

- **Two capabilities grant what a window may call.** The app declares its commands, so
  each works only in a window whose capability grants it. The main window's
  capability grants `core:event:allow-listen` and `core:event:allow-unlisten` and the
  commands that read the view, start all, stop all, build, stop the build, try
  again, set a token, remove a token, hide the window and open a runner's job. The
  confirmation window's
  grants the two commands that read its question and answer it. Neither grants
  anything else, each is limited to its window, and a test shows that a call the
  capability does not grant is refused. The permission files Tauri generates are
  checked in, so a change to them shows in a diff, and Tauri's `dynamic-acl` feature
  is off, so no capability can be added while the app runs. Quit has no command: it
  runs from the menu, on the Rust side.
- **The page cannot open an address.** Open run names a pool and a runner, never an
  address. The app's `open_job` command looks the runner up in the agent's snapshot and
  the `github` topic, refuses a pool the agent does not list, and builds
  `https://github.com/<owner>/<repo>/actions/runs/<run_id>/job/<job_id>` itself from
  the pool's configured repository and the two numbers GitHub gave, then opens it in
  the default browser from the Rust side with Tauri's opener plugin. The plugin's
  page script, which sends link clicks to its own command, is off, and no capability
  grants any of the plugin's three commands, which a test shows are refused.
- **A token passes only through `token_set`.** The window's token field is a password
  input without autocomplete or spell checking. The page clears it when the token is
  submitted, whatever the outcome, and keeps the token in no variable once it has
  called the command. A token half typed is cleared when the window hides: the app
  sends the page `marshal:hidden` before every hide, which needs no permission beyond
  listening, and the page also clears on the web view's own report. The Rust side
  trims it, refuses an empty one or one over 1280 UTF-16 code units, wraps it in the
  library's `Secret`, sends it once and drops it.
  No view state, event, log line or error text of the app holds a token, and the app's
  log masks a word shaped like a GitHub token as defence in depth. The app never
  displays a stored token.
- **No telemetry, no crash reports.** The app's only requests read the update feed
  and download an update, as the next bullet says; opening a job's page hands its
  address to the default browser. The agent's only requests go to GitHub's API.
  Neither sends usage data or a crash report, and
  no dependency may; see [The App's Dependencies](#the-apps-dependencies).
- **Updates come only from the signed feed.** See
  [Updates Come Only From the Signed Feed](#updates-come-only-from-the-signed-feed).
- **The debug-only test variables are ignored in a release build.** A debug build's
  agent and app read `MARSHAL_TEST_ROOT`, `MARSHAL_TEST_DOCKER` and
  `MARSHAL_TEST_GITHUB_API`, so a test can run the executable against a scratch host.
  A release build ignores all three, so no environment can move the lock, channel,
  log, configuration, `docker` program or GitHub address of an installed Marshal, as
  for the CLI under [One Instance per Machine](#one-instance-per-machine-reached-only-by-its-user).
- **The app starts its agent from a file it checked.** It starts it from its own
  executable's absolute path as a plain detached child, never through Tauri's shell
  plugin, and never from a file that is missing, that it could not read when it
  started, or that is no longer the file the app started from. The agent ends within 5 s of the file it started from being moved,
  renamed, replaced or deleted, with the reason `executable_changed`, and the app ends
  with it.
- **The WebView2 runtime is the operating system's.** Windows 11 ships it and
  updates it, and the app bundles no copy, so the web view's engine is as current as
  Windows keeps it; the loader the app links is covered by the licence notices.

Residual risks, accepted on a single-user host:

- Any program running as the user can read a token's Credential Manager entry, can
  connect to the agent's channel and stop the runners, and can read the web view's
  data folder. The Credential Manager and the channel are not boundaries against the
  user's other programs; [The Agent Keeps Tokens in the Operating System's
  Store](#the-agent-keeps-tokens-in-the-operating-systems-store) says the same of the
  store.
- An agent that hangs cannot be ended by Quit, so the goal that quitting leaves no
  Marshal container or process holds only while the agent answers. A Quit the agent
  never answers keeps the app saying it is quitting until the agent's `ending` or the
  connection's end, so the user ends `marshal-app.exe` in Task Manager. The window
  says how only for an agent that does not answer its handshake. The next start
  removes the containers it left.
- Whether WebView2 enforces the policy, whether the agent, the exporter or one of
  their `docker` clients shows a console window (the agent is created with
  `DETACHED_PROCESS`, and the exporter and the clients with `CREATE_NO_WINDOW`, each
  asserted at its command builder, but no check observes the window), and whether the menu opens from the keyboard are shown only on axpc;
  [App](../operations/app.md#verification-status) lists what has and has not been
  shown.
- The app checks its executable and then starts it by path, in two steps, so a file
  put at that path between the two is started. Whoever can do that can already
  replace the executable before the app starts.

## Updates Come Only From the Signed Feed

The Windows app installs a new release by itself, so whoever can put a file in front of
it could run code as the user. [App](../operations/app.md#updates) owns how an update
runs; these are the rules it is reviewed against.

- **Only the app mode updates.** Only the app mode registers Tauri's updater plugin, on
  Windows; the agent, exporter and shim modes never do. The flow runs in Rust, and no
  capability grants the web view an updater command, which a test of the capability
  files shows.
- **One feed, one key.** The feed is
  `https://github.com/axross/runners/releases/latest/download/latest.json`, the only
  endpoint `tauri.conf.json` names. The app verifies every downloaded MSI against the
  one public key committed there, with `requireSignedVersion`, so the signature's
  trusted comment must name the feed's version. A build with no committed key, or a
  development build at `0.0.0-dev`, never checks.
- **Only newer, only from this repository.** The app takes a release only when its
  version is newer than its own, and only when the download address is `https`, on
  `github.com` with no port or credentials, under
  `/axross/runners/releases/download/`; redirects from it are followed, since the
  signature is what makes the bytes trusted. No `dangerous` option is set and
  downgrades are off, which a test of the configuration shows.
- **Bounded requests.** A check gives up after 30 s and a download after 10 minutes.
  Neither uses the GitHub REST API or a token.
- **The installer is started by the app.** The app starts `msiexec` only after the
  agent it asked to drain has ended and its process has exited, so the agent's Job
  Object cannot end the installer, and nothing installs from another client's drain.
- **The record across an update holds no secret.** It is a version and a time in
  Marshal's local data folder, applies for 30 minutes to that version only, and every
  launch removes it.

Residual risks, accepted:

- Anyone who can publish a release here and holds the update-feed key can ship code
  every installed app runs. The key, its custody and its rotation are the feed's
  pull request's to record.
- Tauri's updater keeps each downloaded MSI in the temporary folder
  (tauri-apps/plugins-workspace#2389); Windows' storage cleanup removes it.

## Tokens Stay Out of the Log, and Job Clients End With `start`

`start` holds every pool's token and each slot's JIT configuration for as long as it
runs, and it writes a log that lives after it. [CLI](../operations/cli.md#running-a-host)
owns how the log is written and masked and how its children are contained.

- No token or JIT configuration reaches a log line, the terminal, a debug form, an
  error message, a command line, or a control-channel response.
- Every job container's `docker` client ends when `start` ends, however it ends.
- On Unix a diagnostic container's shim ends its container with `docker kill` and
  keeps it, rather than removing it, so its evidence survives for the next
  finalisation. A forced stop and the Windows console close do the same on purpose.
- Each export runs in an exporter child that ends at its deadline or when its
  liveness pipe from `start` closes, and kills its own clients first: on axpc its
  `docker` clients, and on axmc its `tart exec` clients, which run `docker` in the
  VM. Its clients run in process groups of their own, killed when their call is
  dropped, with the JIT variable cleared, and on Windows the child is also in
  `start`'s Job Object. `start` closes the pipe at the
  export deadline and kills the child 1 s later if it has not ended, so the child
  outlives neither its deadline nor `start`.
- The log directory and its files are per user and private to that user.

Residual risks, accepted on a single-user host:

- On Unix a shim outlives `start` by up to 5 s, and a shim that is itself killed
  leaves its container running until the next `start` removes it. On Windows a
  container outlives its killed client, with the same backstop.
- On Windows, logoff and shutdown events never reach an interactive process, so
  containers left by them wait for the next `start`.
- On Windows the log directory's access control is not checked; it relies on the
  user profile's.
- A JIT configuration that a runtime-core panic's message carried would reach the
  log unmasked, because its mask ends as the panic unwinds.
- A kept diagnostic container, ended but not removed, still holds its JIT
  configuration in its environment until it is finalised, where anyone who may run
  `docker inspect` on the host can read it. The ephemeral runner it configured has
  already ended with its job. On Windows a hard kill of `start` leaves
  a diagnostic job running, as it does an ordinary one, until the next `start`.
- An exporter child that `start` has to kill, because it did not end within 1 s of
  its pipe closing, cannot kill its clients: its `docker` clients on axpc, and its
  `tart exec` clients on axmc, which run `docker` in the VM. On Unix they run until
  each ends by itself, and macOS has no Job Object; on Windows the Job Object ends
  them when `start` ends. They carry no JIT configuration.

## The TOML Configuration Holds No Token and Quotes No Line

A configuration file is untrusted input, and an operator can paste a token into one
by mistake. [CLI](../operations/cli.md) owns the format, its problems, and the write
path; these are the properties it keeps.

- The TOML format has no key for a token or a token file. `token_path` is an
  undefined key, and the conversion from JSON leaves `tokenPath` behind, so a file in
  this format cannot name a token's location.
- A syntax error and a wrong-type problem carry only the key path, the line, the
  column, and fixed text. The reader never uses the parser's display text, which
  quotes the source line, so a token pasted into a value, or on a line of its own, is
  not repeated in a problem, a log, or a terminal. A test pastes a token at every
  position of a valid file and shows it appears only as the name of an unknown key.
- The write path replaces the file in one step: a flushed temporary file in the same
  folder, renamed over `config.toml`. A reader sees the old file or the new one and
  never part of one. On Unix the temporary file is created with mode 0600, and the new
  file takes the old file's mode. A file error carries a path and an error kind, and an
  edit that cannot apply is named by the edit alone. A result that breaks the rules
  carries the validator's problems, which quote only what the first residual risk
  below describes.
- The clean-up of leftover temporary files removes only regular files named exactly
  like a temporary file of the write path, so it cannot remove a file the operator
  keeps beside the configuration.

Residual risks, accepted on a single-user host:

- A token pasted as a key name is echoed as the name of an unknown key, because
  that is how the key is found. A rule that compares a value, such as a duplicate
  label or a name that collides, names that value too, as the JSON problems do.
- The write path compares the file with the bytes it read just before the rename.
  That narrows the window in which an editor's save could be overwritten, and cannot
  close it.
- The write path does not flush the folder after the rename. A crash right after a
  write can leave the old file, and cannot leave a partial one.

## Editing the Configuration Through the Agent Moves No Trust Boundary

The agent's `config_read`, `config_edit` and `config_reload` methods change the
configuration, which decides the repositories runners register to, their labels and
the folder diagnostics are exported to. [CLI](../operations/cli.md#editing-the-configuration)
owns the methods; these are the properties they keep.

- Only the agent's own user reaches the channel, under
  [One Instance per Machine, Reached Only by Its User](#one-instance-per-machine-reached-only-by-its-user),
  and that user can already edit the file. No request names a file for the agent to
  read or write: the agent edits only `config.toml` at its fixed path.
- No request or response carries a token as such. `config_read` sends only the keys
  the schema defines that the file sets with a value of the right type, also while
  the file breaks other rules; an undefined key or a wrong-typed value appears only as
  a problem, which quotes no value. A token pasted on a line of its own, under a key
  of its own, or into a number or a boolean therefore does not reach the window. A
  problem names a key path, as the first residual risk of
  [The TOML Configuration Holds No Token and Quotes No Line](#the-toml-configuration-holds-no-token-and-quotes-no-line)
  describes.
- An edit goes through the write path and then through the checks the agent loads
  with, the checkout rule at the diagnostic directory's own place included, so a file
  the app writes is one the agent would load. An edit or a reload that sets or
  changes a pool's `diagnostic_directory` also runs the exporter's sink checks on it
  (it exists, is a directory, is no link, lies outside the checkout, and is private
  on Unix) and is refused when it fails them; the problem names the key and the
  check, not the path. A directory the configuration in use already names is left
  to the exporter's own checks, as before.
- An edit of `schema_version` or of `[lifecycle]` is refused before the file is
  read.
- A `config_edit` whose `revision` no longer matches the file is refused, and the
  write path compares the file again just before it replaces it.

Residual risks, accepted on a single-user host:

- A same-user client can point a pool's diagnostics at any folder that passes the
  sink checks, as it could by editing the file.
- A token pasted into the value of a defined text key, such as a pool's name or a
  label, is a value of the right type, so `config_read` sends it to the window, which
  shows it in that field. The window is the same user's, who wrote the file.
- The revision is std's default hasher over the file's bytes, with their length:
  SipHash with fixed keys today, which std does not promise to keep. It detects an
  edit made elsewhere; it is not meant to resist a file crafted to collide, and a
  same-user writer needs no collision to change the file. An agent built with
  another Rust may compute another revision for the same bytes, which a client sees
  as `config_changed` and reads the file again.
- A repository no pool names any more keeps its stored token until the client
  removes it with `token_remove`; the agent does not remove it on its own.

## Diagnostic Evidence Stays Private

A diagnostic container's bundle holds what the collector observed during a job, and
the job ran code from the repository. [CLI](../operations/cli.md#diagnostics) owns
how a bundle is written.

- The sink is a directory the operator created, outside the checkout and private
  to the user who runs the CLI, so the evidence reaches no other user and none of
  the repository's files. The CLI refuses a sink that fails its checks and never
  creates, repairs, or relaxes one; the checks and the modes a bundle is written
  with are under Sink checks in [CLI](../operations/cli.md#diagnostics).
- For `start` the checkout is the one above the configuration file; the agent's
  configuration lives outside any checkout, so the agent finds the checkout from
  the sink itself, the first directory at or above it that holds both `.git` and
  `cli/Cargo.toml`, and refuses a sink at or under it, with the same lexical,
  link-free comparison and the per-profile case rule ([The Checkout
  Rule](../operations/cli.md#the-checkout-rule)). A sink the agent could not look at
  from its own place, such as a path of another host, is not compared.
- On Windows the CLI does not check the sink's owner or access rules, because no
  safe call reads them; a bundle inherits the sink's rules. The operator's ACL step
  in
  [Windows Runner Host](../operations/windows-runner-host.md#collecting-private-diagnostics)
  is the only protection on a host that runs the CLI. A sink whose rules admit
  another user would expose the evidence to that user. This is accepted on a
  single-user host, with the ACL step kept in the procedure.
- The sink is never mounted into a job. The container's evidence leaves it only
  through the identity allowlist, which never reads the environment, and an archive
  reader that accepts only the collector's regular files, by name, within fixed
  sizes, so a job cannot write a link, a path outside its bundle, or more than
  40 MiB.
- A bundle is untrusted evidence. The CLI never prints or logs its contents, and a
  warning names only the pool, the container, and the gap.
- A labelled container is removed only by a finalisation or by the manual
  recovery, so a later `start` with the pool's diagnostics off does not destroy its
  evidence. [CLI](../operations/cli.md#diagnostics) owns when a finalisation
  exports, and the cases where it removes a container without an export.
- A diagnostic VM on axmc is the exception, by design: the VM is the retained
  container, and it is deleted without an export in three cases only, a forced stop
  (a second SIGINT), a `start` that ends while it still holds the VM, whose VM shim
  deletes it, and a leftover at the next `start`. Each reports a diagnostic gap that
  names only the pool, the VM or container, and the gap. The paragraph "Evidence a
  VM loses" in the [Tart backend](../operations/cli.md#tart-backend) section of the
  CLI document owns the cases.

## `build` Builds Only What the Binary Embeds

The runner image every job runs in is built by `build` from the definition the CLI
embeds when it is compiled, so what a host builds is what was reviewed and merged.
[CLI](../operations/cli.md#build) owns the steps.

- `build` reads nothing from a checkout, the working directory, or the
  environment for its context: the embedded Dockerfile and the files it copies,
  streamed as an archive the CLI writes, are the whole context. A unit test fails
  when the embedded set is not exactly the Dockerfile and every file it copies.
- Only a debug build can name another definition, for the real-Docker test, and a
  release build ignores the request, as it ignores the other test overrides; a CI
  step checks this in the release profile.
- The context's base image is pinned by digest, and `--pull --no-cache` rebuilds
  every layer from it, so no cache left by another build feeds the image.
- The image CI builds is tagged on the hosted runner and never pushed, and the job
  holds no credential.
- `build` holds the instance lock, so it cannot replace the image under a running
  instance, and the instance checks the image's contract before each job, so an
  image replaced by one with another contract, or removed, registers no runner. A
  replacement between that check and `docker run` is accepted on a single-user
  host.

## CPU Placement Is Not Resource Reservation

Job containers MUST receive both the existing CPU-time quota and automatically
selected affinity, and MUST refuse runner startup when discovery, capacity,
cleanup, or effective process readback fails. A quota limits CPU time, not CPU
IDs. A cpuset limits execution placement but reserves no CPUs or memory, isolates
no repository from shared hardware, and guarantees neither compiler process
counts nor OOM prevention. Slot overcommit and topology changes can create
overlapping sets. CPU discovery MUST use the slot's effective local Docker
image/context without network, mounts, registration, image pull, or added
privileges, and cleanup MUST target only its exact marked probe. The allocation,
deadlines, readback, and separately authorized real-host procedure are in
[Windows Runner Host](../operations/windows-runner-host.md#automatic-cpu-affinity).
CPU policy MUST NOT infer workload roles from names, tags, or labels, or alter
consumer build flags, global WSL allocation, or Windows process affinity.

## LAN Egress From Runner Containers (Accepted Risk)

The Windows host starts containers on Docker Desktop's default network, which
routes to the machine's local network as any process on the machine does. A job
can reach devices there, such as a router or a NAS, that a GitHub-hosted runner
cannot. This is an exception to the rule above that egress is limited where the
host's network reaches anything sensitive. The maintainer accepted this risk,
because the Docker Desktop mechanism to block it (a firewall rule for the
containers' subnet, or a custom network) has not been researched or implemented.
The operator procedure tells operators to run the host only on a network they
would trust the listed repositories' workflows with. Revisit the exception when a
blocking mechanism is verified.

Marshal's agent on Windows keeps that rule when the machine moves
([#82](https://github.com/axross/runners/issues/82)):

- A change of the primary network on two checks in a row stops every runner, unless
  the operator turns `stop_on_network_change` off, and a sleep or a wake always
  does, so runners never carry on onto a network the operator did not choose.
- With `resume_after_stop` on, the runners start again on their own only once the
  primary network recorded before the stop, with its gateway's hardware address,
  holds on two checks in a row. A record without that address, as behind a
  full-tunnel VPN, arms no resume. After 5 automatic resumes within an hour, the next
  signal stop arms none.
- The app's restart of a lost agent and the start after an update keep a signal stop,
  so neither starts the runners on another network.
- The agent writes one value, `Marshal` under the current user's Run key, holding the
  quoted path of the program it runs from and `--sign-in`, only while
  `start_at_login` is on, and removes it when the setting is off. It needs no
  administrator rights, and the value names no token, network or address.

## A Linux Job on axmc Runs in a Per-Job VM

The axmc host runs each `os: "linux"` pool's job in its own Tart virtual machine
(VM), cloned from the golden VM for that job and deleted after it, so a job meets a
single-tenant kernel and filesystem that nothing outlives. This section and the
five after it are the constraints the VM runtime of
[#74](https://github.com/axross/runners/issues/74) is reviewed against.
[macOS Runner Host](../operations/macos-runner-host.md) owns the procedure, and
[CLI](../operations/cli.md) the runtime.

- A job VM MUST be a fresh clone of the golden VM, and MUST be deleted when its job
  ends, on every failure path, at a stop, and by the next `start` if the CLI died.
  A shim per VM holds its Tart process and deletes the VM when the CLI's liveness
  pipe closes, so a hard-killed CLI leaves no VM behind. Cleanup at `start` matches
  only the job VMs of configured pools by their name form, so it never deletes a
  golden VM, a build VM, a VM of an unconfigured pool, or a VM `marshal` does not
  own.
- A job VM MUST share no host directory, clipboard or audio, MUST attach no USB
  accessories, Rosetta or display, and MUST NOT accept the base image's password
  SSH login, which the golden VM disables. A shared directory is a path from the
  job to the Mac's files, and Rosetta, the clipboard and the accessories widen the
  host-facing surface for no job's benefit.
- The JIT configuration MUST reach the guest only through the guest agent's
  standard input, and inside the VM only the golden VM's launcher passes it to
  `docker run`, in the Docker client's environment, as on axpc. It MUST NOT be an
  argument of any `tart` call, appear in a log line, or be written into the VM's
  image.
- Guest-agent output is data. Each call's captured output MUST be cut off at a fixed
  bound (64 KiB), and `marshal` MUST NOT parse it beyond its expected form. Logged
  text is scrubbed and masked as container output is. The one exception is the
  diagnostics export, which streams the `docker cp` archive out of the VM: the
  strict archive reader and the bundle limits of
  [Diagnostic Evidence Stays Private](#diagnostic-evidence-stays-private) bound it
  instead.
- Every `tart` call MUST set `TART_NO_AUTO_PRUNE`, so a clone never deletes an image
  another program cached; MUST get `marshal`'s own `PATH`, which `marshal` also uses
  to find `tart` and `softnet`; and MUST have a standard output that is not a
  terminal, so Tart never offers to change Softnet's privilege. Only `build`
  deletes golden and build VMs.
- A VM MUST have a fixed vCPU count, memory size and disk cap, the pool's `cpus`,
  `memoryGb` and `diskGb`, so one job's use of the Mac's CPU, memory and disk is
  bounded by its pool. The disk cap bounds what a root job can write to the Mac's
  disk.

## CPU Placement Inside a Job VM

Inside a job VM, [CPU Placement Is Not Resource Reservation](#cpu-placement-is-not-resource-reservation)
holds with one difference: the VM is single-tenant, so the runner container's quota
and affinity MUST equal all of the VM's vCPUs, 0 to `cpus` - 1, and its memory limit
MUST equal the VM's memory. Nothing else runs there to share CPUs with, so no
allocation among slots is needed, and the VM's own vCPU count is the bound. The
probe and the guard that read the effective placement back MUST run through the
guest agent, and a failed probe MUST end the slot's cycle before a runner
registers. A job that exhausts memory meets the guest's out-of-memory killer, and
the VM is deleted either way. The Mac's scheduler, not `marshal`, shares its cores
among VMs, so a pool's `cpus` is a count of vCPUs, not a reservation of the Mac's
cores.

## Softnet Confines a Job VM's Network

Every axmc VM runs on Softnet, Tart's filtering network, so a job cannot reach the
local network, another VM, a multicast group or an IPv6 address, and reaches the
Mac itself only for DNS. No acceptance in
[LAN Egress From Runner Containers (Accepted Risk)](#lan-egress-from-runner-containers-accepted-risk)
extends to axmc: that section is about Docker Desktop's default network, and a VM on
Tart's default network would reach the LAN, the host and other VMs.

- Each `tart run` MUST pass Softnet two rules. `--net-softnet-allow` carries
  `out proto udp @host port 53,out proto tcp @host port 53`, and
  `--net-softnet-block` carries `out @host,224.0.0.0/4`. A rule with a port outranks
  one without, so DNS to the Mac's address on Softnet's bridge (the gateway) passes
  and every other packet to the gateway and all multicast do not. With Softnet's
  defaults, which block the private, shared, link-local and loopback IPv4 ranges and
  every IPv6 frame, a VM can then send only to globally routable IPv4 addresses and
  to the gateway's DNS. Softnet's defaults alone let a VM reach every port on the
  gateway, a Mac service listening on all interfaces among them, and forward
  multicast, so they are not enough. A `tart run` MUST carry no other network option.
- Softnet's privilege is a SUID bit on its binary, with no `sudoers` rule.
  `marshal` MUST check it before every VM boot and MUST NOT set it. It finds `softnet`
  as Tart does, as the first match on the `PATH` it gives Tart, follows links, and
  requires a regular file owned by root, with the SUID bit set and no write
  permission for its group or others. Otherwise it refuses and boots nothing. A
  `softnet` that an unprivileged user can write would let that user run code as
  root, and Homebrew installs a new binary without the bit at each upgrade, so the
  check runs at every boot rather than once. `marshal` never handles a root-owned
  file itself, and a `softnet` that does not answer `--version` counts as a
  refusal. Only a debug build can name the `tart` client (`MARSHAL_TEST_TART`) or
  replace the owner and mode read from Softnet's file
  (`MARSHAL_TEST_SOFTNET_OWNER_MODE`), for the tests; a release build always runs
  `tart` and judges the real file. `cli/tests/release_overrides.rs` checks this,
  and the Merge Checks step "Test That A Release Build Ignores The Debug-Only
  Overrides" runs it in the release profile. Likewise, only a debug build can name
  the `route`, `arp` and `caffeinate` programs (`MARSHAL_TEST_ROUTE`,
  `MARSHAL_TEST_ARP`, `MARSHAL_TEST_CAFFEINATE`) or script the sleep, the primary
  network and the keep-awake hold from a directory (`MARSHAL_TEST_SIGNALS`); a
  release build runs `/sbin/route`, `/usr/sbin/arp` and `/usr/bin/caffeinate` and
  reads the Mac's own signals, and the same test checks both.
- Softnet's rules make it track every flow a VM starts, in a table of 32,768
  entries that refuses new connections while full. A job that opens more than that
  within about 5 minutes has new connections refused, and the failure stays inside
  that job.
- The filtering rests on a reading of Softnet's source, which already contradicts
  Tart's documentation about host access. No host run has probed it, and the
  maintainer accepted it as unverified
  ([#74](https://github.com/axross/runners/issues/74#issuecomment-6784246305)).

## The Accepted Host-Facing Surface of a Job VM

These are what an axmc job VM can still touch, or depends on, and the maintainer
accepts them. The acceptance holds only while no public repository and no fork pull
request reaches axmc: a pool serves private repositories whose workflows the
maintainer controls. A change that routes a public repository or a fork pull
request to axmc ends it and needs a new decision.

- **Softnet**, a SUID-root component that every axmc pool depends on. A flaw in it
  is a flaw in the Mac's root boundary, which is why it is pinned and checked.
- **Tart**, a pinned external program that `marshal` calls and never bundles.
- **Guest-agent output**, which the CLI bounds and treats as data.
- **Host disk use by a root job**, limited by the per-VM disk cap.
- **Root in the VM, held by any job**, since the VM-local Docker socket gives it with
  one `docker run --privileged`. The job can send raw frames and rewrite the guest's
  network stack, so Softnet's filtering is the only network boundary in front of the
  host. It can also replace the guest's binaries, including the guest agent, so
  guest-agent output is controlled by the job and bounded only by the 64 KiB cut-off
  and, for the diagnostics export, the strict archive reader.
- **A globally routable address on one of the Mac's interfaces**, which Softnet's
  global allowance would reach through the gateway. The maintainer runs the host on
  networks that give the Mac private addresses.
- **DNS through the Mac's resolver**, which can answer the names of the Mac's
  networks and VPNs, though their addresses stay blocked.
- **The JIT configuration**, which the job can read inside its VM. It is single-use
  and already in the runner's environment.

## The VM-Local Docker Socket

From image contract 2, the runner container of an axmc job mounts the Docker socket of
the VM it runs in, `/var/run/docker.sock`, so a job step can use `docker run`. The
socket belongs to the VM's own Docker Engine, in a VM that is cloned for one job and
deleted afterwards, and not to a host, so no other job and no host process shares it.
That is why the rule that a container never mounts a container socket does not reach
it. The permission is narrow:

- The runtime that mounts it is the VM runtime, and only into the runner container,
  as the one extra option `--mount type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock`
  of `vm_job_run_arguments`. The Docker runtime on axpc MUST NOT mount any socket,
  because Docker Desktop's engine is the host's and serves every pool, and its list,
  `job_run_arguments`, carries none. `marshal` MUST NOT start a runner container
  privileged, with a host's Docker socket, or with any other bind mount. Tests in
  `docker_backend.rs`, `tart_backend.rs` and `runtime::docker_args` hold each side,
  and the socket mount is part of the fingerprint in `image::contract` that approves
  contract 2.
- A job that can use the socket is root in its VM: it can start privileged containers,
  mount the VM's filesystem, and read the JIT configuration, which was already in the
  runner's environment. Before the socket a job was root only inside an unprivileged
  container, so root in the VM needed a container escape; now one
  `docker run --privileged` gives it. The container stops being a layer in front of
  Softnet and the guest-agent channel, and
  [the accepted host-facing surface](#the-accepted-host-facing-surface-of-a-job-vm)
  names what that root can touch. It reaches nothing beyond the VM except what the VM
  can already reach, under [Softnet's rules](#softnet-confines-a-job-vms-network), the
  per-VM disk cap, and the VM's own vCPUs and memory.
- The containers a job starts through the socket are siblings of the runner container
  inside the VM, not children of it. The runner container's CPU quota, affinity and
  memory limit, which [CPU Placement Inside a Job VM](#cpu-placement-inside-a-job-vm)
  holds equal to the VM's, do not reach them, and the VM's vCPUs and memory bound them
  instead.
- The socket's group is the VM's `docker` group, which the golden VM creates with the
  id 123 (`golden::pins::DOCKER_GROUP_ID`). The runner image's `docker` group, which
  its `runner` user belongs to, has that id too. Docker gives a container's user the
  groups that the image's `/etc/group` lists for it, so the runner uses the socket
  with no `--group-add` and no change to the container's user. The match grants
  nothing the mount does not, since the socket is root in the VM already. A base
  image that gave the group another id fails the image build, and a VM whose `docker`
  group has another id fails the golden VM build.
- The runner image's Docker client is pinned like the golden VM's Docker Engine; see
  [Tart, Softnet, and the Golden VM Are Pinned](#tart-softnet-and-the-golden-vm-are-pinned).

The permission holds only inside a per-job VM. A change that mounts a socket on axpc,
mounts a host's socket, or gives a VM to more than one job ends it and needs a new
decision. [macOS Runner Host](../operations/macos-runner-host.md) records that the
mount and the group match have not run on a Mac.

## Tart, Softnet, and the Golden VM Are Pinned

`marshal` neither bundles, installs nor repairs Tart or Softnet: the maintainer
installs both with Homebrew, and `marshal` calls them as external programs. What
they and the golden VM are built from is pinned, so that a rebuild changes only
what a reviewed change changes.

- Tart and Softnet are each pinned to one tested version, recorded in the CLI and
  in [macOS Runner Host](../operations/macos-runner-host.md#installing-tart-and-softnet).
  The tested version runs. A newer patch of the tested minor version runs with a
  warning that names the tested version. Any other version, and any output other
  than three dot-separated numbers (which Softnet alone follows with
  `-<short commit>`), is refused. A minor release can change isolation flags or
  `tart exec`, so an untested minor version does not run. Each host run records the
  versions in use.
- The golden VM's base image, `ghcr.io/cirruslabs/ubuntu:24.04`, MUST be pinned by
  digest. A change of the pin adds the old digest to the CLI's list of retired
  pins, and `build` removes from Tart's cache only the digests on that list, never
  the current pin.
- Docker Engine in the golden VM MUST come from Docker's apt repository at pinned
  package versions, held there, after the repository's signing key is checked
  against a recorded fingerprint. A mismatch fails the build.
- The golden VM is built on each host and never published to a registry. A rebuild
  upgrades the base's Ubuntu packages, so it picks up security fixes; the golden VM
  version goes up whenever the pin, the provisioning steps, the launcher or the
  Docker Engine versions change.
- The runner image's Docker client MUST come from Docker's apt repository at a pinned
  package version, after the repository's signing key is checked against the same
  recorded fingerprint, and the image keeps neither the repository's source nor its
  key. The image's pins are the golden VM's, and a unit test holds the Dockerfile to
  `golden::pins`, so a change of one without the other fails.
- The runner image's arm64 build and smoke test in CI run on `ubuntu-24.04-arm`, a
  GitHub-hosted runner, tag the image on the runner and never push it, and their job
  holds only `contents: read`.
- The VM runtime adds no crate to the CLI.

## Nothing Identifying or Secret Is Committed

The repository is public and is meant to be reusable by anyone, so it holds no
credential and no value that identifies a person or a machine: no token, key,
registration or just-in-time configuration, `.env` value, hostname, IP address,
home-directory path, or account name, other than the public maintainer handle and
the owner and repository names of this repository and of `axross/skills`, and no
consumer-specific owner, repository, image, volume, or task name. Examples use
placeholders (`<owner>`, `<repo>`). A value a runner needs is read from the
environment or supplied at run time. The exceptions are the two default runner labels, `axpc`
and `axmc`, which a host adds when `labels` is absent or empty and which are
committed by the maintainer's decision: `axpc` names the Windows Docker Desktop
host and `axmc` the macOS host. Every other host, consumer, or machine value stays
a parameter. A further exception is the macOS signing team ID and the exact
designated requirement, which a documentation change records in the CLI document
after the first signed release as the contract text for later work. The team ID is
public in every signed file, so recording it discloses nothing; it is not an account
credential. Another is the maintainer's update-feed public key, committed in
`app/crates/app/tauri.conf.json` and pinned by the feed crate's configuration test:
the app verifies every update against it, so it must be in the build, and a public key
lets no one sign or sign in. Its private key and password are never committed; their
custody is the feed's pull request's to record.

The checked-in licence notices under `app/notices/` reproduce third-party licence
texts and copyright notices verbatim, as those licences require, including the names
and e-mail addresses of the people they name. That text is the one exception to the
rule against committing personal names, account names and e-mail addresses. It covers
only verbatim licence and copyright text that `app/notices/generate.sh` produces or
that the repository's licence copies hold (`app/notices/licences/` and the licence
files beside the window's bundled fonts and colours). It also covers the published
repository URL that `generate.sh` writes into the notices for each crate, and a
reference in `docs/operations/app.md` to a third-party crate's public repository,
today `wravery/webview2-rs` and `madsmtm/objc2`; an account name, personal name or
e-mail address of any other kind, or anywhere else, is still a finding.

[`.gitignore`](../../.gitignore) excludes `settings.local.json`, `.env.local`,
token files, and private keys so that a local working file cannot be committed by
accident; it is a backstop, not the control. The workflows read one repository
secret, `CLAUDE_CODE_OAUTH_TOKEN`, used by `claude-review.yaml` and added by the
maintainer. `sign-macos` reads six Environment secrets of the `release` Environment,
`MACOS_APPLICATION_P12_BASE64`, `MACOS_INSTALLER_P12_BASE64`, `MACOS_P12_PASSWORD`,
`MACOS_NOTARY_KEY_P8`, `MACOS_NOTARY_KEY_ID` and `MACOS_NOTARY_ISSUER_ID`, and one
Environment variable that is not secret, `MACOS_TEAM_ID`; the maintainer creates
them, and no value is committed. A secret value is never echoed, passed on a command
line, or written into an image layer or build cache. Two exceptions for a
credential on a command line are recorded, both in `sign-macos`. The password
arguments of `security` were allowed at the plan gate. The `--key-id` and `--issuer`
arguments of `notarytool`, which carry the API key's ID and issuer ID, were allowed
by the maintainer on 2026-11-04 PST. Neither tool has a non-interactive alternative:
`security` has none for its password, and `notarytool` and its `store-credentials`
command take the IDs only as arguments or through a keychain profile that is itself
created from those arguments, with no documented environment variable or
configuration file. The runner is discarded after the job, and a request is
authenticated by a token signed with the `.p8` private key, which reaches `notarytool`
only as a file path. GitHub masks secret values in the
log, not in a process listing.

## The Review Gate Admits One Bot Identity by Login, Not Association

`claude-review.yaml`'s reviewer job gates on the comment's `author_association`,
admitting only `OWNER`, `MEMBER`, and `COLLABORATOR`. The gate exists to keep an
untrusted author from spending this repository's tokens or steering the reviewer.
Review requests from the Claude Code route, when posted through the installed
Claude GitHub App, carry the `claude[bot]` identity, whose association GitHub
reports as `NONE`, the same value an outside contributor's comment carries, so
those requests were gated out identically to an outsider's. The gate's fourth
clause admits that one login directly, and further requires the comment to carry
no Markdown heading anywhere in it. That test does less than it looks like it does:
the reviewer's summary opens with a one-line tally, not a heading, so the test
normally does not exclude it.

Admitting a login rather than widening the association list does not widen who can
trigger a paid run: a comment under the `claude[bot]` identity can only be
produced by a session holding this repository's own operator credentials. The
reviewer also posts its own summary under that same identity, so a login-only
clause would admit the reviewer's own output back through the gate it fired from
and arm an unbounded review chain.

The real guard is a rule, not the heading test. [REVIEW.md](../../REVIEW.md)
forbids the reviewer from writing the review trigger phrase anywhere in a summary
or an inline comment. A summary that quoted the phrase, say while describing an
acceptance criterion, would satisfy every clause of the gate, since it comes from
`claude[bot]` and normally carries no Markdown heading, and would start another
review. The workflow's `!contains(body, '## ')` test only excludes a comment that
happens to carry a heading. Only a top-level summary can trigger the workflow,
since inline review comments arrive as a different event, but the rule covers both
so that it is simple to follow and to check.

Two properties keep the admission bounded, and a third does not hold:

- Execution of untrusted code stays closed: the job checks out the default
  branch, never the pull request head.
- The prompt is fixed, built from the pull request number and repository, never
  from the comment body, so a commenter's text is not an instruction.
- **Steering is not closed.** The pull request's title, description, diff,
  comments, and files are untrusted model input that the reviewer reads, and a
  prompt injection in them can redirect the reviewer within the tools it holds.
  Those tools are the workflow's `--allowedTools` plus whatever the unpinned
  command's frontmatter pre-approves, as described above. A `gh` command can name
  another repository with `-R`, so the command allowlist does not confine reads to
  this repository; only the repository scope of the App token does, which the
  action documents but this job cannot confirm. The token stays reachable to the
  job: the action exports it as `GH_TOKEN` and `GITHUB_TOKEN`, which `gh` needs, and
  writes it into the checkout's `.git/config` remote URL. It carries the installed
  App's permissions, which can include writing contents.
  The action documents its subprocess scrub as removing Anthropic, cloud, and
  GitHub Actions secrets from subprocess environments; this job relies on it for
  `CLAUDE_CODE_OAUTH_TOKEN` and the `ACTIONS_*` runtime tokens, not for the two App
  token variables.
  Blocking direct pushes to the default branch with a repository ruleset caps the
  damage, and is a maintainer decision made outside this repository.

The cost is a silent-skip failure mode. The gate trusts a login string; if that
login changes, the gate silently reverts to skipping the Claude Code route's
requests, with no failed run and no comment. The trigger-phrase rule carries the
opposite risk: if the reviewer quotes the phrase, a review starts that nobody
asked for. Neither is mechanically detected; both are found only by noticing that
reviews stopped arriving, or arrived when they should not have. Admitting the
`NONE` association itself was rejected outright, since that would admit every
outside author.
