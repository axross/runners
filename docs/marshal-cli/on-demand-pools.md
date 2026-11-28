---
type: Specification
title: "On-demand pools"
description: "How does an on-demand pool listen for GitHub demand, start runners, clean up and recover from a failure?"
---

# On-demand pools

A Windows resident-agent pool with `scale_set` waits for GitHub demand instead of
keeping every slot registered. Workflows in the pool's repository target it with
`runs-on: <scale-set name>`. With no offered or assigned demand and no running job,
successful cleanup leaves no registered runner and no job container. There is no
warm-runner or minimum-idle setting. The pool retains its per-container CPU and
memory limits, and `runners` caps concurrent starting and running runners; Windows
gains no host budget or dynamic lending of idle capacity. Pools without the key,
including Mac Linux and native macOS pools, keep their always-on behavior. The
[TOML configuration](./toml-configuration.md#schema-version-1) owns the `scale_set`
key and its validation rules, and the
[configuration editing protocol](./config-protocol.md#editing-the-configuration) owns
how a client edits it. The operator's steps are in
[Configure Windows runner pools](../runner-hosts/windows-pools.md#on-demand-pools-in-the-resident-agent).

This is the Windows/Docker implementation contract, not a report of live product
validation. Native-Windows job delivery, stop/restart, crash recovery, timing and
the app's added states still need their product verification. The earlier protocol
probe is feasibility evidence only; WSL is not native-Windows evidence. Mac
on-demand support is not included. Its separate pull request starts only after the
Windows pull request merges, its M2 prerequisite being met by merged pull
request #118, as the maintainer decided on
[#72](https://github.com/axross/runners/issues/72#issuecomment-7000962487); its
scope is Tart Linux, not native macOS on-demand pools.

## Listening and Admission

At pool start the agent reads the repository's stored
token, exchanges registration credentials and finds or creates the named scale set
in runner group 1. The client pins the Actions service API to `6.0-preview`. The
registration exchange is the one request it retries: a `401` or `403` is tried up
to 3 times in all, 5 s apart, because GitHub does not always accept a fresh
registration token at once. Every other call fails on its first answer, and the
agent's back-off decides what follows.
Before any session opens, both a found and a newly created set must have exactly
one label equal to the configured scale-set name, ignoring case. A mismatch
rejects the pool without opening a session, registering a runner or changing the
set.

Each new or recreated session's initial statistics supply demand before the first
poll. The listener uses GitHub's assigned-job count rather than counting job
events. It acquires `JobAvailable` offers only up to free capacity and also accepts
`JobAssigned` demand. Each admitted slot obtains a scale-set JIT configuration and
runs one ephemeral runner in a Docker container. Slot completion and retry timers
continue reconciliation even when no new event arrives. A `202` poll keeps the
last statistics, not zero demand. Overflow offers retain their request IDs and
remain unacknowledged until acquired as capacity frees or confirmed no longer
pending. Assigned work beyond the cap waits and may be requeued by GitHub.
Acknowledgement follows successful processing, and message progress advances only
after acknowledgement succeeds, so replay does not launch another runner for the
same demand. A `404` on acknowledgement counts as acknowledged, because a repeated
delete after a lost response may find the message already gone; a session that has
really gone answers `404` on the next poll. The service hands an unresolved message back at once, so while one
waits for capacity the listener waits up to 10 s, or until a slot ends, before it
polls again.

## Names and Recovery

The runner and job container share
`<scale-set>-<slot>-<yyyyMMddHHmmssfff>`, with the existing 1-based slot and exactly
17 ASCII UTC timestamp digits. The configured `name` remains the logical pool
identity for limits, volumes, status attribution and runtime ownership; it need
not equal `scale_set`. At startup the agent lists the repository's runners once
and removes only offline runners whose whole name matches this pool's scale-set
format. Online runners, a longer scale-set name, other pools and always-on runners
are not adopted by a prefix match. Container cleanup recognises the same whole
name and retains cleanup of the logical pool's older always-on containers when
changing modes, under the existing diagnostics rules. The Mac runtime's
`marshal-job-<logical-pool>-<slot>-<yyyyMMddHHmmssfff>` VM names and exact configured
logical-pool cleanup rules are unchanged; a scale-set alias is not a VM pool name,
and no cleanup broadens to golden, build or unrelated VMs.

## Stop and Cleanup

Stop All, Quit, a lifecycle stop and removal of the pool close
the session and stop its runners. Neither a stop nor an update drain acquires more
work or starts a runner. Update drain retains its running-job deadline and resume
rules. Session close runs beside the existing stop sequence, best-effort within
5 s, without delaying its steps or extending its deadline. Registration cleanup
uses exact owned runner IDs and names after normal completion, a launch failure,
abnormal exit, stop, or JIT success that arrives after stop or drain. A lost JIT
response is reconciled by that attempt's exact name before another registration.
A confirmed deletion or confirmed absence settles cleanup; a failed or unknown
outcome stays visible with the pending non-secret identity. A stopped container
alone does not prove the registration is gone. A cleanup that fails when a job
ends is retried with the slot's back-off while the slot keeps its capacity, and
the slot shows `registration cleanup incomplete (<reason>)` until it settles. An
error a retry cannot change, such as a refused token, ends the retries: the slot
is held, the cleanup stays reported incomplete, and Start All retries it. A stop
gets one last attempt within the stop deadline, and also ends a registration
that is waiting for, or renewing, its credentials. During an update drain a
failing cleanup gets at most one more attempt, then the slot counts as drained
with its cleanup still reported incomplete, and the retries resume if the drain
ends without an update. Cleanup shares the existing stop deadline.

## Editing the Configuration

An edit that restarts or removes an on-demand pool
waits for that pool's listener to end within the stop deadline. A pool whose
listener has not ended, or has cleanup left, is stopped by the edit and stays
stopped, shown as skipped, until Start all, and the other pools take their
effects all the same. The edit then reports each such pool, as `pool <name>: the
edit's listener cleanup did not finish within the stop deadline; Start all is
required before another listener starts` or `pool <name>: on-demand cleanup is
incomplete; Start all is required before another listener starts`.

## Tokens and Requests

On-demand pools use the same repository-scoped
fine-grained token, Administration: Read and write, with Actions: read optional for
job names; the Fine-Grained Token section of
[Register a pool with GitHub](../runner-hosts/registration.md#the-fine-grained-token) gives its
steps, including those for an organisation-owned repository. The agent rereads the store before every JIT registration. Replacement
invalidates credentials derived from the old token even while those credentials
are unexpired; removal or a store failure blocks registration rather than falling
back to cached credentials. Running jobs keep their original JIT configuration
and lifecycle behavior. Registration exchanges, refresh, startup runner listing,
exact reconciliation and deletion are lifecycle requests outside the display
queue and do not feed its budget. The listener renews the service credential when
2 minutes of its life remain, and that expiry-driven renewal never repeats
within 30 s; a replaced token renews it at once. A renewed credential that is due
again at once backs off as a failure instead of renewing every cycle. An idle listener with unchanged credentials
and no pending cleanup makes no periodic REST display request: the listener polls
the Actions broker and renews service credentials before expiry. Outbound hosts
and memory-only credential handling are governed by
[On-Demand Scale Sets Keep Repository and Host Boundaries](../security/pool-isolation.md#on-demand-scale-sets-keep-repository-and-host-boundaries).

## Pool-Local Failures

Other pools keep running:

| Cause                                                                     | Listener behavior and recovery                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No stored token                                                           | No registration; the existing no-token reason applies.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Store failure at start or refresh                                         | `store_error`, with the store's kind and platform code; waits for Start All. A failure during JIT registration uses slot failure/back-off, never cached authorization.                                                                                                                                                                                                                                                                                                                                                                  |
| Session conflict (`409` on opening a session)                             | `another_listener`; registers nothing and retries with back-off. It does not identify another host, and startup runner cleanup has already run by then, so two hosts that share a scale-set name for one repository can remove each other's offline runners, which [Security](../security/pool-isolation.md#on-demand-scale-sets-keep-repository-and-host-boundaries) forbids. A `409` on any other call is a service failure, below.                                                                                                   |
| Scale set or session gone (`404`)                                         | Finds or creates the set again, and does so again for a later `404` once a poll has succeeded in between. A second `404` before such a poll rejects the pool. Recreations are at least 30 s apart; a `404` inside that window waits, showing `backing_off` with status `404` and the reason `scale set not found; recreating`, and admits nothing until the next recreation; a store error showing then stays shown (`store_error` wins over `backing_off`). A `404` on acknowledgement is not one of these: it counts as acknowledged. |
| Incompatible scale-set labels                                             | `rejected`, reason `scale-set label mismatch`, with no invented HTTP status; waits for Start All after operator repair.                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Rate limit                                                                | `backing_off`, then automatic retry: `429`, or `403` with `Retry-After`, zero remaining quota or a secondary-limit message. `Retry-After` alone suffices even with nonzero quota. A `Retry-After` or quota reset more than one hour away counts as one hour.                                                                                                                                                                                                                                                                            |
| Other persistent `401`, `403`, `404`, or refused outbound host            | `rejected`, with status or refused host, until Start All. A `401` or `403` on the registration exchange is persistent only after its 3 tries.                                                                                                                                                                                                                                                                                                                                                                                           |
| Network, `5xx`, a `409` outside session creation or other transient error | `backing_off`, then retries.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |

## Retained Scale Sets and Label Mismatch

Marshal retains sets when a pool stops, is removed or changes its scale-set name, so
jobs queued while stopped can wait for the listener to return. Marshal never
changes or deletes the incompatible set automatically.

To resolve a `scale-set label mismatch`, choose an unused valid scale-set name, or
stop workflows using the incompatible set and delete it manually before Start All
lets Marshal recreate it. The operator steps for the manual deletion are in
[Removing an Unwanted Scale Set](../runner-hosts/windows-pools.md#removing-an-unwanted-scale-set).
