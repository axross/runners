---
type: Definition
title: Project glossary
description: Names used by runner operators, Marshal clients and contributors, with routes to the contracts that define their behavior.
---

# Glossary

A runner executes a GitHub Actions job. A resident agent supervises runners.
An agent host serves an AI coding session. Use these distinctions when reading
operator procedures, UI labels or change records. The [naming convention](./conventions/documentation.md#project-names)
explains how contributors choose new terms.

## Runners

### Pools and registration

**Runner** — A GitHub Actions self-hosted runner process that takes and executes a workflow job. An **Ephemeral Runner** registers for one job and is then removed. Marshal uses this lifecycle. Persistent volumes have their own lifetime. [Runner registration](./marshal-cli/pool-runtime.md#registration).

**JIT Config** — The single-use encoded credential GitHub issues when registering a runner for a repository or an organisation’s runner group. Marshal passes it to the new runner at start. It stays out of the repository. [Registration](./marshal-cli/pool-runtime.md#registration).

**Runner Host** — The machine that builds or stores a runner’s image and starts containers or VMs for its jobs. The **Host Profile** selects platform rules, path forms and labels. The two profiles are `axpc` (Windows with Docker Desktop) and `axmc` (macOS with Tart). [Host profiles](./marshal-cli/json-validation.md#profiles).

**Host Configuration** — The machine-local configuration outside the repository. Its image and pools are validated for the host profile. The agent uses TOML; legacy hosts and migration also have a JSON representation. [TOML and legacy configuration](./marshal-cli/toml-configuration.md).

**Execution Pool** — A configured group of **Slots** sharing a registration target, routing labels and a resource namespace. Each **Slot** is one concurrent runner position. In an always-on pool, when a job ends the slot starts a fresh runner; in an **On-Demand Pool**, a slot starts a runner only for demand a **Runner Scale Set** reports, up to the pool's `runners`. [Pool resources and slot lifecycle](./marshal-cli/pool-runtime.md#names-and-the-slot-loop).

**Runner Scale Set** — A repository-level GitHub Actions routing identity with a name and a message session that supplies job demand to an **On-Demand Pool**. [Listening and admission](./marshal-cli/on-demand-pools.md#listening-and-admission).

**On-Demand Pool** — An **Execution Pool** with the optional TOML `scale_set` setting, whose **Resident Agent** starts **Ephemeral Runners** in response to demand from a **Runner Scale Set**. An **Organisation Pool** is never on-demand. [On-demand pool contract](./marshal-cli/on-demand-pools.md).

**Target Repository** — The GitHub repository served by a repository pool. Its pools use the same repository-scoped token. Foreground configurations may name a token file; the resident agent uses the OS credential store. [Token scope](./security/pool-isolation.md#a-token-carries-only-its-pool-kinds-permissions).

**Organisation Pool** — An execution pool whose TOML `organization` and `runner_group` select a **Runner Group**. This GitHub group admits repositories to runners. Marshal requires a selected-repositories, non-default, non-inherited group with public access disabled. Its pools use the organisation token and have no persistent volumes. [Runner group trust](./security/pool-isolation.md#organisation-pools-share-a-runner-groups-trust-boundary).

**Routing Label** — A `runs-on` label selecting a pool. For an always-on pool, in JSON, missing or empty host `labels` falls back to the host label; an explicit list replaces it. TOML requires `routing_label`. A **Custom Label** adds a pool-specific choice alongside the runner’s platform labels. [Label selection](./marshal-cli/toml-configuration.md#schema-version-1).

**Runner Image** — The container image carrying the runner and job tools, built from a Dockerfile under `images/<name>/`. Its **Image Contract Version** is the positive integer in `com.github.axross.runners.contract`. The CLI compares it with the embedded Dockerfile’s contract before accepting the launcher, arguments and labels. [Image compatibility](./marshal-cli/runner-images.md#the-image-contract).

**Shared Volume** — Storage mounted by successive runner containers, such as a package-cache volume. An ephemeral runner can leave data there for a later job. [Shared-storage threat model](./security/pool-isolation.md#shared-runner-storage-is-a-cache-poisoning-surface).

**Shared Fixtures** — Configuration inputs under `tests/host-configuration/` used by the CLI tests. Their `expectations.json` records the required result for each input. [Configuration fixture contract](./marshal-cli/configuration-fixtures.md#fixtures-and-tests).

### Marshal and the agent

**CLI** — The `marshal-cli` library under `cli/`, named for the historic `marshal` command. Its `validate`, `build` and `start` implement validation, image builds and foreground supervision. Current main exposes the library to the app and test helpers. Recovery builds the historical CLI from a fixed tree. [Library entry points](./marshal-cli/entry-points.md).

**Supervisor** — The host process that keeps slots filled by requesting JIT credentials and starting a fresh runner after each job. This is the resident agent in Marshal or the foreground `start` in command-line releases. [Registration and the slot loop](./marshal-cli/pool-runtime.md#names-and-the-slot-loop).

**Resident Agent** — The long-running supervisor controlled over the local channel. It starts and stops runners, builds images and can remain up after a configuration, image or pool failure. It shares the foreground instance’s lock, channel and log directory, so they cannot run together. [Resident agent lifecycle](./marshal-cli/resident-agent.md#the-agent).

**Marshal App** — The desktop client and its executable under `app/`. On Windows the notification-area menu opens the main window. The executable’s `agent` mode runs the resident agent as a separate process that outlives the window. The client uses protocol version 2; the agent owns configuration, credentials and runner operations. [Executable and process roles](./marshal/session.md#the-executable-and-its-modes).

**Instance State** — The foreground instance’s running, stopping, stopped-with-reason or resuming state, or `building` when a build holds the lock. It determines which requests the instance accepts. [Foreground state and requests](./marshal-cli/foreground-instance.md#instance-state).

**Runtime Core** — The part of `start` that prepares pools and runs each slot loop until stopped. The instance can restart the core after failure. The **Runtime Seam** separates registration, backoff and stopping from the chosen engine’s readiness, images, execution, cleanup and diagnostic export. [Runtime responsibilities](./marshal-cli/pool-runtime.md#runtime-seam).

**Token Store** — The resident agent’s credential store. Keys are lower-case `owner/name` for a repository and the lower-case login for an organisation. Windows uses locally persistent Credential Manager entries; macOS uses the login keychain. On Linux the agent skips pools because there is no store. [Host token store](./marshal-cli/token-store.md#the-token-store).

**Protocol Version 2** — The agent’s line-based JSON protocol. The first line must be `hello` before any request is accepted. It defines request/response messages, subscribed `state` and `github` topics, heartbeats and `ending`. Version 1 requests cannot bypass the handshake. [Protocol messages](./marshal-cli/agent-protocol.md#protocol-version-2).

**Snapshot** — The local state returned by `status` and the `state` topic. It includes state and reason, pools, runners, images and stop residue, without credentials. Its serialized size fits the 64 KiB message budget including its envelope. [Snapshot fields and truncation](./marshal-cli/agent-protocol.md#the-snapshot).

**GitHub State** — The remote runner list’s offline, idle, busy or unlisted state, with the busy job when known. It is distinct from the Snapshot. The agent polls only for eligible registrations while a client subscribes to `github`. [Remote polling](./marshal-cli/github-status.md#github-status).

**Request Budget** — Remaining hourly GitHub requests, read from `x-ratelimit-*` headers on status reads. Polling slows at or below 25% remaining and pauses at or below 10% or when GitHub reports that the limit has been reached. Registration consumes the same budget without supplying these observations. [Polling limits](./marshal-cli/github-status.md#github-status).

**Diagnostic Container** — A container whose collector is enabled, with the `runners.diagnostic-lifecycle=1` label and without `--rm`. It retains its original settings after a job until the supervisor exports evidence to the pool’s private sink and removes it. [Diagnostic finalization](./marshal-cli/diagnostics.md#diagnostics).

### Virtual machines on axmc

**Golden VM** — The Tart Ubuntu template built by `build --profile axmc` for Linux job VMs. It contains Docker Engine, the launcher and the runner image. Its name identifies the golden VM version, image contract, runner version and build time without booting it; only `build` deletes it. [Linux VM contract](./runner-hosts/linux-vm.md#the-golden-vm-contract).

**Golden macOS VM** — The template built for a configured macOS pool from the digest-pinned Cirrus Labs base. It contains the runner and launcher directly and names them with `marshal-golden-macos-g<golden VM version>-r<runner version>-<time>`. [Golden macOS build](./marshal-cli/macos-pools.md#the-golden-macos-vm-build).

**Job VM** — A per-job clone named `marshal-job-<pool>-<slot>-<time>`. A slot boots it to wait for work. It uses Softnet, has no shared folder, clipboard, audio, USB accessories or Rosetta and is deleted after its job. Linux and macOS pools select different golden VMs. [Per-job isolation](./security/vm-isolation.md).

**Build VM** — The temporary `marshal-build-linux-<time>` or `marshal-build-macos-<time>` used to make a new golden VM. The rename commits the new golden VM. After an interrupted build, cleanup can fall to the shim or the next build, and the outcome of an interrupted rename can be unknown. [Interrupted builds and cleanup](./marshal-cli/build-images.md#stop-and-failure).

**macOS VM Limit** — The two concurrent macOS VMs allowed on a Mac by Apple’s licence. Marshal counts a macOS build VM in any state, including when it is stopped. Linux build VMs are excluded. A macOS slot checks the count before cloning and waits when both places are occupied. [Capacity and waiting](./marshal-cli/macos-pools.md#the-two-vm-count).

**VM-Local Docker Socket** — The `/var/run/docker.sock` of the Docker Engine inside a Linux Job VM. From Image Contract Version 2 the runner container may use that VM’s socket. The VM serves one job and is then deleted; axpc mounts no host socket. [Docker socket boundary](./security/vm-isolation.md#the-vm-local-docker-socket).

**Softnet** — Tart’s network filter. Job VMs can reach globally routable IPv4 destinations and their gateway for DNS only. Marshal also blocks multicast. Marshal checks Softnet’s SUID bit before every boot; the operator sets it. [Softnet setup](./runner-hosts/mac-setup.md#setting-up-softnet).

**Primary Network** — On macOS, the interface holding the IPv4 default route, its router and the router’s hardware address. The instance records this when it starts running. Two consecutive readings that differ from the recorded network stop every runner. User-facing state names interfaces without disclosing network names or addresses. [Network-change stop conditions](./runner-hosts/mac-resume.md#what-stops-every-runner).

### Release names

**Marshal Release** — A `v<version>` tag on main and its GitHub release. The maintainer dispatches `release.yaml`; semantic-release chooses the version from commit subjects since the previous `v*` tag. The active recovery route publishes the historical CLI binaries, MSI, macOS pkg and checksums. The retained app release set has its own restoration conditions. [Active and suspended release sets](./security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main).

**Marshal Installer** — In the app installation procedure, the unsigned, per-user Windows MSI containing `marshal-app.exe`, its licence notices and a Start menu entry. Its version is the release’s `X.Y.Z` or `0.0.0` for a dry run. The historical CLI MSI belongs to the recovery procedure. [App installer](./marshal-cli/install-on-windows.md#installing).

**Check Block** — An optional download-verification block. The app MSI check verifies checksums, the tag commit on main, attestation and the app inside the MSI without installing anything. A failure reports `refused:` and its cause. Use the recovery instructions for a CLI artifact. [App download checks](./marshal-cli/install-on-windows.md#checking-the-download).

**Release Environment** — The GitHub Environment `release`, which holds macOS signing secrets and `APPLE_TEAM_ID`. It admits main and requires the maintainer’s approval for each run. In active recovery only `sign-macos` declares it. The maintainer controls its settings. [Signing authority](./security/release-authority.md#temporary-cli-recovery).

**Signing Identifier** — The identifier `app.axross.marshal` used for the macOS code object and historical pkg. The fixed identifier and team give the designated requirement continuity across releases, including the trust decision used for keychain items. [Signing contracts](./marshal-cli/signing.md#the-signing-contracts).

**Placeholder Version** — The `0.0.0-dev` in the CLI and app manifests and lockfiles on main. The release build writes the version into its runner checkout; the stamp is not committed. [Release versioning](./marshal-cli/release-artifacts.md#releases).

## Agent Hosts

**Agent Host** — A planned long-running environment for reaching an AI coding agent from another device. Its proposed WSL 2 configurations are `agent-hosts/{claude-code,amp}/`. **Remote Control** is Claude Code’s `claude remote-control` mode. An **Amp Runner** waits for work using `amp --no-tui --runner-id`. The **Authoring Host** in a change record identifies the product used to write the change. [Repository layout and planned paths](./conventions/directory-structure.md#the-layout).

## Change Loop

**Authoring Host** — Claude Code, Codex, Amp or “manual” when no agent is involved. It selects the post-push review provider and is recorded with that provider so another session can resume the run. [Selecting review](./agent-work/independent-review.md#the-post-push-review).

**Standing Delivery Grant** — The maintainer’s adopted authorization for a named set of delivery effects. Every run holds it within its specified targets, conditions and exclusions. [Grant scope and adoption](./agent-work/delivery-grant.md#authorizing-delivery-operations).

**Tracking Issue** — The GitHub issue whose body holds the change plan. The plan gate is the maintainer’s approval of that plan. [Change loop](./agent-work/change-delivery.md#the-change-loop).

**Plan-Gate Delivery Confirmation** — The answer a Claude Code session collects with each plan-approval question about delivering the draft pull request, requesting Claude CI review each round and watching the pull request for activity. [Claude Code delivery confirmation](./agent-work/host-permissions.md#confirming-delivery-at-the-plan-gate-in-claude-code).

**Amp Review Child Thread** — A fresh Amp thread for one post-push review round. It posts directly on the pull request. Archiving depends on the plan-gate archive confirmation. [Amp review](./agent-work/amp-review.md#amp-review-child-thread-from-amp).

**Claude CI Review** — The post-push review a Claude Code run requests each round by posting the Claude review trigger phrase in one top-level pull request comment. The `claude-review.yaml` workflow runs on a GitHub-hosted runner and posts the result as `claude[bot]`. [Requesting Claude CI review](./agent-work/claude-ci-review.md#claude-ci-review-from-claude-code).

**Plan-Gate Archive Confirmation** — The human’s answer on whether an Amp run archives its review child threads. Collect it at the plan gate or before archiving when the run lacks a confirmation, including on resume. [Amp archive confirmation](./agent-work/host-permissions.md#confirming-review-thread-cleanup-at-the-plan-gate-in-amp).

**Run State** — The recoverable record that lets a fresh session resume a change loop at its pending phase. [Resume checkpoints](./agent-work/run-state.md#retrieve-state-on-resume).

**Review Coverage Evidence** — The distinct facts a run can establish about the policy available, input consumed by the post-push reviewer, findings reported and result published. [Coverage and limits](./agent-work/independent-review.md#review-coverage-evidence).

**Branch Naming Rule** — A working branch identifies the issue that sourced the change or marks that it has none. The name identifies the origin of the change independently of the authoring host. [Branch convention](./agent-work/branches.md#branches-and-merging).

**Agent Marker** — The `<!-- ai-agent -->` comment marking an issue or comment written by an agent. The retired marker `<!-- agent -->` identifies historical material only. [Agent working agreement](../AGENTS.md).

**Installed Skill** — A directory copied from `axross/skills` into `.claude/skills/`. The `skills-lock.json` lockfile records each installed skill’s source commit and content hash. The refresh command takes the latest upstream when run. Updates are manual, and installed copies are never hand-edited. [Skill installation](./agent-work/skill-installation.md#install-and-refresh).
