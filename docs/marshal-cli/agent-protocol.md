---
type: Specification
title: "Agent protocol version 2 and snapshots"
description: "Which messages and snapshot fields can a client rely on, including when versions differ?"
---

# Agent protocol version 2 and snapshots

## Protocol Version 2

Protocol version 2 is served only by the agent, on the channel of the instance layer
with its access rules: only the agent's user reaches it, as [Control
Channel](./foreground-instance.md#control-channel) describes. A message is one JSON object on one line. A
client message is at most 16 KiB and an agent message at most 64 KiB, line break
included. The first line a client sends MUST be `hello`; anything else, including
`start`'s version 1 requests, is answered with `unsupported_protocol` and the
connection is closed, so today's commands exit 1 against an agent and change
nothing. [Version Rules](#version-rules) says what a later agent of version 2 may add.

| Message     | Fields                                                                                                              |
| ----------- | ------------------------------------------------------------------------------------------------------------------- |
| `hello`     | `versions` (the versions the client speaks), `client` (`name`, `version`), which the agent ignores                  |
| `welcome`   | `version` (2), `agent` (`version`), `methods`, `topics`                                                             |
| `request`   | `id` (an integer of the client's own), `method`, `params`                                                           |
| `response`  | `id`, then `result` or `error`                                                                                      |
| `error`     | `error`, for what answers no request: a failed handshake, `busy`, or a message that is not a request                |
| `event`     | `topic` (`state` or `github`), `seq`, `value` (the snapshot, or the GitHub status)                                  |
| `heartbeat` | `seq`, the last event's                                                                                             |
| `ending`    | `reason`: `quit`, `executable_changed` or `update`; `update` (`version`, `stopped_by_you`, `runners`) for an update |

A reader ignores a message type, a field and a topic it does not know, and calls only
the methods `welcome` lists. The library's `Client` does so, refuses a `welcome` that is not version 2, and
subscribes only to a topic `welcome` listed, reports a result it cannot read, and a `response` whose
envelope it cannot read but whose `id` it can, as `BadAnswer`, not as a closed connection, and refuses a request longer than 16 KiB
with `TooLong` before writing anything, so the connection stays usable. Its wire, snapshot, `Push`, `CallError` and
`ConnectError` types are `#[non_exhaustive]`, so a later slice can add a field or a
variant without breaking a program that uses them; `tests/agent_protocol.rs` shows it
against unknown methods, topics and fields.

The agent answers one connection's requests in order and several connections at once.
A connection holds at most one subscription, so a client that wants both topics
opens two connections; a `subscribe` to another topic on a connection that holds one
is answered with `invalid_request`, and one to the same topic again changes nothing
but, for `github`, counts as a renewal.

| Method                      | Params                                              | Result                                                             |
| --------------------------- | --------------------------------------------------- | ------------------------------------------------------------------ |
| `status`                    | none                                                | The snapshot                                                       |
| `subscribe`                 | `topic`: `state`, `github`                          | `{topic}`; the first event follows at once                         |
| `renew`                     | `topic`: `github`                                   | `{topic}`; keeps the connection's `github` subscription            |
| `start_all`                 | none                                                | `{state}`, `resuming` while the pools are admitted                 |
| `stop_all`                  | none                                                | `{state}`                                                          |
| `build`                     | none                                                | `{state, started}`, `started` in milliseconds since the Unix epoch |
| `quit`                      | none                                                | `{state}`                                                          |
| `token_set`                 | `repository`, `token`                               | `{repository}`, the key it was stored under                        |
| `token_remove`              | `repository`                                        | `{repository}`                                                     |
| `organization_token_set`    | `organization`, `token`                             | `{organization}`, the lower-case key it was stored under           |
| `organization_token_remove` | `organization`                                      | `{organization}`                                                   |
| `drain_for_update`          | `version`                                           | `{state, update}`; see The Update Drain                            |
| `config_read`               | none                                                | `{revision, status, settings, pools, problems, too_large}`         |
| `config_edit`               | `revision`, `edits`, `check`                        | `{revision, effects, repositories_released, applies}`              |
| `config_reload`             | `revision`, `check`                                 | `{revision, effects, repositories_released, applies}`              |
| `launch`                    | none                                                | `{state}`, with `reason` when it stays stopped                     |
| `lifecycle_set`             | `key`, `value` (`true`, `false` or `null` to unset) | `{lifecycle}`, the snapshot's part                                 |

`token_set` and `organization_token_set` carry a secret: the token travels only in the
`token` param, from the client to the agent, and the agent never sends, logs or
repeats it. Its error text never quotes the params. A method a later slice adds
under the version rules above that carries a secret is handled as `token_set` is.
On Windows the client makes its same-user check (see [Control
Channel](./foreground-instance.md#control-channel)) at connection, before it sends any request, so every
such method is covered without a check of its own.

Errors have a stable `code`, an open set, and a `message` in plain text:

| Code                   | Meaning                                                                                                                                  |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `unsupported_protocol` | The first line was not a `hello` that lists version 2; `versions` lists what the agent serves                                            |
| `busy`                 | The agent is in a transition that does not take the method, or has its most connections                                                  |
| `unknown_method`       | The agent has no such method                                                                                                             |
| `invalid_request`      | The params are not what the method takes, or the message is not a request; a malformed request with a readable `id` is answered under it |
| `subscription_limit`   | The agent has its most subscriptions                                                                                                     |
| `not_subscribed`       | A `renew` on a connection that holds no subscription to the topic, such as one that ended for want of renewal; subscribe again           |
| `token_invalid`        | The token is empty or too long                                                                                                           |
| `store_failed`         | The token store could not do it; `store` holds `kind` and `code`                                                                         |
| `refused`              | The agent's state does not allow the method                                                                                              |
| `config_changed`       | `config.toml` no longer holds the bytes of the `revision` the request names                                                              |
| `config_invalid`       | The result breaks the rules; `problems` lists each with its `path`, `line` and `message`                                                 |
| `config_unusable`      | The file is missing, unreadable or not TOML, or the migration of `config.json` failed or is pending                                      |
| `config_unwritable`    | `config.toml` could not be replaced, for example while another program holds it open on Windows                                          |
| `config_refused`       | An edit touches `schema_version` or `lifecycle`                                                                                          |
| `config_file_form`     | The file is written in a form the write path does not edit, such as an inline table over several lines; fix it in a text editor          |
| `config_applying`      | An edit is being checked or applied while the runners run                                                                                |
| `runtime_restarting`   | The runtime core waits to restart after a failure                                                                                        |
| `image_unusable`       | The image a change names is missing or unusable on a running agent                                                                       |

A subscription gets the snapshot at once as an event with `seq` 1, then an event
with the whole snapshot after each change, at most one per 250 ms: changes within a
window are merged into one event, so a client never sees an intermediate state it did
not ask for. `seq` rises by one with each event of the connection. After 15 s without
an event the agent sends a `heartbeat` with the last `seq`, and the library's client
reads 45 s of silence on a subscribed connection as a lost agent. A `Client::call`
fails with `Closed` once the connection has ended but waits as long as the agent
takes on one that stays open, so a caller that needs a bound wraps it in its own
timeout. Dropping a call, as that timeout does, is safe: the client has queued the
request whole for a task that writes it to the end, and discards the answer.
Before the agent ends it sends `ending` to every connection, one still in
its handshake included, and waits up to 0.5 s for them to end; an
answer already given, such as `quit`'s, is written before the `ending`.

The limits are timers on the agent's own runtime on every operating system, so a
silent or stalled client holds no task of its own past them. A thread is held only
where the pipe crate needs one, which is measured on Linux only (the stalled readers'
test counts the runtime's worker threads there, and finds none held); the Windows
case is next to the table.

The limits:

| Limit         | Value                                                                                                                |
| ------------- | -------------------------------------------------------------------------------------------------------------------- |
| Connections   | 16; a 17th is told `busy` and closed; beyond 32 connections open or waiting for `busy`, one is closed with no answer |
| Subscriptions | 4; a 5th is told `subscription_limit` and the connection stays                                                       |
| Handshake     | 5 s from connect to `hello`                                                                                          |
| Idle          | 60 s without a message, for a connection with no subscription and no request pending                                 |
| Renewal       | 90 s without a `renew` or `subscribe` ends a `github` subscription; the connection stays                             |
| Write         | 5 s for one message, or the connection is closed                                                                     |

On Windows the pipe is created as the first instance of the name with the descriptor
`D:P(A;;GA;;;<user SID>)` and with remote clients rejected, as for `start`, but it
is served by the pipe crate's asynchronous listener on the agent's runtime, so the
limits above are the same there as on macOS. Both channels take the pipe's options
from one function, so they cannot differ. The pipe crate flushes a pipe on a blocking
thread that waits until the peer has read everything, and that wait cannot be
cancelled. So the agent flushes only the messages that end a connection (`ending`,
and the error that answers a refused or failed handshake), each in at most the write
limit, and abandons a connection's write half when the connection ends for any
reason, so the crate does not park it on a waiting thread. A client that never reads
and never closes can still keep one such thread for each of those final messages.
The agent leaves its runtime without waiting for blocking tasks, so neither the exit
nor the release of the lock waits for such a thread, and
`tests/agent_quit_stalled.rs` runs the agent as a process on every platform and
shows a `quit` ends it within a second or so while a subscribed client reads
nothing. On Unix the socket has the checks of
[Control Channel](./foreground-instance.md#control-channel).

## Version Rules

These rules bind every agent and every client of protocol version 2, including the
ones other children add. The sections above describe what version 2 holds today;
this one says how it may grow without a new version.

Within version 2 an agent may add methods, topics, fields, error codes, runner
states, images, image warnings, and the reasons a state or `ending` carries, and
lists its methods and topics in `welcome`. A client ignores what it does not know,
shows an unknown runner state by the runner's reason, and calls only methods listed.
Removing one, or changing its meaning, needs version 3.

So a client written against today's agent keeps working against a later agent of
version 2, and a later client works against today's agent by calling only what
`welcome` lists. [Protocol Version 2](#protocol-version-2) says how the library's
`Client` follows these rules, and App says how the app follows them, in
[The Window's Page](../marshal/window.md#the-windows-page) and
[The Agent and the App's Session](../marshal/session.md#the-agent-and-the-apps-session). A
change that removes or reinterprets anything above is a version 3, which is a new
framing decision for the maintainer, not an addition.

## The Snapshot

The snapshot is what `status` returns and what the `state` topic carries. It comes
from local state the agent already keeps; it reads nothing from Docker or GitHub, and
never holds a token or a JIT configuration. Every field is optional for a reader, and
a reader ignores one it does not know.

| Field                                          | Holds                                                                                                                                                       |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agent`                                        | `version`, `state`, `reason` (`code`, `message`), `since`, `build`, `restart_due`, `update`                                                                 |
| `agent.update`                                 | While a drain for an update runs: `version`, `waiting`, `deadline`, `pid`                                                                                   |
| `agent.build`                                  | `started`, `outcome` (`building`, `ready`, `failed`, `cancelled`), `finished`, `message`                                                                    |
| `configuration`                                | `path`, `problems` (at most 100), `status`, `in_use`, `migration` and `last_apply`                                                                          |
| `configuration.last_apply`                     | Absent until an edit applies; else `revision`, `at`, `outcome` (`applying`, `applied`, `abandoned`, `failed`) and `reason`                                  |
| `configuration.status`                         | What the latest read of `config.toml` found: `valid`, `invalid`, `missing` or `unreadable`                                                                  |
| `configuration.in_use`                         | Absent when none is in use; else `source` (`file`, `last_valid` or `json`) and `read_at`                                                                    |
| `configuration.migration`                      | Absent when none applies; else `state` (`migrated`, `failed`, `pending`), `at` and `reason`                                                                 |
| `runners_up`, `runners_wanted`                 | Totals over the listed pools, kept when rows are left out; 0 while `restart_due` is set                                                                     |
| `repositories`                                 | Each repository (`owner/name` in lower case), `has_token` and its pools                                                                                     |
| `organizations`                                | Each organisation login in lower case, `has_token` and its pools                                                                                            |
| `pools`                                        | `name`, `repository`, `state`, `reason`, `has_token`, `runners_up`, `slots`, `runners`, `diagnostics`                                                       |
| `pools[].organization`, `pools[].runner_group` | Present only for an organisation pool, which has no `repository` field                                                                                      |
| `pools[].runners`                              | `slot`, `state`, `workload`, `workload_kind`, `reason`, `since`                                                                                             |
| `pools[].diagnostics`                          | `limit`, `written`, `in_sink`, `containers`, `findings`, `manual_recovery`                                                                                  |
| `leftovers`                                    | What a stop could not remove (`kind` `container`), and on axmc each counted job VM of an unconfigured pool (`kind` `vm_unconfigured_pool`), with its `name` |
| `images`                                       | Each image the agent tracks (the runner image, and on axmc the golden macOS VM): `name`, `lines`, `warnings`                                                |
| `omitted`                                      | How many `runners`, `problems`, `pools`, `repositories`, `organizations`, `leftovers` and `images` it left out                                              |

When the runtime core fails, the agent starts it again after 10, 30 and 60 s for the
first, second and third failure within 10 minutes, and stops for `restart_limit` at the
fourth. While it waits, `agent.restart_due` holds the time it restarts, the agent's
state stays `running`, and `pools` is empty, so `runners_up` and `runners_wanted` read
0 for the wait, because the core's rows are not kept across a failure. A stopped agent
lists its pools again. `repositories` is still listed.

A pool is `waiting_for_docker`, `preparing`, `running`, `skipped` or `stopped`; the
first word stands for whichever engine the pool waits for. A runner is `running`,
`registering`, `backing_off`, `held`, `waiting_for_vm_limit`,
`waiting_for_vm_deletion` or `stopped`. The `reason` of a runner is plain text that
says more about its state, such as what a state means that the reader may not know.
A `waiting_for_vm_limit` runner's macOS job VM waits for a place under the macOS VM
limit, and a `waiting_for_vm_deletion` runner waits for the deletion of the VM its
`workload` names; both occur only on axmc's Tart backend, and each `reason` says what
the slot waits for. `workload_kind` is `vm` when the `workload` is a Tart VM, and is
absent for a container. A backing-off runner gives the reason it backs off: a token store
failure's kind and code, or what GitHub answered, scrubbed of any token and never a
command. A `held` runner is one whose job host a host prerequisite refuses, with
that prerequisite's reason, scrubbed the same way and cut to 500 characters; it
occurs only on axmc's Tart backend, which the agent, running axpc only today, does
not run. An image warning is data, not text: a `code` (`not_built_by_marshal`,
`packages_old`, `image_old`, `runner_old` or `runner_may_be_newer`) with `days` or
`version` where they apply, so the app words it; the golden macOS VM's codes are
under [macOS Status](./macos-pools.md#macos-status). `leftovers` lists every container
name of a failed removal call, which can name more containers than the one that
failed. They stay listed until a start prepares the pools, which removes what is
stale, so a `start_all` that stops for a missing image or an invalid configuration
keeps them. A `vm_unconfigured_pool` entry is not a failed removal: it follows the
latest count of macOS VMs, which `start` never deletes, and stays listed until a
count no longer finds it.

An agent may add a runner state, an image and an image warning within version 2, so
a reader MUST treat a state word it does not know as an unknown state and show the
runner's `reason`, MUST list an image it does not know, and MUST ignore a warning
code it does not know. The library's `RunnerState`, `RunnerInfo::kind`,
`ImageWarning::kind` and `ImageInfo::known_warnings` do so, and a unit test of the
client shows an event with a state and a warning from a later agent.

A snapshot is cut to at most 64 KiB less the message's envelope. When it is larger
it first lists no more than 100 configuration problems, then leaves out runner rows
from the end, as many as it must and no more, then pools from the end, then
leftovers, repositories, organisations, images and configuration problems from their ends, and says
how many in `omitted`. Last of all the long texts, the configuration path, the
migration's reason and the agent's version, reason and build message, are cut to 512
characters. `runners_up`
and `runners_wanted` still count a row left out.
