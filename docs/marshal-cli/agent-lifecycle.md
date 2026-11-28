---
type: Specification
title: "Agent lifecycle and update drain"
description: "Which lifecycle settings and drain conditions govern stopping, resuming or updating the agent?"
---

# Agent lifecycle and update drain

## The Agent's Signals

The agent acts on the five `[lifecycle]` settings of
[The Agent's Configuration](./resident-agent.md#the-agents-configuration) on axpc and
on axmc. A key that is absent, or the whole table, takes its default, which is the
same on both (M4, [#90](https://github.com/axross/runners/issues/90)), and so does
every key while the agent runs on a `config.json` its migration could not replace; the
snapshot marks such a setting `defaulted`. On axmc the agent keeps no sign-in item,
since the app keeps the login item, and `stop_on_network_change` is accepted and
written but has no effect: a change of the primary network always stops every runner
there.

| Setting                   | When on                                                                                                                    | Default |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------- |
| `start_at_login`          | The agent keeps the sign-in item registered, so the app starts at sign-in with `--sign-in`; off, it removes the item       | off     |
| `start_runners_on_launch` | `launch` starts the runners; off, `launch` leaves the agent stopped by you, with the banner's "not started at launch" line | on      |
| `keep_awake`              | The computer is kept from idle sleep while the agent is `running`, `resuming`, `building` or `stopping`                    | on      |
| `stop_on_network_change`  | A change of the primary network stops every runner with `network_changed`                                                  | on      |
| `resume_after_stop`       | After a sleep or network-change stop, the agent starts the runners again once it is back on the recorded network           | off     |

A sleep stops every runner whatever the settings say.

The agent runs a `Monitor` (`instance::lifecycle`) beside its states, the seam the
`start` instance uses on axmc. `Agent` takes it through `Seams`, with `NoMonitor` as
the default, and the agent itself names no operating system:

- Entering `running`, `resuming`, `building` or `stopping` takes the keep-awake hold,
  and entering `stopped` or `quitting` ends it; `maintain` runs for the agent's whole
  life, so a refused hold is retried for as long as the hold is wanted, and in
  `stopped` and `quitting` nothing is retried.
- Each run calls `begin`, then waits on `watch` beside the runtime core. A sleep or a
  network change stops every runner with the stop sequence and leaves the agent
  stopped for `slept` or `network_changed`.
- A signal stop during a drain ends the agent for the update, and `ending`'s `update`
  names the signal's code in `runners`. A stop by you during the drain's last steps
  still makes it `by_you`.

`ending`'s `update.runners` is `running`, `by_you`, or a signal's code, and
`stopped_by_you` repeats whether it is `by_you`. The app starts the runners after an
update only from `running`, so an update never undoes a signal stop.

On Windows, `agent::run` gives the agent `WindowsMonitor` (`windows_host::monitor`)
on `WindowsSignals` (`windows_host::signals`). On the axmc profile it gives M1's
`SignalMonitor` (`macos::monitor`), the monitor `start` uses there, sharing its
network gate with the Tart backend, with the settings in effect through
`with_settings`: with `keep_awake` on the Mac is kept awake while the agent is
`running`, `resuming`, `building` or `stopping`, a sleep or a change of the primary
network stops every runner with `slept` or `network_changed` whatever the settings
say, and the resume below reads the network through the monitor. Elsewhere it gives
`NoMonitor`. On every profile the
agent's stops reserve the backend's release budget as `start`'s do, so a stop on Tart
has the time to delete its job VMs.

`WindowsMonitor` reads the settings in effect, which the agent sets from each
configuration it reads into `lifecycle_settings::Current`, at every tick, so a
change takes effect within a second:

| Signal          | What it reads                                                                                                                                                           | Gated by                                                                                       |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Sleep           | Every 1 s, `Instant` against `QueryUnbiasedInterruptTime`: the unbiased clock more than 3 s behind, or more than 30 s between samples, is a sleep                       | Nothing; a sleep always stops                                                                  |
| Primary network | Every 5 s, `netdev`'s default interface, bounded to 2 s on a blocking thread with one reading outstanding at most; M1's `NetworkWatch` judges two differing checks      | `stop_on_network_change` stops; the network is read only while it or `resume_after_stop` is on |
| Keep-awake      | The request the executable supplies through `AgentOptions::keep_awake`, held by one thread that lives as long as the agent; a refusal warns once and retries every 10 s | `keep_awake`; nothing is held while no setting is in effect                                    |

`lifecycle_set` writes one key of `[lifecycle]` through K1's write path, so the rest
of the file keeps its form and comments, and `null` removes only that key's line. It
answers with the snapshot's `lifecycle` part and refuses as `config_edit` does: a file
that changed since the agent last read or wrote it (`config_changed`, so a hand edit
is read at the next launch or Start all first), has problems or cannot be written, a
file written in a form the write path does not edit (`config_file_form`, such as
`lifecycle = { … }` over several lines), and while no valid configuration was read
(`config_unusable`); while an edit applies it is `busy`.

The snapshot's `lifecycle` part, present while settings are in effect, holds
each setting's `on` and `defaulted`, the sign-in item (`registered`,
`not_registered`, `turned_off` in Windows' Startup apps, or `failed` with
`sign_in_error`; absent on axmc), `keep_awake` (`held`, `released`, `warning` with its text, also
while the program supplied no request, or `off`), and after a signal stop with
`resume_after_stop` on, `resume` (`waiting`, or `no_gateway`) with `resume_cause`,
the stop's code. The agent keeps the sign-in item, the current user's Run key value
`Marshal` (`MarshalTest` in a debug build), in step with `start_at_login` at each
configuration it reads and at each
`lifecycle_set` of that key; it holds the quoted path of the program it runs from
and `--sign-in`, at most 260 characters.

A sleep is judged before the network, so a wake on another network stops with the
sleep's reason, `Windows slept, so every runner was stopped`. An interface is named
by its kind and index, such as `Wi-Fi 12`, never by its alias.

The resume (`instance::resume`) follows a signal stop while `resume_after_stop` is on:

- At the stop, the agent arms it with the monitor's `recorded` network, the primary
  network last read while running. A record without a gateway hardware address arms
  nothing, and `resume` is `no_gateway`.
- While a resume waits, the stopped agent reads the network through the monitor's
  `next_reading`, every 5 s. Once two readings in a row match the record, with the
  gateway's hardware address read in both, it takes `start_all`'s path: the
  configuration is read again and the image checked.
- `stop_all`, `quit`, `build`, `start_all`, `launch` and turning the setting off
  cancel it. A `stop_all` that cancels one leaves the agent stopped `by_you`, and so
  does a `stop_all` while the signal's stop is still stopping, which arms nothing.
- The agent counts its automatic resumes over a rolling hour, in memory. A signal stop
  after 5 of them arms nothing and stops with `resumed_too_often`, whose text begins
  with the signal's.
- [The app session](../marshal/session.md#the-agent-and-the-apps-session) keeps a signal stop across its restart of a lost agent and an
  update.

## The Update Drain

W4 ([#78](https://github.com/axross/runners/issues/78)) adds `drain_for_update` to
protocol version 2, under its rules for added methods and fields. Before the app
installs an update, it asks the agent to drain: to stop registering runners and to end
once the jobs already running have finished, within 6 hours. The `version` param is
the version of the update, three numbers and an optional suffix, at most 64
characters in all; anything else is `invalid_request`.

A runner is busy once its container's output has carried a line that reads
`Running job:` after the runner's timestamp, and idle once it has carried
`Listening for Jobs` and not `Running job:`. A runner whose output has carried
neither line is waited for, as if busy. That fallback covers a runner that prints
neither line, not one that still prints `Listening for Jobs` but rewords or drops
`Running job:`: its busy runners would read as idle and be stopped mid-job, so
[Bumping the Runner](../releases/runner-update.md#bumping-the-runner) checks both lines.
Both streams count, a mark never clears, and
no other line, such as `Runner reconnected.`, changes it. Only the fixed text is
compared: the drain keeps no job name and puts none in the snapshot or a protocol
message; the line itself reaches the log, as all runner output does.

The drain's last steps begin when the drain ends its runners: when no runner is waited
for any more, when the 6 hours pass, or when the runtime core ends for the drain or
fails. In them the container steps run and the agent then ends for the update; the
drain can no longer be cancelled, because its runners are already stopping.

| When                                                     | Result                                                                                                                                                 |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `drain_for_update`, while `running`                      | Drains: the answer is `{state, update}`, and `agent.update` appears                                                                                    |
| `drain_for_update`, while `stopped`                      | Ends for the update at once; `runners` is the stop's code when stopped by you or by a signal, and `running` after an error stop                        |
| A signal stop, while draining                            | Stops every runner, so running jobs fail, then ends for the update; `runners` is the signal's code                                                     |
| `drain_for_update`, in any other state or while draining | `refused`, and nothing changes                                                                                                                         |
| `start_all` or `build`, while draining                   | `refused`, and nothing changes                                                                                                                         |
| `stop_all`, while draining                               | Answers `quitting`, also during the drain's last steps; stops every runner, so running jobs fail, then ends for the update, stopped by you             |
| `quit`, while draining                                   | Stops and ends as a `quit` does; `agent.update` is gone once the stop begins or, during the drain's last steps, at once, and the `ending` names `quit` |
| The connection that asked closes, while draining         | Before the last steps, the drain is cancelled, the agent keeps running, and the slots register again; during them, the agent still ends for the update |
| A runtime core failure, while draining                   | The container steps run, so running jobs fail; the core does not start again, and the agent ends for the update                                        |
| No runner is waited for any more                         | The container steps run, then the agent ends for the update                                                                                            |
| 6 hours pass                                             | The container steps run, so the jobs still running fail, then the agent ends                                                                           |

While the agent drains, nothing registers a runner: not a slot, not a pool prepared
after a core failure or when Docker comes back, and not a pool that waited for a token.
A slot that is registering or backing off stops without starting a container, an idle
runner's container is stopped with a stop's grace, and killed when the stop fails,
and a busy runner's container runs
until it exits; its slot then finalises it as after any job and registers nothing. A
drained slot is listed `stopped` with the reason "stopped for an update". The agent's
state stays `running`, so an older app shows it running; `agent.update.waiting` counts
the slots still running a container, an idle one until its stop ends, `deadline` is the bound's time in milliseconds since the
Unix epoch, and `pid` is the agent's process ID, which the app waits on. Before the
agent ends for an update, every connection receives `ending` with the reason `update`
and an `update` field naming the version and whether the runners were stopped by you.
A `drain_for_update` while stopped answers `{state, update}` with the state
`quitting`, so the app has the process ID it waits on. If the requesting
connection's close is seen before the core ends for the drain, the drain is cancelled
and the core starts again; if the core's end is seen first, the last steps have begun. Only the
container backend reads a runner's output today; a backend that does not, such as
axmc's Tart backend, reports every runner as waited for, so its drain ends at the
bound.
