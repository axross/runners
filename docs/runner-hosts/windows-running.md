---
type: Procedure
title: "Start and monitor Windows runners"
description: "Control pools from Marshal and recognize a healthy host in GitHub, on the machine and in the agent log."
---

# Start and monitor Windows runners

## Running the Host

Start Marshal from the Start menu entry Marshal. A launch starts its agent, which runs
the pools, and the notification-area icon shows their state; the agent keeps running
when the window or the app closes. The agent checks the configuration as
[The Host Configuration](./windows-pools.md#the-host-configuration) describes, writes its log under
`%LOCALAPPDATA%\marshal\logs\`, waits for Docker, checks the image, prepares every
pool, and then runs every slot until it is stopped. With a configuration that cannot
be used, an image that cannot run jobs, or no pool able to start, it stays running
and stopped, and the window names the problem. Unless
`start_at_login` is on, nothing starts Marshal at sign-in: after a reboot or a sign-out
the pools stay down until you start it again.

From the notification-area icon's menu:

| Item                  | What it does                                                                                                                         |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| **Open Marshal**      | Opens the window: the agent's state, each pool's state, runners and diagnostics, the image and its freshness warnings, and tokens    |
| **Start all runners** | Reads the configuration file again, checks the image, and runs the pools; use it after editing the configuration                     |
| **Stop all runners**  | Stops every runner with the stop sequence, within 30 s, and leaves the agent running and stopped                                     |
| **Build image**       | Builds the runner image while the runners are stopped; see [Building the Runner Image](./windows-image.md#building-the-runner-image) |
| **Quit Marshal**      | Stops every runner as Stop all runners does, cancelling a build first, then ends the agent and the app                               |

See the [window and notification-area presentation](../marshal/window.md#the-windows-page) and the [commands and events](../marshal/webview-commands.md#commands-and-events) that connect operator actions to the agent.
The agent's states, waits and reasons are in [Agent State](../marshal-cli/resident-agent.md#agent-state).

Stop the host with **Stop all runners**, or with **Quit Marshal**. A stop gives the
ordinary containers their grace period, and finalises each diagnostic container, as
[Collecting private diagnostics](./windows-diagnostics.md#collecting-private-diagnostics) describes. Closing
the window, or ending the app, leaves the agent and its runners running, and the next
launch reconnects to them. A sign-out or shutdown gives the agent no event it can act
on, so its containers wait for the next start, which removes them. If Marshal does
not answer, end `marshal-app.exe` in Task Manager; the next start removes the
containers it left.

However the agent ends, even from Task Manager, the Job Object it runs in ends every
`docker` client it started. Docker Desktop keeps a container running after its client
is killed, so a job can run on until the next start removes its container. The Job
Object against Docker Desktop is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
hard-kill check was not run, by the maintainer's decision.

One agent runs per machine. While it runs, another Windows user's Marshal is refused,
and its window says that another Marshal process holds the runner lock; that refusal
is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
second-user check was not run, by the maintainer's decision. The app reaches the agent
through a control pipe that only its own user can open and that rejects remote
clients; the rejection of a client on another machine is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
remote check was not run, by the maintainer's decision.
[Security](../security/local-agent.md#one-instance-per-machine-reached-only-by-its-user)
records what this protects.

For the order of registration, container startup and cleanup behind each reported slot state, see [How a Container Picks Up a Job](./windows-job.md#how-a-container-picks-up-a-job).

## Health Checks

- **GitHub:** a healthy host shows, under each repository's **Settings, Actions,
  Runners**, the sum of its entries' `slots` runners, **Idle** when no job is queued and **Active** when
  one is running, because each slot registers a runner before it starts a
  container. Each registration carries exactly `self-hosted`, `linux`, `x64`, and
  the entry's labels, `axpc` by default. Fewer runners than that sum, and none in
  particular, mean Marshal's runners are not running or one or more pools are
  failing. Compare
  runner-name prefixes with each entry's `name` and `slots` to identify the affected
  pool; see Recovery.
- **Marshal:** the window shows the runners running, every pool running, and no
  image problem. A pool that did not start shows its reason. A footer warning about
  the image is a freshness warning; see
  [Rebuilding the Image](./windows-image.md#rebuilding-the-image-for-operating-system-updates).
- **Machine:** `docker ps` shows `slots` running containers per entry, named
  `<name>-<index>-<timestamp>`, whether idle or busy, since an idle container is
  a runner waiting for a job. None means the same as above.
  `docker inspect` on a container shows its limits under `HostConfig`: `NanoCpus`
  is the CPU-time quota in billionths of a CPU, `CpusetCpus` is the requested
  CPU set, and `Memory` and `MemorySwap` are both `memoryGb` gigabytes in bytes.
  Requested settings are not effective readback. Each job container's output in the
  agent's log has the guard's bounded `CPU affinity verified` line, which reports
  the kernel-granted set; the authorized real-host check also resolves the actual
  cgroup cpuset/quota layout.
- **Tokens:** the Tokens tab shows a token set for every repository a pool names.
- **End to end:** for each pool, a workflow run whose `runs-on` matches that
  entry's routing label starts executing rather than sitting queued.
