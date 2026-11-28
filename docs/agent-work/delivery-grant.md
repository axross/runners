---
type: Policy
title: Standing delivery grant
description: Identify the exact authorized targets, effects, costs, exclusions and adoption evidence for agent delivery.
---

# Standing delivery grant

## Authorizing delivery operations

The maintainer, @axross, issues this **standing delivery grant** once, here, to
every agent run in this repository. It is the operation grant that
`loop-engineering` and `github-operation` require before an external effect:
actual human authorization that names its targets, its effects, its lifetime,
and its exclusions, rather than an inference from project policy. Its
human-evidence locator is the act that adopted it: @axross's merge of the pull
request that closes
[issue #1](https://github.com/axross/runners/issues/1), which added this
section following the plan approved in that issue. An amendment to this section
is adopted the same way, by the maintainer's merge of the pull request that
carries it, and takes effect only on that merge; the run on the amending pull
request works under the grant as merged. GitHub records that merge, and the
amendment's tracking issue records its plan approval. Agent runs share the
maintainer's operator identity, so the signal that distinguishes such a merge as
a human act is that agents never merge on their own: merge is excluded from this
grant, and it is the maintainer's decision, per
[Branches and Merging](./branches.md#branches-and-merging). The grant is separate from plan
approval, which releases implementation and is not what authorizes a delivery
effect, but every run already holds it, so no run asks for it.

The grant's targets are:

- this repository
- the run's working branch named under [Branches and Merging](./branches.md#branches-and-merging); for
  recovery, the existing pull request's head branch when it follows that naming
  rule, for pushes only
- the run's tracking issue, if any, and the execution-plan issues explicitly
  linked to it for the approved plan's slices; review-only recovery can have
  none, while fixes require a linked issue and an approved plan
- the run's draft pull request against the default branch; for recovery, the
  existing pull request
- the review provider the authoring host maps to in
  [the post-push review](./independent-review.md#the-post-push-review), which for Claude Code is
  [the Claude CI review](./claude-ci-review.md#claude-ci-review-from-claude-code)
- the Amp review child threads the run creates for that pull request, each
  identified in run state

Its covered effects, each performed without asking, are:

- creation of the tracking issue, publication and revision of the plan in its
  body, and status comments and labels on it
- closing the run's tracking issue and its linked execution-plan issues as
  completed once the conditions in [Tracking Issue Completion](./change-delivery.md#tracking-issue-completion) are
  satisfied, preserving canonical plan content and assignees; a parent issue is
  included only when its entire scope is complete, and a human instruction to
  keep an issue open excludes that closure
- append-only pushes of the working branch, including review fixes
- creation of the draft pull request for new delivery
- addition of the authenticated operator as an assignee on the run's newly
  created tracking issue and draft pull request, and read-back of their
  assignees; this covers additions that preserve existing assignees, not
  removal, assignment of other people, or changes to unrelated existing targets
- conversion of a previously ready pull request back to draft for new changes
- maintenance of that pull request's description throughout the review loop,
  including approach changes
- replies to and resolution of that pull request's review threads when
  addressing findings
- publication of evidence at the destination named in the approved plan when an
  acceptance criterion carries the
  `(verified out of tree: <where the evidence will be published>)` marker
- review requests on that pull request through the mapped provider, which for a
  Claude Code run is the Claude CI review, one per round, including fresh
  reviews after fixes, up to the four-round cap, with the consequences listed
  below
- for an Amp run, which the Amp review child thread route serves: creation of
  one review child thread per round, that child's COMMENT-type review on the
  run's pull request, and archiving of the child thread once its result is
  verified or its round has failed

The grant is standing: it holds across runs, sessions, and executors until the
maintainer amends or removes this section, and no run's own end expires it.
Rounds beyond the fourth are not covered; they need a human direction stating
how many additional fix-and-review rounds it allows, and that direction covers
only those rounds. The ready transition is not a grant-gated effect: it follows
the post-push review gate, and a run that has satisfied that gate needs no
grant to flip its pull request to ready.

**Guidelines:**

- MUST perform a covered effect without asking the human for permission, before
  or after plan approval, and MUST NOT report a run as authorization-waiting on
  a covered effect. Meeting a host's own condition on such an effect, as the
  host-boundary guideline below describes, is not asking for permission.
- MUST still obtain plan approval before implementation and the post-push
  review before ready; the grant releases neither gate.
- MUST honor a host permission prompt, refusal, instruction, or tool usage
  condition that requires approval for or forbids a covered effect as the
  host's boundary: report it, and never work around it or try another route. The
  grant is the maintainer's authorization, not a change to what a host allows.
  Claude Code's explicit-request condition on pull request creation is one such
  condition, and the Confirming Delivery at the Plan Gate in Claude Code section
  of [Agent Sessions](./host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code) describes how a run meets it.
- MUST NOT read the grant as covering an effect it does not name, or a target it
  does not list.
- MUST read the grant as it stands on the default branch. A change to this
  section takes effect only once the maintainer merges it; a run on a branch
  that edits the grant keeps working under the merged text.

Creating the draft pull request and pushing fixes while it is open trigger
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml). It runs on a
GitHub-hosted runner with `contents: read`, checks out the pull request, and runs
`mise run check`, which executes the pull request's own task definitions and the
linters they call, including the CLI's Rust gates and, for the app under `app/`, its
formatting, the lint and tests of its platform-independent crates, its cargo-deny
run, the main window page's Node tests, a check that its checked-in licence notices
match `app/Cargo.lock`, a check
that both Rust toolchain files name one channel and that `app/tauri-cli.version`
holds one exact version, the test of the script that reads that version
(`test:tauri-cli-version`), and `check:app-lock`, which fails when the app's
lockfile resolves a crate `marshal-cli` builds with to another version than the
CLI's. Before that run, the job prepares the editor rendering gate with
`mise run prepare:app-render` and installs Chromium's OS libraries on its disposable
runner. After it, the job runs `mise run test:app-render`, which `check` does not
include, and keeps the gate's screenshots and measurements as the `editor-renders`
artifact for 14 days, whether or not the gate passed. The same workflow also runs a
`rust` job that formats, lints, and tests the CLI on Linux, Windows, and macOS hosted
runners, with four more steps on Linux only: the release-profile tests of the
debug-only overrides, the test definition `build` reads among them; a build of the
runner image's base stage on that runner (never pushed); tests that run a container
through the hosted runner's Docker daemon against that image, and export a diagnostic
container's evidence through the CLI's exporter child from a real `docker cp` stream
into a private temporary sink; and a test that runs the library's `build`, through the
test-only `commands-helper`, against the same
daemon against the debug-only test definition in `cli/tests/build-definition/`, checks
the image's five labels, that no temporary tag remains, and that the CLI's image check
finds the image matching and an absent tag missing, and removes the image. On Linux
and macOS a further step holds the CLI's single-instance lock with a debug-only test
helper and runs a copy of the helper as `nobody` through the hosted runner's
passwordless `sudo`, failing unless that other user is refused the lock. On Windows a
further step has the helper open the control pipe and reads the live pipe's security
descriptor back with PowerShell 7, failing unless it grants the current user alone.
Neither step needs a new action, permission, or secret. An `app-modes` job on a macOS
hosted runner holds `contents: read` and runs clippy and the tests of the app's
mode-dispatch crate, which covers the dispatch of the Unix shims there, then runs
that crate's tests in the release profile, without building the web view, and an `app` job on a Windows
hosted runner holds `contents: read` and builds the app's whole workspace: it
installs `tauri-cli` with `cargo install --locked` at the version in
`app/tauri-cli.version`, the one place that pin is set, lints and tests every crate,
runs `cargo tauri build --no-bundle`, fails if the build changed a tracked file or
left a file that is neither tracked nor ignored, and keeps the built
`marshal-app.exe` as the run's artifact. An `app-macos` job on a macOS hosted runner
holds `contents: read` and lints and tests every crate of the workspace with the
`fake-engine` feature. None of the app jobs uses a cache action, so `tauri-cli`
compiles on every run, and the `app` job's timeout is 45 minutes;
[App](../marshal/building.md#building-the-app) records how it was set.
The workflow also builds the
runner image for linux/arm64 on the hosted `ubuntu-24.04-arm` runner, in a job of its
own that builds the image, runs the same smoke test, which expects no Android toolchain
on arm64, and never pushes ([#74](https://github.com/axross/runners/issues/74)). Its release-build job is the
release build's dry run: it calls
[`release-build.yaml`](../../.github/workflows/release-build.yaml), which builds the
Windows app on a hosted `windows-latest` runner with `tauri-cli` at its pin, runs the
release-profile tests, checks that the app's product version is the stamped version
and that it imports no `vcruntime` DLL, builds and tests Marshal's per-user MSI with
the real `msiexec`, and uploads the app executable, the MSI and `SHA256SUMS` as one
artifact of the run. Its `app-macos` job, on a macOS hosted runner with `contents: read`
and a 45-minute timeout, installs `tauri-cli` the same way, runs the location rule's
tests in the release profile, bundles the unsigned `Marshal.app` at bundle version
`0.0.0`, fails as the `app` job does if the build changed a tracked file, checks the
bundle's layout, and builds an unsigned disk image with `hdiutil`. It keeps the app's
`ditto` archive and the disk image as the `marshal-macos-app` artifact, outside the
unsigned set, and hands the archive's SHA-256 over as the workflow output
`app_sha256`. The dry run signs nothing and references no Environment, and no
app-build job declares one. The temporary CLI recovery has its own unsigned dry
run; only its dispatch-only signing job declares the `release` Environment.
The app dry run holds `contents: read`, and nothing is attested or published; [CLI](../marshal-cli/release-artifacts.md#releases)
describes the build. Its `Install Blocks` job runs on Windows, needs that dry run,
and holds `contents: read`. It downloads the dry run's artifact and runs the check and
uninstall blocks that [CLI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back) documents:
against stand-ins, under Windows PowerShell 5.1 and PowerShell 7, and then against
the dry run's real MSI with only `gh` stubbed. A standard local user without
administrator rights also runs the check block, installs the MSI and runs the
uninstall block. A pull request from a fork runs without
repository secrets.
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) checks the
CLI's locked crates and, in a second step, the app's, against the RustSec advisory
database every week, on manual dispatch, and on a pull request that changes
`cli/Cargo.lock`, `cli/deny.toml`, `app/Cargo.lock`, `app/deny.toml`, or the workflow
itself. It needs the network and moves with the outside world, so it is not
part of `mise run check` and is not a required check: a new advisory turns it red and
the maintainer triages it. Codex review may:

- incur charges
- read the pull request and repository content, including files outside the diff
- publish findings and a review to that pull request

A [Claude CI review](./claude-ci-review.md#claude-ci-review-from-claude-code) request, which is a Claude Code run's mapped provider, can:

- start the GitHub-hosted `claude-review.yaml` job, which runs a third-party
  artificial intelligence action with repository-reading and shell tools
- let the Anthropic action read pull request content and fetch its review
  plugin from a marketplace that is not pinned to a commit, an accepted risk that
  [Security](../security/workflow-permissions.md#the-review-plugin-marketplace-is-not-pinned-accepted-risk)
  records
- pass the repository secret `CLAUDE_CODE_OAUTH_TOKEN` to the Anthropic action
- let the action request a GitHub OpenID Connect identity token through the
  job's `id-token: write` permission
- give the job's token only `contents: read` and `id-token: write`; the action
  publishes findings with a Claude GitHub App token it obtains by exchanging the
  OpenID Connect token, so the job holds no pull request, issue, or check write
  scope
- incur charges through the Anthropic action
- publish findings to the pull request through the Anthropic action, with the
  permissions of the installed Claude GitHub App

An Amp review child thread can:

- incur Amp usage charges
- read the repository, the pull request, and files outside the diff through
  the executor and GitHub access the child thread is given
- hold the same GitHub access as the authoring run, including its write
  access, and the Amp tools and connected services the authoring user's threads
  hold, including starting other threads, so the assignment's no-edit,
  no-push, and no-delegation limits bind only as far as the host enforces them
- be steered within those tools by pull request content it reads, such as a
  comment any GitHub user can post on this public repository, as Security
  records for the Claude review
- publish a COMMENT-type review on the pull request under the same GitHub
  operator account as the run that authored the change

An Amp review child thread's review supplies no GitHub approval and no APPROVE
verdict; a shared operator account is not a distinct bot identity. The selected
route supplies context-independent review for this project's delivery gate. This
grant covers neither unrelated threads or sessions nor reconfiguration to make
an unavailable route work.

These are the consequences the maintainer accepted in issuing the grant. They document
that acceptance; an agent does not re-disclose them or ask about them before a
covered effect.

The grant does not cover, and each of these needs an explicit human
instruction:

- a push to the default branch
- force-push
- merge
- release and deployment
- setup, settings, or secret changes, including adding the
  `CLAUDE_CODE_OAUTH_TOKEN` repository secret
- other delivery targets or review providers, including a review session started
  by any session other than the run's own
- a push to a head branch that does not follow the naming rule in [Branches and Merging](./branches.md#branches-and-merging); on an existing pull request with such a head, the grant covers
  review requests and comments but no push

For Amp's and Codex's host-permission boundaries, follow
[Agent Sessions](./host-permissions.md#amp-delivery-authorization).
