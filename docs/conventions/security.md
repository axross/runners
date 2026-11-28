---
type: Guide
title: "Security"
description: "Find the policy for an existing security topic by its established name."
---

# Security

Use the [security boundaries index](../security/index.md) to choose a control by trust boundary. These links also preserve the published section names. For [runner images](../../images/actions-runner/) and [Windows host configuration](../../hosts/windows-docker-desktop/), apply the runner trust and storage rules. The per-job VM boundaries also cover the macOS host ([implementation issue](https://github.com/axross/runners/issues/74)). The planned agent-host configurations will be reviewed against the same applicable constraints. General application security is covered by the [Application Security skill](../../.claude/skills/application-security/SKILL.md).

## Pinning Convention

[Pinning tools, actions and app dependencies](../security/dependency-pins.md#pinning-convention).

## The App's Dependencies

[Pinning tools, actions and app dependencies](../security/dependency-pins.md#the-apps-dependencies).

## `actions/*` Stays on Major Tags

[Pinning tools, actions and app dependencies](../security/dependency-pins.md#actions-stays-on-major-tags).

## Workflow Permissions Start From Read-Only

[Permissions for workflows and automated review](../security/workflow-permissions.md#workflow-permissions-start-from-read-only).

## A Release Is Published Only From a Dispatch on Main

[Release authority and signing boundaries](../security/release-authority.md#a-release-is-published-only-from-a-dispatch-on-main).

### Temporary CLI Recovery

[Release authority and signing boundaries](../security/release-authority.md#temporary-cli-recovery).

### Retained App Release Baseline (Suspended)

[Release authority and signing boundaries](../security/release-authority.md#retained-app-release-baseline-suspended).

## The Review Plugin Marketplace Is Not Pinned (Accepted Risk)

[Permissions for workflows and automated review](../security/workflow-permissions.md#the-review-plugin-marketplace-is-not-pinned-accepted-risk).

## Feed Signing Lifts Ubuntu's User Namespace Restriction

[Permissions for workflows and automated review](../security/workflow-permissions.md#feed-signing-lifts-ubuntus-user-namespace-restriction).

## Self-Hosted Runners Stay Out of This Repository's Own Jobs

[Permissions for workflows and automated review](../security/workflow-permissions.md#self-hosted-runners-stay-out-of-this-repositorys-own-jobs).

## Shared Runner Storage Is a Cache-Poisoning Surface

[Runner pools, tokens and shared storage](../security/pool-isolation.md#shared-runner-storage-is-a-cache-poisoning-surface).

## Per-Repository Isolation on a Runner Host

[Runner pools, tokens and shared storage](../security/pool-isolation.md#per-repository-isolation-on-a-runner-host).

## A Token Carries Only Its Pool Kind's Permissions

[Runner pools, tokens and shared storage](../security/pool-isolation.md#a-token-carries-only-its-pool-kinds-permissions).

## Organisation Pools Share a Runner Group's Trust Boundary

[Runner pools, tokens and shared storage](../security/pool-isolation.md#organisation-pools-share-a-runner-groups-trust-boundary).

## One Instance per Machine, Reached Only by Its User

[Local agent and credential boundaries](../security/local-agent.md#one-instance-per-machine-reached-only-by-its-user).

## The Agent Keeps Tokens in the Operating System's Store

[Local agent and credential boundaries](../security/local-agent.md#the-agent-keeps-tokens-in-the-operating-systems-store).

## The App Surface

[Marshal app and update trust](../security/app-trust.md#the-app-surface).

## Updates Come Only From the Signed Feed

[Marshal app and update trust](../security/app-trust.md#updates-come-only-from-the-signed-feed).

## Tokens Stay Out of the Log, and Job Clients End With `start`

[Local agent and credential boundaries](../security/local-agent.md#tokens-stay-out-of-the-log-and-job-clients-end-with-start).

## The TOML Configuration Holds No Token and Quotes No Line

[Local agent and credential boundaries](../security/local-agent.md#the-toml-configuration-holds-no-token-and-quotes-no-line).

## Editing the Configuration Through the Agent Moves No Trust Boundary

[Local agent and credential boundaries](../security/local-agent.md#editing-the-configuration-through-the-agent-moves-no-trust-boundary).

## Diagnostic Evidence Stays Private

[Local agent and credential boundaries](../security/local-agent.md#diagnostic-evidence-stays-private).

## `build` Builds Only What the Binary Embeds

[Local agent and credential boundaries](../security/local-agent.md#build-builds-only-what-the-binary-embeds).

## CPU Placement Is Not Resource Reservation

[Runner pools, tokens and shared storage](../security/pool-isolation.md#cpu-placement-is-not-resource-reservation).

## LAN Egress From Runner Containers (Accepted Risk)

[Runner pools, tokens and shared storage](../security/pool-isolation.md#lan-egress-from-runner-containers-accepted-risk).

## A Linux Job on axmc Runs in a Per-Job VM

[Isolation on the macos runner host](../security/vm-isolation.md#a-linux-job-on-axmc-runs-in-a-per-job-vm).

## CPU Placement Inside a Job VM

[Isolation on the macos runner host](../security/vm-isolation.md#cpu-placement-inside-a-job-vm).

## Softnet Confines a Job VM's Network

[Isolation on the macos runner host](../security/vm-isolation.md#softnet-confines-a-job-vms-network).

## The Accepted Host-Facing Surface of a Job VM

[Isolation on the macos runner host](../security/vm-isolation.md#the-accepted-host-facing-surface-of-a-job-vm).

## The VM-Local Docker Socket

[Isolation on the macos runner host](../security/vm-isolation.md#the-vm-local-docker-socket).

## A macOS Job on axmc Runs in a Per-Job macOS VM

[Isolation on the macos runner host](../security/vm-isolation.md#a-macos-job-on-axmc-runs-in-a-per-job-macos-vm).

## Tart, Softnet, and the Golden VM Are Pinned

[Isolation on the macos runner host](../security/vm-isolation.md#tart-softnet-and-the-golden-vm-are-pinned).

## Nothing Identifying or Secret Is Committed

[Credentials and identifying information](../security/disclosure.md#nothing-identifying-or-secret-is-committed).

## The Review Gate Admits One Bot Identity by Login, Not Association

[Permissions for workflows and automated review](../security/workflow-permissions.md#the-review-gate-admits-one-bot-identity-by-login-not-association).
