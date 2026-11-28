---
type: Architecture
title: "Mac sleep and network signals"
description: "How do host signals reach a foreground instance and prevent an unsafe resume?"
---

# Mac sleep and network signals

## Signal Source

The axmc host stops every runner when the Mac sleeps or its primary network
changes, and holds off idle sleep while it runs. `macos` holds what reads those
signals, and on axmc the instance polls it
([Wiring](#wiring-the-signals-into-the-instance)). The seam is `SignalSource` in
`macos::signals`, which the instance polls for five things:

| Call                                                       | Answers                                                                   |
| ---------------------------------------------------------- | ------------------------------------------------------------------------- |
| `sleep_detected`, every 1 s                                | Whether the Mac slept, or its wall clock stepped, since the sample before |
| `rearm_sleep`, at each start of a run                      | Forgets the clocks sampled so far, so the next sample sets a new baseline |
| `read_network`, every 5 s                                  | A `NetworkReading`: the primary network, or `NoDefaultRoute`              |
| `hold_keep_awake`, `poll_keep_awake`, `release_keep_awake` | Taking the hold, checking it and retrying a lost one, and ending it       |
| `keep_awake_warning`                                       | The warning text while the hold is wanted and not held                    |

`MacSignals` is the real source. `FakeSignals` in `macos::fake` is the in-process
source the tests of the code above it use, a debug or test build only.

**Sleep.** Each sample holds the wall clock and an `Instant`. On macOS the
standard library reads `Instant` from `CLOCK_UPTIME_RAW`
(`library/std/src/sys/time/unix.rs` in the pinned toolchain's source), which does
not advance while the Mac sleeps, so the wall clock moves ahead of it by the length
of the sleep. `slept_between` judges two samples: a gap between the two clocks'
movements, in either direction, of more than 3 s is a sleep, and exactly 3 s is
not. A wall-clock step of that size is judged the same way, which errs on the safe
side. A sleep shorter than the gap goes unseen. The first sample of a
`SleepDetector` only sets its baseline.

**Primary network.** `route -n get default` gives the interface and the router, and
`arp -n <router>` the router's hardware address. Each call is bounded to 2 s, runs
in its own process group with standard input null, and has its standard output
capped at 64 KiB; more than that is refused. A release build runs `/sbin/route`,
`/usr/sbin/arp` and `/usr/bin/caffeinate` by absolute path. The readers accept only
the forms macOS prints, which are the ones `print_getmsg` in `route.tproj/route.c`
and `print_entry` in `arp.tproj/arp.c` write in Apple's `network_cmds`:

- `route` prints `route to: default` first, then `interface: <name>` exactly once,
  and `gateway: <IPv4 address>` at most once. The gateway line is absent when the
  route leaves through a point-to-point interface, as a VPN's does, and the reading
  then has no router and makes no `arp` call. A gateway that is a link
  (`link#<index>` of 1 to 9 digits) or a hardware address names the interface and no
  router, and reads the same way: the interface holds the default route. A gateway
  that is neither of those nor a routable IPv4 address, a second `interface:` or `gateway:` line, an interface name that is not
  1 to 15 letters and digits starting with a letter, or a failing exit code is
  refused. A refused table, a hung or missing `route`, and a table with no IPv4
  default route (`route: writing to routing socket: not in table`) are all
  `NoDefaultRoute`.
- `arp -n <router>` prints one line per entry,
  `? (<router>) at <address> on <interface> [ifscope] [permanent] [published (proxy only)] [<type>]`.
  The address is six lowercase hexadecimal bytes of one or two digits each, as `%x`
  writes them (`0:0:5e:0:53:1`). A line of another form refuses the whole answer;
  `(incomplete)` and `-- no entry` give no address; entries on other interfaces are
  ignored; entries on the route's interface that disagree give no address. An
  address that cannot be read leaves the reading with the router and no hardware
  address.

`NetworkWatch` in `macos::watch` is the pure judgment over the readings, built from
the first reading at start or resume:

- A primary network at the first reading is recorded at once. With none, the watch
  waits, and records a network only when it has held, as the same interface and
  router, on two checks in a row.
- A reading is the same network as the record when the interface and the router are
  equal and the hardware addresses are equal, or either is unread. A hardware
  address that was unread at the record is added to it the first time it is read.
- A reading that differs from the record counts, and one that matches resets the
  count. Two differing checks in a row are a change, even if the two differ from
  each other. The change names the record's interface and the latest reading's
  interface, or `none` when the latest has no default route. A watch that has
  reported a change keeps reporting it, and a resume builds a new one.
- Joining another network, a VPN that takes the default route switched on or off,
  and losing the default route for two checks are changes. Roaming between access
  points behind the same router is not.

`macos::reason` builds the two texts `status` and the `start` terminal show:
`the Mac slept, so every runner was stopped` and
`the primary network changed (<old interface> to <new interface>), so every runner was stopped`.
They name interfaces and never network names or addresses.

**Keep-awake.** `KeepAwake` runs `caffeinate -i -w <this process's ID>` as a child
with standard input and output null, in its own process group; `-w` ends it when
this process ends, however it ends. `hold` takes it, `release` ends it, and `poll`
checks it. A `caffeinate` that cannot start, or ends early, gives one `Warned`
event with the warning text, and `warning` returns the text until the hold is back.
`poll` retries a lost hold 10 s after the failure (`RETRY_INTERVAL`), and reports
`Recovered` at the first check after a retry that finds its `caffeinate` still
running, so a `caffeinate` that keeps ending warns once, not at every retry.

**Overrides.** A debug build reads these variables, and a release build ignores
every one, which `tests/release_overrides.rs` shows in the release profile for the
programs and the directory:

| Variable                  | Replaces                                                                     |
| ------------------------- | ---------------------------------------------------------------------------- |
| `MARSHAL_TEST_CAFFEINATE` | The `caffeinate` program                                                     |
| `MARSHAL_TEST_ROUTE`      | The `route` program                                                          |
| `MARSHAL_TEST_ARP`        | The `arp` program                                                            |
| `MARSHAL_TEST_SIGNALS`    | The whole source, with a directory a test writes into while the process runs |

A scripted source also runs the instance's cadence in a fiftieth of its time, so a
process test sees a network check every 100 ms; the ratio of five samples to a check
is the real one.

The directory holds a `sleep` file, which the next sample reports once and removes;
a `network` file, which is `none` or lines `interface=`, `router=` and `hardware=`;
a `keep_awake_failure` file, whose text is the hold's failure while it exists; and
the file `keep_awake`, which the source writes as `held`, `warning` or `released`.

The tests run the real code against the `fake-macos` helper, one program that plays
`caffeinate`, `route` and `arp` by their arguments, records every call's arguments,
process group and terminals, and answers as a test scripts it, including a failure,
a hang and a flood. `macos_signals.rs` covers the pure judgments, and
`macos_processes.rs` the readers, the bounds, the hold and the scripted source. Not
verified by them: the output of a real macOS `route` and `arp`, which the samples
copy from Apple's source (a VPN's `gateway: link#<index>` form included), and a real
sleep; both wait for a run on the Mac.

### Wiring the Signals into the Instance

`instance::lifecycle::Monitor` is the instance's view of the signals; `Instance`
takes one with `with_monitor`, and without one, as on axpc, it asks for nothing.
`start` gives axmc a `macos::monitor::SignalMonitor` built on the `Signals` source
for the process, which is the real `MacSignals` or, in a debug build, the scripted
one.

- **Keep-awake follows the state.** `set_state` takes the hold on entering running
  and resuming, keeps it through stopping, and releases it on entering stopped and
  when the instance ends. `build` neither holds it nor reacts to the signals. A
  failed or lost hold warns once in the log, shows as a `warning` line in `status`,
  and is retried every 10 s; `Keep-awake is held again.` is logged when it recovers.
  The hold is checked every sample while running (inside the watch) and while
  resuming or stopping (the monitor's `maintain`, which `serve` polls beside the
  work it waits for).
- **Each run begins afresh.** When the instance enters running, at the first run and
  at every resume or restart, it calls `begin`: the sleep baseline is set again,
  because nothing sampled the clocks while it was stopped and the time spent
  stopped is no sleep, and the gate below is closed. The watch then reads the
  primary network and builds a new `NetworkWatch`, so a record from before a stop
  is never reused.
- **The watch.** One future per run samples the clocks every second and checks the
  network every fifth sample. A sleep, or two differing checks in a row, ends it
  with a `StopReason`: `Slept`, whose text is
  `the Mac slept, so every runner was stopped`, or `NetworkChanged`, whose text names
  the old and the new interface (`none` when no interface holds the default route).
  Dropping the future, as a stop does, ends the watch. The sleep is checked before
  the network in each sample, so a wake that also changes the network stops with the
  sleep reason.
- **The stop.** The reason ends the instance's running as `stop` does: the core is
  asked to stop, and the container steps and the release of every prepared job host
  run within the stop sequence's 28 s exit deadline, with the state `stopping`. The instance then moves
  to `stopped` with the reason, which `status` shows as `reason:` and the `start`
  terminal as `The instance is stopped: <reason>.`. A `stop` that arrives during the
  sequence ends the instance instead. The restart policy counts nothing, and only
  `start` or `restart` resumes; nothing resumes on its own.
- **No primary network.** `begin` closes the `NetworkGate` in `macos::gate`, and the
  first reading opens it when a primary network is recorded. The Tart backend waits
  at the gate in `prepare_job_host`, before the host prerequisites, so the boot that
  follows a wait of any length uses a check made after it, and a slot that waits has
  cloned nothing.
  Without a default route the gate stays closed until a network has held for two
  checks, and `status` shows
  `network: waiting for a network (no IPv4 default route)`. A stop ends the wait, as
  it ends a preparation, and the gate is open for a backend that was given none.
