# Security

This project's supply-chain and runner-trust conventions: how a `uses:` reference
under [`.github/`](../../.github) is pinned, what a workflow may be granted, where
a self-hosted runner may and may not run, what a runner's shared storage exposes,
and what is never committed. It does not cover application-level security, such as
input validation or the OWASP-style concerns the installed `application-security`
capability owns.

The runner image under [`images/actions-runner/`](../../images/actions-runner) and
the Windows host scripts under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) exist and are
reviewed against the sections on runner trust and shared storage below. The
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
`.mise/locks/` together.

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
`deny.toml` lists. The HTTPS client's TLS
stack also needs the `ISC` and `BSD-3-Clause` licences on the allowlist, and the
Windows pipe crate needs `0BSD`; the maintainer approved all three, and [CLI](../operations/cli.md#dependencies) records why
each crate is there. `rustup` itself is
installed by [`.agents/install-rust`](../../.agents/install-rust) from a pinned
`rustup-init` release for `x86_64-unknown-linux-gnu`, refused unless its SHA-256
matches. The release and its hash are refreshed by hand together, the hash
computed from the real download, and are not covered by Dependabot. Dependabot
does cover the CLI's own dependencies: its `cargo` and `rust-toolchain` entries
for `/cli` propose the crate and toolchain refreshes as pull requests.

The Rust toolchain is the one download in this repository that is not
checksum-pinned, and the maintainer accepted that exception when approving the
CLI's plan. `rustup` fetches a channel manifest over HTTPS and checks each
component's hash against that manifest, but it does not verify the manifest's
signature, so the trust rests on the HTTPS connection to the Rust project's
distribution server. Recording the manifest's hash here would need a refresh for
every release without adding a signature check. A step that installs the toolchain
`cli/rust-toolchain.toml` names through `rustup` is this recorded exception; any
other unpinned download remains a finding.

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
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) and
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) hold only
`contents: read`. [`claude-review.yaml`](../../.github/workflows/claude-review.yaml)
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
- a ruleset blocking direct pushes to the default branch, which the maintainer is
  adding and which is outside this repository's files;
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
  the LAN egress section below is one.
- Each shared mount is declared with the reason it exists and the trust level of
  every job that can write to it.

## Per-Repository Isolation on a Runner Host

A runner host serves several repositories from one machine, so each repository's
registration, storage, and containers are kept apart from every other's. The rules
for [`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) and any
later host:

- One token per target repository, a fine-grained personal access token limited to
  that repository's Administration permission. A token MUST NOT cover several
  repositories or be stored as a GitHub Actions secret. A token that covers
  several repositories, or a classic token, would let one leak register runners
  for all of them, and a secret that can register runners for the automation that
  reads it is not a boundary. The token is read from a file on the host on every
  registration, and the file is restricted to the host user. On macOS the CLI
  refuses a token file that is not a regular file owned by the current user with
  mode `0600`, and skips its pool with a warning. On Windows the CLI does not check
  the token file's access control list, because no safe call reads it, so the
  restriction rests on the operator's setup: today the Task Scheduler setup
  restricts each file, and an operator procedure that replaces that setup has to
  keep an explicit step for it.
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
  one HTTPS `POST` to `https://api.github.com` that follows no redirect, so a
  redirect cannot carry the token to another host. It fails a connection after
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
- No request carries a configuration path for the instance to read, and no
  response carries a token or a JIT configuration. A resume names a path only so
  the instance can compare it with the one it started with.
- Only a debug build can move the lock, the channel, the log directory, or their
  roots, for the tests; a release build always uses the host's fixed locations,
  and a CI step checks in the release profile that it ignores the request. The
  `instance-helper` binary that the tests and CI run does nothing in a release
  build.

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
  liveness pipe from `start` closes, and kills its own `docker` clients first. Its
  clients run in process groups of their own with the JIT variable cleared, and on
  Windows the child is also in `start`'s Job Object. `start` closes the pipe at the
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
  its pipe closing, cannot kill its `docker` clients. On Unix they run until each
  ends by itself; on Windows the Job Object ends them when `start` ends. They carry
  no JIT configuration.

## Diagnostic Evidence Stays Private

A diagnostic container's bundle holds what the collector observed during a job, and
the job ran code from the repository. [CLI](../operations/cli.md#diagnostics) owns
how a bundle is written.

- The sink is one the operator created: the CLI never creates, repairs, or relaxes
  it. It must lie outside the checkout, and neither it nor a directory above it may
  be a link or a Windows reparse point. On Unix it must be owned by the current
  user with mode `0700`, and the CLI creates each bundle `0700` and each file `0600`.
- On Windows the CLI does not check the sink's owner or access rules, because no
  safe call reads them; a bundle inherits the sink's rules. The PowerShell exporter
  checks them, so the operator's ACL step in
  [Windows Runner Host](../operations/windows-runner-host.md#collecting-private-diagnostics)
  is the only protection left once Phase 4c deletes the PowerShell scripts. A sink
  whose rules admit another user would expose the evidence to that user. This is
  accepted on a single-user host, with the ACL step kept in the procedure.
- The sink is never mounted into a job. The container's evidence leaves it only
  through the identity allowlist, which never reads the environment, and an archive
  reader that accepts only the collector's regular files, by name, within fixed
  sizes, so a job cannot write a link, a path outside its bundle, or more than
  40 MiB.
- A bundle is untrusted evidence. The CLI never prints or logs its contents, and a
  warning names only the pool, the container, and the gap.
- A labelled container is removed only by a finalisation, which tries its export
  first, or by the manual recovery, so a later `start` with the pool's diagnostics
  off does not destroy its evidence.

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
a parameter.

[`.gitignore`](../../.gitignore) excludes `settings.local.json`, `.env.local`,
token files, and private keys so that a local working file cannot be committed by
accident; it is a backstop, not the control. The only repository secret the
workflows read is `CLAUDE_CODE_OAUTH_TOKEN`, used by `claude-review.yaml` and
added by the maintainer. A secret value is never echoed, passed on a command line,
or written into an image layer or build cache.

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
