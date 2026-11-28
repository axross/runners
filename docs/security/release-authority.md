---
type: Policy
title: "Release authority and signing boundaries"
description: "Determine who may dispatch or approve a release and which jobs may sign, attest or publish."
---

# Release authority and signing boundaries

The maintainer controls release dispatch and Environment approval. Use the record below to check a change to the release workflows, the release tooling or the release scripts.

## A Release Is Published Only From a Dispatch on Main

[`release.yaml`](../../.github/workflows/release.yaml) releases the app with
semantic-release, and
[`release-build.yaml`](../../.github/workflows/release-build.yaml) builds the app
executable and its MSI; [CLI](../marshal-cli/release-artifacts.md#releases) owns the steps. This is the
record a change to either workflow, to the build call in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml), to
[`release/`](../../release), or to the scripts under
[`.github/scripts/`](../../.github/scripts) is checked against:

- **A manual dispatch is the only trigger.** `release.yaml` runs on
  `workflow_dispatch` and on nothing else: no push, tag, pull request, schedule, or
  `workflow_run` trigger. The dispatch is the maintainer's decision to release.
  From W1's installer pull request until the next signing job, M2's or W4's, joins
  the chain, no job declares the `release` Environment or any other, and none waits
  for an approval, so in that period the dispatch, after the commit check in
  [Each Release](../releases/publish.md#each-release), is the only
  decision. The Environment and its secrets are kept for that signing job, as the
  held record below says.
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
  - `assemble-signed` holds `contents: read` alone, declares no Environment, and
    does not run the npm tree;
  - the attest job holds `id-token: write` and `attestations: write`, and nothing
    else. It checks nothing out and installs no npm package: it downloads the
    set from `assemble-signed`, checks it, and runs `actions/attest`;
  - the release job holds `contents: write`, to push the tag and its note and to
    create the release, and runs the npm tree.

  A job that runs the npm tree can push tags and edit releases, and its token can
  fast-forward main, but it cannot mint an attestation, so a file it substitutes
  fails `gh attestation verify` for an operator who runs the optional check before
  opening it. An operator who skips the check opens the substituted file, as the
  residual risks below say. A change it pushes to main is another matter, as they
  also say. One release job with all three scopes
  was rejected: any package in the npm tree could then attest any file as
  `release.yaml` on `refs/heads/main`. No job needs `artifact-metadata: write`,
  which is for storage records in a registry. `@semantic-release/github`'s comments
  and labels are off, so the release job needs no issue or pull request scope.
  Every job in `release-build.yaml` and the dry-run call in `merge-checks.yaml` hold
  `contents: read` alone, and neither has a publish or attest step.

- **The files are checked twice before they are used.** The attest job, and the
  release gate [`verify-release.sh`](../../.github/scripts/verify-release.sh) that
  semantic-release runs before it tags anything, each refuse unless the files are
  exactly the app executable and the MSI named for the version and `SHA256SUMS`,
  `SHA256SUMS` lists exactly those two, and `sha256sum --check --strict` passes. The
  attest job attests the two files `SHA256SUMS` lists. The MSI is a per-user package
  that the build leg packs the release app into unchanged and tests; it is not
  signed. Its only custom actions that run code are the start of the installed app,
  only when the `msiexec` line sets `AUTOLAUNCHAPP=True`, and at an uninstall the
  `reg.exe` deletion of the HKCU Run value `Marshal`; both run as the installing user,
  and the build leg's test fails on any other. One script,
  [`assemble-release.sh`](../../.github/scripts/assemble-release.sh), writes every
  `SHA256SUMS`. The gate checks against the version semantic-release worked out in
  its own run.
  `@semantic-release/github` skips an asset that is missing rather than failing, so
  the gate is what holds the file set.
- **Signing is held for M2 and W4.** No job signs, declares an Environment or reads
  a signing secret from W1's installer pull request until M2's disk-image pull
  request brings back `sign-macos` and `verify-macos` or W4's feed pull request adds
  its signing job. The Signing, Held for M2 and W4 section of
  [CLI](../marshal-cli/signing.md#signing-held-for-m2-and-w4) says what returns, and its Signing Contracts stay
  in force. The Environment and its secrets are kept for those jobs, and the
  record below is held for them:
  - **The Environment `release`** is created and maintained by the maintainer.
    Deployment branches are selected branches, `main` only and no tags. Required
    reviewers are the maintainer, with "Prevent self-review" off, because the
    maintainer is the only reviewer and
    [the roadmap](https://github.com/axross/runners/issues/72) requires
    self-approval. "Allow administrators to bypass" is off. The maintainer reports
    the settings on the tracking issue, because agents cannot read them.
  - **Its secrets** are the six Environment secrets `APPLE_APPLICATION_P12_BASE64`,
    `APPLE_INSTALLER_P12_BASE64`, `APPLE_P12_PASSWORD`, `APPLE_NOTARY_KEY_P8`,
    `APPLE_NOTARY_TEAM_KEY_ID` and `APPLE_NOTARY_ISSUER_ID`, and its variable is
    `APPLE_TEAM_ID`, which is not secret. A job reads them only if it declares the
    Environment, and only after the reviewer approves it. No repository secret is
    added for signing. W4's `sign-update` adds two more Environment secrets,
    `UPDATE_FEED_SIGNING_KEY` and `UPDATE_FEED_SIGNING_KEY_PASSWORD`, which the
    maintainer creates before the first release with the feed. They reach only its
    signing step, through `env:` and never a command line, and no job reads them
    until that job is in the release run.
  - **The signing identifier** `app.axross.marshal` and the team are fixed and
    checked at each release, so the designated requirement of the code object that
    carries it stays the same. Changing either breaks the Keychain trust that later
    work relies on and goes through a roadmap revision.
  - **A signing failure stops the release before any tag.** Every signing step
    comes before `release`, no job continues past a failure, and the notarization
    result is read from the status, not the exit code.
  - **Pull requests never sign or reach the Environment.** The dry run builds
    unsigned, and no workflow declares an Environment.
    [`check-release-shape.mjs`](../../.github/scripts/check-release-shape.mjs) in
    `mise run check` fails if any job or step in any workflow declares an
    Environment or reads a signing secret, so the signing job that returns changes
    it in the same pull request.
    The What the Release Tests Cover section of [CLI](../marshal-cli/release-artifacts.md#what-the-release-tests-cover)
    lists its rules. A pull
    request can edit it, so it guards against mistakes, not against a hostile pull
    request, and [CLI](../marshal-cli/release-extensions.md#extending-the-release-workflows) names
    what it does not read. What keeps pull requests away is the branch rule on the
    Environment and that `release.yaml` runs only on a dispatch. W4's feed jobs are
    [staged](../marshal-cli/release-artifacts.md#the-update-feeds-tools-and-checks)
    in a file that is not a workflow, so its `environment: release` reaches nothing
    and the shape check does not read it. The `update-feed` jobs of
    `merge-checks.yaml` hold `contents: read`, reach no Environment and read no
    secret; they run the staged signing scripts with a throwaway key made for the run.
  - **No secret is printed.** In a signing job, secret values reach the scripts through `env:` and
    files written with mode 0600 under the runner's temporary directory, and each
    line of the `.p8` is masked. The keychain and those files are deleted whether the
    job passes or fails, and the notary log, which holds no secret, is uploaded.
    The certificates' subjects name the maintainer's Apple developer account and
    team, as every Developer ID signature does; nothing of that is committed, except
    that a later documentation change records the team ID and the exact designated
    requirement in the CLI document, as the exception under [Nothing Identifying or
    Secret Is Committed](./disclosure.md#nothing-identifying-or-secret-is-committed) describes.
  - **Extensions** by later work add their signing jobs or steps to the shape check
    and keep these rules: only signing jobs declare the Environment, a signing job
    holds `contents: read` and runs no repository code, checksums and attestation
    follow every signing step, no pull-request run reaches the Environment, and
    F0's and M2's signing jobs use only operating-system signing tools. W4's
    update-signing job is the one exception to that last rule: it runs `tauri-cli`'s
    `signer sign`, built with `--locked` in a tool job without the Environment from a
    crate archive whose SHA-256 equals one committed in the repository. The signing
    job checks the binary it receives against the SHA-256 that the tool job outputs,
    before it runs it. The job also has one `sudo sysctl` step, recorded in
    [Security](./workflow-permissions.md#feed-signing-lifts-ubuntus-user-namespace-restriction).
    These are contracts for later pull requests, not jobs already implemented.
    W1's installer pull request comes first; M2's disk-image and W4's feed
    pull requests may then merge in either order. From W1's installer pull request
    until the next signing job, M2's or W4's, no job declares `release` and no
    signing approval step runs. The maintainer's dispatch remains the decision;
    the Environment and its secrets stay in place. W4's pull request MUST allow
    `release` on `sign-update` as well as `sign-macos` when present. The release
    build's exact-pin `cargo install --locked` use of `tauri-cli` is an exception
    under the Pinning Convention, with no cache and Cargo's checks against the
    crates.io index checksums.
    [CLI](../marshal-cli/release-extensions.md#extending-the-release-workflows) owns the release
    sets, job dependencies, restoration point and each extension's obligations.
- **The documented install blocks run against the dry run's installer on a hosted
  runner.** The `Install Blocks` job in
  [`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) needs the dry run,
  downloads its artifact, and holds `contents: read`. It stubs `gh` and runs the
  blocks of [CLI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back) with
  the real MSI, so it installs and removes the dry run's own MSI on a
  `windows-latest` runner that is discarded afterwards. It also creates a local user
  outside the Administrators group, runs the check block and the install as that
  user through a secondary logon with its profile loaded, and removes the user at
  the end. Nothing changes a user right. The test sets the Windows Installer
  policy `DisableMSI` to 0 for the run, because the GitHub-hosted Windows Server
  image blocks a non-elevated per-user install, and restores it at the end. The
  runner is discarded after the job either way.
  It uses no secret, attests nothing, and adds no action beyond the `actions/*`
  already used. This job and the build leg upload their installer logs, from
  `msiexec /l*v`, as workflow artifacts. They ask for 14 days. The repository's
  artifact retention limit applies instead when it is shorter, and it cut the build
  leg's installer logs to 7 days in a pull request's dry run.
  The logs may hold the ephemeral GitHub-hosted runner's machine and account names,
  which the job log's "Set up job" section already shows, and they hold no secret.
- **No credential stays in a checkout.** Every checkout sets
  `persist-credentials: false`. Under GitHub Actions, semantic-release builds an
  `x-access-token:` URL from `GITHUB_TOKEN` for its own git commands.
- **No cache.** No job uses a cache action, and `actions/setup-node` is given no
  `cache` input and `package-manager-cache: false`, because a release run could
  restore a cache that another ref saved. Each `cargo` build and test step compiles
  from `Cargo.lock` with `--locked`, except `cargo tauri build`, which runs Cargo
  without it: the build leg copies the stamped `app/Cargo.lock` just before that
  build and fails the step if the build changed it, so a build that resolved
  anything anew fails. The build leg installs `tauri-cli` into a fresh directory
  under `$RUNNER_TEMP`, and `npm ci` installs from `release/package-lock.json`.
- **Context values reach a script through `env:`**, never expanded into `run:`.
- **Pins.** `actions/checkout`, `actions/setup-node`, `actions/upload-artifact`,
  `actions/download-artifact`, and `actions/attest` stay on major tags, under
  [`actions/*` Stays on Major Tags](./dependency-pins.md#actions-stays-on-major-tags). Neither release
  workflow uses a third-party action, and the one tool the build fetches besides
  the recorded Rust toolchain and `tauri-cli` exceptions is WiX, pinned under
  [Pinning Convention](./dependency-pins.md#pinning-convention). `jdx/mise-action` was rejected for Node
  here: at this repository's pin it caches by default, installs every mise tool, and
  receives the job token. W4's staged `update-tool` job and the `update-feed` job of
  `merge-checks.yaml` use the pinned action only to install `cargo-deny`, with
  `install_args: cargo-deny` and `cache: false`, and neither holds the Environment.
  `actions/setup-node` installs Node `24.21.0`, the version
  in `mise.toml`, and the Node download is trusted under the same joint-trust
  decision, not by a checksum; see [Pinning Convention](./dependency-pins.md#pinning-convention).
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
  [Cutting a Release](../releases/publish.md#cutting-a-release)
  states the rules. Nothing is committed back: `cli/Cargo.toml`, `app/Cargo.toml` and
  their lockfiles on main hold `0.0.0-dev`, and
  [`set-release-version.sh`](../../.github/scripts/set-release-version.sh) writes
  the version in the build leg. It refuses unless every version it owns holds the
  placeholder, `--locked` fails if a manifest and its lockfile disagree, and the
  leg's product-version check compares the built `marshal-app.exe` with the
  version.

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
  that `SHA256SUMS` does not list, and an installer whose `marshal-app.exe` does not
  match the version's `SHA256SUMS` line for the app. An attestation from any other workflow, branch, or commit is refused
  there; see
  [CLI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back). The
  attestation's role is to make that check possible.
- Immutable releases are turned on for the repository before the first dispatch,
  so a published release's assets cannot be replaced and its tag cannot be moved
  or deleted.

Immutable releases are a repository setting, not a file here;
[Cutting a Release](../releases/publish.md#cutting-a-release) gives
the setting and its read-back.

An agent never dispatches `release.yaml`, never approves a deployment of it,
including the `release` Environment's approval of a signing job, never pushes,
moves, or deletes a `v*` tag, and never changes release settings, such as
immutable releases or an Environment or ruleset that applies to releases. This rule
is absolute, and it is
stricter than the
[delivery grant](../agent-work/delivery-grant.md#authorizing-delivery-operations).
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
  [Each Release](../releases/publish.md#each-release), before
  each dispatch, that every commit on main since the last published release came
  from a merged pull request. A stronger option is a rule on main that requires a
  pull request, with only the maintainer able to bypass it. That is the
  maintainer's choice, and it is not in place.
- **The attest job attests whatever passes its checks.** `SHA256SUMS` comes from
  the same build artifact, so the check catches transport damage, not a compromised
  build leg.
- **No tag ruleset.** Any account with write access can dispatch a release, and
  dispatching needs only that access. That is the account the agents run as. The rule
  above is the only control on agents. The maintainer accepted this in choosing a
  manual dispatch with no ruleset.
- **A hostile change on main gets signed if it is approved (held for M2 and W4).**
  Once a signing job returns, it signs whatever the dispatched commit builds, with the maintainer's Developer ID. The
  control is the check in
  [Each Release](../releases/publish.md#each-release), before each
  dispatch and before the approval, that every commit on main since the last
  published release came from a merged pull request. A stronger option is a rule on main that
  requires a pull request, which is the maintainer's choice and is not in place.
- **One identity can dispatch and approve (held for M2 and W4).** The Environment allows self-approval,
  because the roadmap requires it and the maintainer is the only reviewer, and agents
  share the maintainer's GitHub identity. A session could therefore dispatch and
  approve a signed release. The rule above, which now covers approving a deployment,
  is the control, and a second reviewer is not an option here.
- **A certificate or membership lapse stops signing (held for M2 and W4).** The
  Developer ID certificates are expected to last one year, which the maintainer
  confirms from the certificates' dates, and the signing job refuses an expired one and warns within 90 days.
  Signatures made with a secure timestamp stay valid after the certificate expires.
  Whether an older release's package still installs after its Installer certificate
  expires is not confirmed, and Apple's pages disagree, which bears on rolling back
  through that package. A membership lapse leaves signed software working but allows
  no new certificate. [Development
  Workflow](../releases/signing-renewal.md#renewing-the-certificates) records
  the dates and the renewal.
- **An early dispatch creates an empty Environment (held for M2 and W4).** Once a
  signing job declares it again, a dispatch before the maintainer creates the
  `release` Environment makes GitHub create one with no protection rules and no
  secrets. The signing job refuses on the empty secrets before it imports
  anything, and [Before the First
  Release](../releases/publish.md#before-the-first-release) creates the
  Environment first. Until the maintainer reports its settings, nothing in the
  repository shows that its branch rule and reviewer are in place.
- **The first signed release after signing returns is its first run (held for M2
  and W4).** The last command-line release ran F0's signing; M2's or W4's
  signing job first runs for real at the first release after its pull request, and
  nothing before merge can exercise it. The stand-in suites and the shape check cover the flow,
  not Apple's answers. A failure stops that release before its tag.
- **Unprotected tags change the next version.** Any account with write access can
  push, move, or delete a `v*` tag that has no published release, `v0.0.0` and
  `v0.1.1` included, and that changes the version semantic-release works out next.
  Once a release exists after `v0.1.1`, that tag no longer affects the version; before
  one did, moving `v0.1.1` to another commit, or deleting it, could make the next plan
  print `1.0.0`, as [Version Rules](../releases/versions.md#version-rules) records. The check in
  [Each Release](../releases/publish.md#each-release) names such a
  tag only when the check would start at it, and never `v0.1.1`, which it skips.
  Immutable releases lock only the tags of published releases.
- **A weaker install pin.** `--source-ref refs/heads/main` proves that the build
  ran from main, not from a particular tag. `--source-digest` and the reachability
  check still tie the file to one commit on main, but no longer to a version. The
  optional check therefore extracts `marshal-app.exe` from the downloaded installer
  without running it and refuses unless it matches the version's `SHA256SUMS` line
  for the app, so an installer that holds another build of the app is refused by an
  operator who runs the check. It no longer runs the program, so it no longer reads
  a version from it; the build leg's product-version check ties the app to the
  version before it is attested. CI runs the check block's refusals against
  stand-ins and the dry run's installer. The block has not run on axpc.
- **A green run can release nothing.** If main moves after the dispatch, the
  release job logs that the local branch is behind the remote one and ends without
  a tag or a release, after the files are attested.
  [Cutting a Release](../releases/publish.md#cutting-a-release)
  says to check that the release exists after a green run.
- **A partial release.** semantic-release pushes the tag and its
  `refs/notes/semantic-release-<tag>` note before `@semantic-release/github`
  creates the draft release. If the upload fails, the tag still counts as the last
  release, so the next dispatch skips that version. The clean-up is the
  maintainer's, in
  [Cutting a Release](../releases/publish.md#cutting-a-release).
- **Versions follow commit subjects.** A mistitled squash merge releases the wrong
  bump, or none, and no check catches it.
- **Node can come from nodejs.org.** When the `actions/node-versions` manifest
  lacks `24.21.0` or cannot be fetched, `actions/setup-node` downloads Node from
  nodejs.org with no checksum, outside the joint-trust argument in
  [Pinning Convention](./dependency-pins.md#pinning-convention). That Node runs the npm tree with
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
  page then holds. That is a file nothing has checked. Signing the MSI, or making the check required again, is the remedy in a later
  change.
- **A replaced installer can run code.** A replaced MSI could carry a custom action
  that runs code as the user, or ask for elevation behind a prompt the procedure
  has taught operators to pass. An operator who runs the optional check block
  verifies the attestation, which ties the file to `release.yaml`'s build on `main`.
  The genuine MSI's only custom actions that run code are the start of the installed
  app with `AUTOLAUNCHAPP=True` and the deletion of the one Run value at an
  uninstall, and the build leg tests that it has no other.
- **On axpc, code running as the operator can read what the operator can**,
  including the tokens Marshal stores in Credential Manager and any old token files.
  A replaced installer runs as the operator, so it can read them; the per-file access control step in
  [command-line token file permissions](../runner-hosts/registration.md#token-files-of-the-command-line) restricts those files
  to other accounts, not to this one.
- **The MSI refuses a lower version.** That stops an older genuine MSI under a newer
  name from replacing a newer installed version even when the operator skips the
  check.
- **The Windows procedure asks operators to pass prompts for unsigned files.** The
  MSI and the app are not code-signed, so Windows SmartScreen and the
  browser warn or block, and the procedure tells the operator to choose "Run anyway"
  and to keep the download. An operator who has learned to pass those prompts passes
  the same prompt for a replaced file. The prompts are
  described as expected until the first install on each host records them. On axpc, a
  command-line binary fetched with `gh release download` had no `Zone.Identifier`
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211)), and
  SmartScreen did not prompt, with "Check apps and files" On and Smart App Control Off
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511120441)). The installer's procedure removes no mark.
- **Smart App Control On may block the installer.** It blocks unsigned code unless
  its cloud service predicts the file safe, whether or not the file carries a Mark
  of the Web, and whether it evaluates an unsigned `.msi` is not verified. It is Off
  on axpc. Signing the MSI is the remedy in a later change.
- The runner image's definition is embedded in the app, so a runner bump reaches
  a host only through a new release and an update.
