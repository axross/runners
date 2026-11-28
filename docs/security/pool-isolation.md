---
type: Policy
title: "Runner pools, tokens and shared storage"
description: "Determine which jobs and repositories may share a cache, runner group, token or network boundary."
---

# Runner pools, tokens and shared storage

Before routing a job to a pool, identify which repositories can reach its runner group, token, network and persistent volumes. A fresh runner can still inherit data that another job wrote to its cache.

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
  of [Windows Runner Host](../runner-hosts/windows-pools.md#cache-volumes) states each
  volume's reason and writers and owns the Android storage exclusions, which
  this exception does not reach. Any other volume of content a later job
  executes or links, or one that fails a condition, stays a finding.
- A job that holds a deployment secret does not mount a volume that a less
  trusted job could have written.
- A container does not run privileged, does not mount the host's container socket
  or home directory, and has its egress limited where the host's network reaches
  anything sensitive. A justified exception is stated where it is declared;
  the [LAN egress exception](#lan-egress-from-runner-containers-accepted-risk) is one, and [The VM-Local Docker Socket](./vm-isolation.md#the-vm-local-docker-socket)
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
  reads it is not a boundary. The agent reads the token from the operating system's
  store on every registration, one entry per repository, as
  the section The Agent Keeps Tokens in the Operating System's Store
  records. Until W1's installer pull request the command line read it from a file
  restricted to the host user; those token-file rules, and the command line that
  applied them, are at the last command-line release's tag, identified in
  The Last Command-Line Release section of [CLI](../marshal-cli/release-artifacts.md#the-last-command-line-release).
  No code path of the app reads a token file.
- Multiple execution pools for one repository MUST use the same token, the
  repository's one store entry; pools of different repositories never share one.
  Extra pools do not expand the token's repository authority.
- An always-on registration is made with the entry's own token and carries `self-hosted`, the
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
- The CLI's always-on registration request (see [CLI](../marshal-cli/pool-runtime.md#registration)) is
  one HTTPS `POST` to `https://api.github.com`, and the agent's GitHub status reads
  (see [CLI](../marshal-cli/github-status.md#github-status)) are `GET`s there through the same
  client; neither follows a redirect, so a redirect cannot carry the token to
  another host, and a status read follows a next page only on the same origin and
  endpoint path, or GitHub's numeric-id form of it for the same repository or
  organisation. It fails a connection after
  10 s, a request after 30 s, and a response over 1 MiB. The token lives in a type
  with no `Display` and a redacted `Debug`, the header that carries it is marked
  sensitive, and error text has the token replaced before it is logged. Only a
  debug build can move the base address, and only to `http://127.0.0.1:<port>`, for
  the loopback tests; a release build always uses GitHub, which a CI step checks in
  the release profile. Likewise only a debug build can name the `docker` client
  `start` and `build` run, and a release build always runs `docker`. The JIT
  variable is removed from the environment of every `docker` client but the one
  that starts the container.
- Volume names and always-on container names start with the entry's `name`, which
  the operator sets. On-demand names follow the scale-set rules below; volume
  ownership still follows the logical pool. An entry's volumes are mounted only into its own containers. Two entries
  whose names are equal, or where one is the other followed by a hyphen, are
  rejected, because one would claim the other's containers and volumes.
  Stale-container cleanup removes only containers matching the entry's own name
  pattern.
- A repository that can run pull requests from forks on a host's labels MUST
  require approval for outside collaborators' workflow runs. A self-hosted runner
  executes whatever a workflow checks out, so an unapproved fork's run would
  otherwise execute arbitrary code on the host before anyone reviews it. The
  [repository settings procedure](../runner-hosts/registration.md#repository-settings-set-by-hand) gives the operator
  step.
- `docker run` uses `--pull never`, so a missing local image fails instead of
  pulling a same-named public image.

### On-Demand Scale Sets Keep Repository and Host Boundaries

The Windows resident agent uses repository-level runner scale sets, with a
hand-written client on the library's existing HTTP, TLS and JSON dependencies, not
a third-party scale-set crate or a credential-holding helper process. Mac
on-demand pools remain rejected; the existing Linux and native macOS always-on
runtime and VM ownership rules do not change. [On-demand pools](../marshal-cli/on-demand-pools.md)
owns the listener and recovery contract,
[TOML configuration](../marshal-cli/toml-configuration.md#schema-version-1) owns the
configuration, and
[Configure Windows runner pools](../runner-hosts/windows-pools.md#on-demand-pools-in-the-resident-agent)
owns the operator procedure.

- An on-demand pool MUST use only its repository's fine-grained token with
  Administration: Read and write; Actions: read remains optional under the token
  permission rule below. No classic token, GitHub App or organization token is
  used. The scale set uses runner group 1. Finding or creating it, opening its session, obtaining JIT
  configurations, and listing, reconciling and deleting runner registrations MUST
  stay within that pool's repository. Cleanup authority does
  not extend to all runners that the token could delete.
- The scale-set name MUST be the set's only label. Marshal MUST check the sole
  label on both lookup and creation, ignoring case, before opening any session,
  including a recreated session. An empty, extra or different label rejects
  startup without changing or deleting the set. Adding `self-hosted`, the host
  routing label, platform labels or a pool's aliases would let generic workflows
  reach a scale set they did not name; it is not a routing fallback.
- The scale-set name starts with the host's `routing_label` and a hyphen. The
  conservative character, length and collision rules in the CLI schema keep
  ownership unambiguous across the host, even when logical pool names and
  scale-set aliases differ. The runner and job container use
  `<scale-set>-<slot>-<yyyyMMddHHmmssfff>`, with exactly 17 ASCII UTC timestamp
  digits. The runtime MUST bind that identity to the configured logical pool and
  slot for prepare, launch, status, stop and release; an alias MUST NOT become a
  different pool's identity or volume namespace.
- Startup runner cleanup MUST remove only offline runners whose whole name
  matches that pool's scale-set runner-name format. It MUST NOT delete online
  runners or claim another set by prefix alone. Job-container cleanup uses the
  same whole-name rule and retains the configured logical pool's older always-on
  container cleanup under the existing diagnostics rules. Registration cleanup
  after a job, failed launch, abnormal exit or stop MUST use the exact owned
  runner ID and name. An uncertain JIT result is reconciled by that attempt's
  exact name before another registration. Unconfirmed deletion MUST stay visible
  as incomplete cleanup with its non-secret pending identity, not as proof of
  zero registrations. The existing stop deadline MUST NOT be extended.
- Two hosts MUST NOT use one scale-set name for one repository. Startup runner
  cleanup runs before a session can show another listener, so it would remove the
  other host's offline runners. `validate` reads one host's configuration and
  cannot see across hosts, so this is a rule for the operator and for review, not a
  check.
- Tart job VMs retain `marshal-job-<logical-pool>-<slot>-<yyyyMMddHHmmssfff>` and
  their configured-logical-pool cleanup boundary. Scale-set names MUST NOT replace
  logical pool names in VM lookup or cleanup, and golden, build and unrelated VMs
  MUST NOT be admitted by a broader match. Runner and VM timestamps need not be
  equal. This constraint does not enable Mac on-demand operation.
- The repository token and registration-token exchange MUST go only to
  `api.github.com`. Admin and queue credentials MUST go only to the URLs GitHub returns, using HTTPS and a
  host equal to `actions.githubusercontent.com` or ending in
  `.actions.githubusercontent.com`. The suffix MUST match a DNS label boundary:
  `xactions.githubusercontent.com` is not allowed. This admits the service host
  and `broker.actions.githubusercontent.com`, not arbitrary GitHub-related hosts.
  Redirects MUST stay off. Responses retain the 1 MiB limit and the connection
  limit; the broker poll has a separate timeout longer than its 50 s hold. Only a
  debug build may use a loopback fake; a release build MUST ignore that override.
- The registration token, admin token, queue token and JIT configuration MUST be
  held only in the agent's in-memory secret type, never persisted as service
  credentials, returned on the control channel, logged, shown, passed on a command
  line or written to a file or image. The JIT configuration reaches the runner
  through the existing runtime handoff, not a new storage path; the documented
  Docker environment exposure still applies. Only the repository token belongs
  in the operating system's token store. Non-secret exact runner names and IDs
  may be tracked for cleanup; a service URL, session credential or response body
  MUST NOT appear in a view or error.
- Before every scale-set JIT registration the agent MUST reread the token store
  and bind cached service credentials to the token that produced them. Replacement
  invalidates old credentials even before expiry; removal or any store failure
  MUST prevent registration through a cache hit. Already running jobs retain
  their original JIT configuration and lifecycle rules.
- Stop and update drain MUST admit no new work. Session close runs best-effort
  beside the existing stop sequence, within 5 s, and MUST NOT delay its steps.
  Removing or renaming a pool MUST NOT delete its scale set automatically; queued
  workflows and other operators may still refer to it. The operator deletes an
  unwanted set manually after stopping its workflows and listener.

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
- An organisation pool's token is created by an owner of the organisation, has the
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

## Organisation Pools Share a Runner Group's Trust Boundary

An organisation pool's isolation boundary is its named runner group, not one
repository. Every repository the group admits can run jobs on those runners.
Repositories in one group are one trust domain; Marshal does not isolate them
from each other. The organisation token's permissions and cost if leaked are
stated in [pool token permissions](#a-token-carries-only-its-pool-kinds-permissions).

- The group MUST admit selected repositories only and MUST forbid public
  repositories. Marshal refuses the default group, inherited groups, `all` and
  `private` visibility, and `allows_public_repositories: true`. It checks every
  required page before each registration; a failed read never reuses a previous
  permission. A conditional page counts only after an authorised `304`.
- An organisation pool MUST NOT have persistent volumes, since jobs from
  different repositories may use its slots. Its diagnostic bundles may contain
  evidence from any admitted repository and remain subject to the private-sink
  rules in Diagnostic Evidence Stays Private.
- The operator MUST disable fork pull request workflows in private repositories,
  or require their approval, for every private repository the group admits.
  Marshal changes no group, repository or organisation policy.
- An organisation pool and another pool for that organisation or one of its
  repositories MUST NOT share a custom label when their OS labels match.
  An on-demand pool's only label is its scale-set name, so it shares a label with
  an organisation pool only when that pool lists the name, never through the
  default routing label or the platform labels. Validation checks pools on this
  host. Across hosts this remains an operator and review rule, since no host
  knows another host's configuration.
- A registered idle runner may wait indefinitely for a job. The group can be
  opened, or the runner moved to another group, after a successful check and
  before that job arrives. Marshal neither deletes such runners nor polls group
  settings outside registration. The risk is acceptable only while the people who
  can change a group are trusted: the organisation's owners, members holding the
  CI/CD admin role or a role that manages runners, and holders of the organisation
  token.
- A program running as the host's user can read that user's Credential Manager
  entry or request the Keychain item. Reading an organisation token reaches the
  whole organisation rather than one repository. The token is present on each
  host that serves or has served its organisation: removing the last pool for a
  login neither removes its token nor lists it in `repositories_released`, and the
  window then shows no row for it. The same-user store and channel risks also
  apply to organisation keys; the agent accepts any well-formed login, not just
  those currently configured.

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
[Windows Runner Host](../runner-hosts/windows-pools.md#automatic-cpu-affinity).
CPU policy MUST NOT infer workload roles from names, tags, or labels, or alter
consumer build flags, global WSL allocation, or Windows process affinity.

## LAN Egress From Runner Containers (Accepted Risk)

The Windows host starts containers on Docker Desktop's default network, which
routes to the machine's local network as any process on the machine does. A job
can reach devices there, such as a router or a NAS, that a GitHub-hosted runner
cannot. This is an exception to the [runner isolation rule](#shared-runner-storage-is-a-cache-poisoning-surface) that egress is limited where the
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
