---
type: Policy
title: "Isolation on the macOS runner host"
description: "Decide which resources each Tart VM may reach, including Softnet, the VM-local Docker socket and shared host services."
---

# Isolation on the macOS runner host

A Linux or macOS job on axmc receives its own VM. The hypervisor, network filter, base images and accepted host-facing interfaces determine what the job can reach.

## A Linux Job on axmc Runs in a Per-Job VM

The axmc host runs each `os: "linux"` pool's job in its own Tart virtual machine
(VM), cloned from the golden VM for that job and deleted after it, so a job meets a
single-tenant kernel and filesystem that nothing outlives. This section and the
five after it are the constraints the VM runtime of
[#74](https://github.com/axross/runners/issues/74) is reviewed against.
[macOS Runner Host](../runner-hosts/index.md#macos-with-tart) owns the procedure, and
[the Tart backend](../marshal-cli/tart-backend.md) the runtime.

- A job VM MUST be a fresh clone of the golden VM, and MUST be deleted when its job
  ends, on every failure path, at a stop, and by the next `start` if the CLI died.
  A shim per VM holds its Tart process and deletes the VM when the CLI's liveness
  pipe closes, so a hard-killed CLI leaves no VM behind. Cleanup at `start` matches
  only the job VMs of configured pools by their name form, so it never deletes a
  golden VM, a build VM, a VM of an unconfigured pool, or a VM `marshal` does not
  own.
- A job VM MUST share no host directory, clipboard or audio, MUST attach no USB
  accessories, Rosetta or display, and MUST NOT accept the base image's password
  SSH login, which the golden VM disables. A shared directory is a path from the
  job to the Mac's files, and Rosetta, the clipboard and the accessories widen the
  host-facing surface for no job's benefit.
- The JIT configuration MUST reach the guest only through the guest agent's
  standard input, and inside the VM only the golden VM's launcher passes it to
  `docker run`, in the Docker client's environment, as on axpc. It MUST NOT be an
  argument of any `tart` call, appear in a log line, or be written into the VM's
  image.
- Guest-agent output is data. Each call's captured output MUST be cut off at a fixed
  bound (64 KiB), and `marshal` MUST NOT parse it beyond its expected form. Logged
  text is scrubbed and masked as container output is. The one exception is the
  diagnostics export, which streams the `docker cp` archive out of the VM: the
  strict archive reader and the bundle limits of
  [Diagnostic Evidence Stays Private](./local-agent.md#diagnostic-evidence-stays-private) bound it
  instead.
- Every `tart` call MUST set `TART_NO_AUTO_PRUNE`, so a clone never deletes an image
  another program cached; MUST get `marshal`'s own `PATH`, which `marshal` also uses
  to find `tart` and `softnet` (for the Mac app's agent, the fixed
  `/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin` the app gives it, so no
  directory the user's shell adds is searched); and MUST have a standard output that is not a
  terminal, so Tart never offers to change Softnet's privilege. Only `build`
  deletes golden and build VMs.
- A VM MUST have a fixed vCPU count, memory size and disk cap, the pool's `cpus`,
  `memoryGb` and `diskGb`, so one job's use of the Mac's CPU, memory and disk is
  bounded by its pool. The disk cap bounds what a root job can write to the Mac's
  disk.

## CPU Placement Inside a Job VM

Inside a job VM, [CPU Placement Is Not Resource Reservation](./pool-isolation.md#cpu-placement-is-not-resource-reservation)
holds with one difference: the VM is single-tenant, so the runner container's quota
and affinity MUST equal all of the VM's vCPUs, 0 to `cpus` - 1, and its memory limit
MUST equal the VM's memory. Nothing else runs there to share CPUs with, so no
allocation among slots is needed, and the VM's own vCPU count is the bound. The
probe and the guard that read the effective placement back MUST run through the
guest agent, and a failed probe MUST end the slot's cycle before a runner
registers. A job that exhausts memory meets the guest's out-of-memory killer, and
the VM is deleted either way. The Mac's scheduler, not `marshal`, shares its cores
among VMs, so a pool's `cpus` is a count of vCPUs, not a reservation of the Mac's
cores.

## Softnet Confines a Job VM's Network

Every axmc VM runs on Softnet, Tart's filtering network, so a job cannot reach the
local network, another VM, a multicast group or an IPv6 address, and reaches the
Mac itself only for DNS. No acceptance in
[LAN Egress From Runner Containers (Accepted Risk)](./pool-isolation.md#lan-egress-from-runner-containers-accepted-risk)
extends to axmc: that section is about Docker Desktop's default network, and a VM on
Tart's default network would reach the LAN, the host and other VMs.

- Each `tart run` MUST pass Softnet two rules. `--net-softnet-allow` carries
  `out proto udp @host port 53,out proto tcp @host port 53`, and
  `--net-softnet-block` carries `out @host,224.0.0.0/4`. A rule with a port outranks
  one without, so DNS to the Mac's address on Softnet's bridge (the gateway) passes
  and every other packet to the gateway and all multicast do not. With Softnet's
  defaults, which block the private, shared, link-local and loopback IPv4 ranges and
  every IPv6 frame, a VM can then send only to globally routable IPv4 addresses and
  to the gateway's DNS. Softnet's defaults alone let a VM reach every port on the
  gateway, a Mac service listening on all interfaces among them, and forward
  multicast, so they are not enough. A `tart run` MUST carry no other network option.
- Softnet's privilege is a SUID bit on its binary, with no `sudoers` rule.
  `marshal` MUST check it before every VM boot and MUST NOT set it. It finds `softnet`
  as Tart does, as the first match on the `PATH` it gives Tart, follows links, and
  requires a regular file owned by root, with the SUID bit set and no write
  permission for its group or others. Otherwise it refuses and boots nothing. A
  `softnet` that an unprivileged user can write would let that user run code as
  root, and Homebrew installs a new binary without the bit at each upgrade, so the
  check runs at every boot rather than once. `marshal` never handles a root-owned
  file itself, and a `softnet` that does not answer `--version` counts as a
  refusal. Only a debug build can name the `tart` client (`MARSHAL_TEST_TART`) or
  replace the owner and mode read from Softnet's file
  (`MARSHAL_TEST_SOFTNET_OWNER_MODE`), for the tests; a release build always runs
  `tart` and judges the real file. `cli/tests/release_overrides.rs` checks this,
  and the Merge Checks step "Test That A Release Build Ignores The Debug-Only
  Overrides" runs it in the release profile. Likewise, only a debug build can name
  the `route`, `arp` and `caffeinate` programs (`MARSHAL_TEST_ROUTE`,
  `MARSHAL_TEST_ARP`, `MARSHAL_TEST_CAFFEINATE`) or script the sleep, the primary
  network and the keep-awake hold from a directory (`MARSHAL_TEST_SIGNALS`); a
  release build runs `/sbin/route`, `/usr/sbin/arp` and `/usr/bin/caffeinate` and
  reads the Mac's own signals, and the same test checks both.
- Softnet's rules make it track every flow a VM starts, in a table of 32,768
  entries that refuses new connections while full. A job that opens more than that
  within about 5 minutes has new connections refused, and the failure stays inside
  that job.
- The filtering rests on a reading of Softnet's source, which already contradicts
  Tart's documentation about host access. No host run has probed it, and the
  maintainer accepted it as unverified
  ([#74](https://github.com/axross/runners/issues/74#issuecomment-6784246305)).

## The Accepted Host-Facing Surface of a Job VM

These are what an axmc job VM can still touch, or depends on, and the maintainer
accepts them. The acceptance holds only while no public repository and no fork pull
request reaches axmc: a pool serves private repositories whose workflows the
maintainer controls. A change that routes a public repository or a fork pull
request to axmc ends it and needs a new decision.

- **Softnet**, a SUID-root component that every axmc pool depends on. A flaw in it
  is a flaw in the Mac's root boundary, which is why it is pinned and checked.
- **Tart**, a pinned external program that `marshal` calls and never bundles.
- **Guest-agent output**, which the CLI bounds and treats as data.
- **Host disk use by a root job**, limited by the per-VM disk cap.
- **Root in the VM, held by any job**, since the VM-local Docker socket gives it with
  one `docker run --privileged`. The job can send raw frames and rewrite the guest's
  network stack, so Softnet's filtering is the only network boundary in front of the
  host. It can also replace the guest's binaries, including the guest agent, so
  guest-agent output is controlled by the job and bounded only by the 64 KiB cut-off
  and, for the diagnostics export, the strict archive reader.
- **A globally routable address on one of the Mac's interfaces**, which Softnet's
  global allowance would reach through the gateway. The maintainer runs the host on
  networks that give the Mac private addresses.
- **DNS through the Mac's resolver**, which can answer the names of the Mac's
  networks and VPNs, though their addresses stay blocked.
- **The JIT configuration**, which the job can read inside its VM. It is single-use
  and already in the runner's environment.

## The VM-Local Docker Socket

From image contract 2, the runner container of an axmc job mounts the Docker socket of
the VM it runs in, `/var/run/docker.sock`, so a job step can use `docker run`. The
socket belongs to the VM's own Docker Engine, in a VM that is cloned for one job and
deleted afterwards, and not to a host, so no other job and no host process shares it.
That is why the rule that a container never mounts a container socket does not reach
it. The permission is narrow:

- The runtime that mounts it is the VM runtime, and only into the runner container,
  as the one extra option `--mount type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock`
  of `vm_job_run_arguments`. The Docker runtime on axpc MUST NOT mount any socket,
  because Docker Desktop's engine is the host's and serves every pool, and its list,
  `job_run_arguments`, carries none. `marshal` MUST NOT start a runner container
  privileged, with a host's Docker socket, or with any other bind mount. Tests in
  `docker_backend.rs`, `tart_backend.rs` and `runtime::docker_args` hold each side,
  and the socket mount is part of the fingerprint in `image::contract` that approves
  contract 2.
- A job that can use the socket is root in its VM: it can start privileged containers,
  mount the VM's filesystem, and read the JIT configuration, which was already in the
  runner's environment. Before the socket a job was root only inside an unprivileged
  container, so root in the VM needed a container escape; now one
  `docker run --privileged` gives it. The container stops being a layer in front of
  Softnet and the guest-agent channel, and
  [the accepted host-facing surface](#the-accepted-host-facing-surface-of-a-job-vm)
  names what that root can touch. It reaches nothing beyond the VM except what the VM
  can already reach, under [Softnet's rules](#softnet-confines-a-job-vms-network), the
  per-VM disk cap, and the VM's own vCPUs and memory.
- The containers a job starts through the socket are siblings of the runner container
  inside the VM, not children of it. The runner container's CPU quota, affinity and
  memory limit, which [CPU Placement Inside a Job VM](#cpu-placement-inside-a-job-vm)
  holds equal to the VM's, do not reach them, and the VM's vCPUs and memory bound them
  instead.
- The socket's group is the VM's `docker` group, which the golden VM creates with the
  id 123 (`golden::pins::DOCKER_GROUP_ID`). The runner image's `docker` group, which
  its `runner` user belongs to, has that id too. Docker gives a container's user the
  groups that the image's `/etc/group` lists for it, so the runner uses the socket
  with no `--group-add` and no change to the container's user. The match grants
  nothing the mount does not, since the socket is root in the VM already. A base
  image that gave the group another id fails the image build, and a VM whose `docker`
  group has another id fails the golden VM build.
- The runner image's Docker client is pinned like the golden VM's Docker Engine; see
  [Tart, Softnet, and the Golden VM Are Pinned](#tart-softnet-and-the-golden-vm-are-pinned).

The permission holds only inside a per-job VM. A change that mounts a socket on axpc,
mounts a host's socket, or gives a VM to more than one job ends it and needs a new
decision. [Mac verification status](../runner-hosts/mac-verification.md#verification-status) records that the
mount and the group match have not run on a Mac.

## A macOS Job on axmc Runs in a Per-Job macOS VM

An `os: "macos"` pool's job runs in its own Tart macOS VM, cloned from the golden
macOS VM for that job and deleted after it. The constraints of
[A Linux Job on axmc Runs in a Per-Job VM](#a-linux-job-on-axmc-runs-in-a-per-job-vm)
hold for it: the clone and deletion, the shim, the settings that share and attach
nothing, `TART_NO_AUTO_PRUNE`, the bounded guest output, and the fixed vCPUs, memory
and disk. [CLI](../marshal-cli/macos-pools.md#macos-vm-pools) owns the runtime and
[macOS Runner Host](../runner-hosts/macos-vm.md#the-golden-macos-vm-and-macos-pools)
the procedure. The differences are these:

- **The guest is open inside.** SIP and Gatekeeper are off in the base, and a job
  has root through `sudo`. This is accepted, because the VM serves one job and is
  deleted afterwards. The job can read its own JIT configuration inside the VM, as
  on Linux.
- **The JIT configuration** reaches the launcher only on the guest agent's standard
  input, and the launcher passes it to the runner in `ACTIONS_RUNNER_INPUT_JITCONFIG`.
  It is never an argument of any call.
- **Remote access is off.** The golden macOS VM's build turns off Remote Login and
  Screen Sharing, so the base's known `admin` password opens no network login.
- **Guest OS fixes arrive only with a new base pin.** The build turns off the
  automatic download and installation of macOS updates and leaves the automatic
  check on, so a job never meets a half-installed update; the base-age warning from
  30 days prompts a bump.
- **The base is a third-party image** with preinstalled tools, trusted by its pinned
  digest. A base bump reviews what Cirrus Labs changed.
- **The runner archive is checked.** The build installs
  `actions-runner-osx-arm64-<version>.tar.gz` only when its SHA-256 equals the runner
  record's. GitHub's JIT configuration turns the runner's self-update off, so the
  checked archive is the only runner a job VM runs.
  `MARSHAL_TEST_RUNNER`, which declares another runner and digest, works only in a
  debug build.
- **At most two macOS VMs.** Apple's macOS licence, §2B(iii) in both the macOS
  Tahoe 26 and the macOS 27 licence, allows "up to two (2) additional copies or
  instances" in virtual machines on a Mac the licensee owns or controls, for
  software development, testing during development, macOS Server, or personal
  non-commercial use, and excludes service-bureau, time-sharing and similar
  services. The runtime counts the macOS VMs before every clone, as
  [CLI](../marshal-cli/macos-pools.md#the-two-vm-count) states, over-counting when it cannot
  read a VM's OS.
  The allowance assumes the Mac's macOS is licensed under §2B, as macOS installed or
  updated by download is; a copy licensed only under §2A carries no right to virtual
  copies.
- **CPU placement** is the VM's vCPUs, with no container inside.
- **Untested.** Softnet's filtering and the isolation settings are not tested for
  macOS job VMs, as for Linux ones. M1's condition carries over: macOS pools take
  jobs only while no public repository and no fork pull request reaches axmc.

## Tart, Softnet, and the Golden VM Are Pinned

`marshal` neither bundles, installs nor repairs Tart or Softnet: the maintainer
installs both with Homebrew, and `marshal` calls them as external programs. What
they and the golden VM are built from is pinned, so that a rebuild changes only
what a reviewed change changes.

- Tart and Softnet are each pinned to one tested version, recorded in the CLI and
  in [macOS Runner Host](../runner-hosts/mac-setup.md#installing-tart-and-softnet).
  The tested version runs. A newer patch of the tested minor version runs with a
  warning that names the tested version. Any other version, and any output other
  than three dot-separated numbers (which Softnet alone follows with
  `-<short commit>`), is refused. A minor release can change isolation flags or
  `tart exec`, so an untested minor version does not run. Each host run records the
  versions in use.
- The golden VM's base image, `ghcr.io/cirruslabs/ubuntu:24.04`, MUST be pinned by
  digest. A change of the pin adds the old digest to the CLI's list of retired
  pins, and `build` removes from Tart's cache only the digests on that list, never
  the current pin.
- Docker Engine in the golden VM MUST come from Docker's apt repository at pinned
  package versions, held there, after the repository's signing key is checked
  against a recorded fingerprint. A mismatch fails the build.
- The golden VM is built on each host and never published to a registry. A rebuild
  upgrades the base's Ubuntu packages, so it picks up security fixes; the golden VM
  version goes up whenever the pin, the provisioning steps, the launcher or the
  Docker Engine versions change.
- The runner image's Docker client MUST come from Docker's apt repository at a pinned
  package version, after the repository's signing key is checked against the same
  recorded fingerprint, and the image keeps neither the repository's source nor its
  key. The image's pins are the golden VM's, and a unit test holds the Dockerfile to
  `golden::pins`, so a change of one without the other fails.
- The runner image's arm64 build and smoke test in CI run on `ubuntu-24.04-arm`, a
  GitHub-hosted runner, tag the image on the runner and never push it, and their job
  holds only `contents: read`.
- The macOS base, `ghcr.io/cirruslabs/macos-tahoe-base`, MUST be pinned by digest in
  `MACOS_BASE`, with its macOS version and publication day, and its retired digests
  follow the same rule as the Ubuntu base's.
- The VM runtime adds no crate to the CLI.
