---
type: Procedure
title: "Resume runners after sleep or a network change"
description: "Interpret the stop reason, identify the current network and resume the pools only on a trusted connection."
---

# Resume runners after sleep or a network change

## Sleep, Wake, and Network Changes

While runners run, the runtime holds off the Mac's idle sleep, unless the resident
agent runs with `keep_awake` off. A sleep and its wake, or
a change of the primary network, stops every runner, deletes every VM and moves the
instance to stopped, with the reason in its status and its log. The runners are never left to carry on across a sleep or onto
another network, because a VM that lived through either has a clock and a network
that no longer match its job. A new start or a `restart` runs them again, and so does
the resident agent when `resume_after_stop` is on, as [Resuming](#resuming) says;
nothing else does. This
section applies to the axmc profile; the Windows host is not affected.

### Keeping the Mac Awake

- While the instance is running or resuming, and while it is stopping until the last
  VM is deleted, the runtime runs `caffeinate -i -w <its own process ID>`. That stops
  idle sleep, and ends with that process, even if it is killed hard. `start` always
  takes the hold. The resident agent takes it only while `keep_awake` is on, which it is
  by default, and releases it within a sample of the setting turning off; a Mac that
  then sleeps stops every runner at its wake, as [What Stops Every Runner](#what-stops-every-runner)
  describes. [The Agent's Signals](../marshal-cli/agent-lifecycle.md#the-agents-signals)
  owns the agent's settings.
- Stopped, the instance holds nothing: the Mac may sleep when it is idle.
  `build` never holds it either; keep the Mac awake for a build as
  [Building the Golden VM](./golden-build.md#building-the-golden-vm) says.
- If `caffeinate` cannot start, or ends early, the runtime logs
  `keep-awake is not held (<cause>), so the Mac may sleep when idle; trying again`
  once, shows it as a `warning:` line in the status, and tries again every 10
  seconds until it holds. A Mac that sleeps in the meantime stops every runner at its
  wake, as [What Stops Every Runner](#what-stops-every-runner) describes, so a lost hold costs the running jobs and nothing else.
- The hold stops idle sleep only. Closing a laptop's lid, choosing Sleep from the
  Apple menu, or running out of battery still sleeps the Mac and stops every runner.

### What Stops Every Runner

The runtime learns of a sleep only when the Mac wakes, so a sleep and its wake are one
stop. It compares how far the wall clock and the monotonic clock moved between two
samples one second apart, which macOS advances differently across a sleep, and
judges a gap of more than 3 seconds a sleep. A manual change of the clock by more
than 3 seconds counts as a sleep too, which only errs on the safe side. A sleep
shorter than that goes unseen, and leaves the VMs and the network as they were.

The primary network is the interface that holds the IPv4 default route, that route's
router, and the router's hardware address. It is read with `route -n get default` and
`arp -n <router>` every 5 seconds, each within 2 seconds, and only for IPv4, because
Softnet carries only IPv4. The runtime records it when the instance starts running and
when it resumes, and it counts as changed when it differs from that record on two
checks in a row, so a stop begins within 15 seconds of a change.

| What happens                                                                   | Counts as a change |
| ------------------------------------------------------------------------------ | ------------------ |
| Joining another Wi-Fi network or a wired network                               | Yes                |
| Losing the default route for longer than one check                             | Yes                |
| A VPN that takes the default route, switched on or off                         | Yes                |
| Roaming between access points of one network, behind the same router           | No                 |
| A VPN that leaves the default route alone                                      | No                 |
| A router whose hardware address cannot be read at a check                      | No (not compared)  |
| Tart's and Softnet's own bridge interfaces, which never hold the default route | No                 |

A VPN that leaves the default route alone is not noticed, because Softnet's block on
private address ranges already covers the ranges such VPNs usually carry. A check
that cannot read the routing table counts as no default route.

When `start` begins, or an instance resumes, with no IPv4 default route, as on
an IPv6-only network, the runtime clones no VM until a primary network has held for two
checks in a row, and then records it. The status shows
`network: waiting for a network (no IPv4 default route)` meanwhile, and a stop still
ends the wait.

### The Stop Reasons

The status shows the stop as `state: stopped` and a `reason:` line, and the log of
`start` records it as `The instance is stopped: <reason>.`:

- `the Mac slept, so every runner was stopped`
- `the primary network changed (<old interface> to <new interface>), so every runner was stopped`,
  for example `(en0 to en0)` for another Wi-Fi network behind another router,
  `(en0 to utun4)` for a VPN that took the default route, and `(en0 to none)` when no
  interface holds the default route any more.

The reason names only interfaces, so a change of the router, or of the router's
hardware address, on the same interface also reads `(en0 to en0)`, with the same
name twice. The text does not say which of the interface, the router or the hardware
address differed; `route -n get default` shows the router that holds the route now.

A reason names interfaces and never a network's name or an address. Before it says
stopped, the instance fails every running job, deletes every job VM within the same
30-second stop sequence a `stop` uses, and leaves the golden VM alone.

### Resuming

Nothing resumes `marshal start`'s instance on its own: a Mac that wakes, or a network
that comes back, leaves the instance stopped. A new `start` with the same
configuration, by the same user, or a `restart`, which the stopped instance
answers by reading the configuration again, checking Tart, Softnet and the golden VM,
and starting every pool, resumes it. The resident agent resumes only with
`resume_after_stop` on, and then by itself: once two readings of the network in a row
match the network the runners ran on, with the router's hardware address read in both,
it reads the configuration again as a `start_all` does, as
[The Agent's Signals](../marshal-cli/agent-lifecycle.md#the-agents-signals)
describes. Either resume records the primary network and arms the sleep
check afresh, so the time spent stopped counts as neither a sleep nor a change. Do
this only on a network you trust, because the signals only stop the runners; they
never judge a network.
