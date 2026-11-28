---
type: Procedure
title: "Verify a runner on Mac hardware"
description: "Exercise a private job on the Mac and distinguish host evidence, CI stand-ins and behavior still awaiting a hardware check."
---

# Verify a runner on Mac hardware

## Verification Status

**Only the runs this section names have run on a Mac.** A claim about the
behaviour of Tart, the guest agent, Softnet, `caffeinate`, `route` or `arp` rests on
a run only when this section links that run's evidence. By the maintainer's decision
[recorded on #74](https://github.com/axross/runners/issues/74#issuecomment-6784246305),
the axmc procedures were added with no host run. The one host run planned
comes with the arm64 runner image, and checks only that a Linux job that uses
`docker run` runs in a Tart VM with no Docker Desktop, that closing the lid or
switching networks stops the runners, and that `stop` leaves no job VM while the
golden VM stays. Its evidence, with the literal output of `tart --version` and
`softnet --version`, is posted on #74. An item below moves to a verified statement,
with a link to the evidence, only when a run's evidence covers it.

**The host run.** Jobs come from a workflow in a private repository the maintainer
chooses, since this repository is public and never uses self-hosted runners. After
the routing check in [Starting, watching and stopping](./mac-pools.md#starting-watching-and-stopping),
the run goes in this order:

1. Confirm that no Docker Desktop is installed, and record the macOS version, the
   literal output of `tart --version` and `softnet --version`, and Softnet's SUID bit
   as [Setting Up Softnet](./mac-setup.md#setting-up-softnet) lists it.
2. Install the last command-line release's macOS binary with the macOS procedure at
   its tag, since no later build carries one. Open
   `https://github.com/axross/runners/blob/v<version>/docs/operations/cli-recovery.md`, replacing `<version>` with the CLI recovery release's version without its initial `v`. Record its version, the commit of its tag and the
   program commit it was built from.
3. Build the golden VM, then start the pools, with that release's `build` and
   `start`, as its [program tree’s host procedure](https://github.com/axross/runners/blob/dc8e1ebcc0722997206487f3dcca2ae7e519cf02/docs/operations/macos-runner-host.md#starting-watching-and-stopping) describes, with the
   sizing of [the example configuration](../../hosts/macos/runner-host.example.json).
   Record the golden VM's and Tart's cache's disk use.
4. Run a job with `runs-on: [axmc, linux]` and a step that uses `docker run`, and
   confirm with `tart list` that the job's VM runs while it does. Record the
   runner's labels and the slot's turnaround. This step also covers the mount of the
   VM's Docker socket and the match of the two `docker` groups.
5. With no external display attached, close the lid for about 15 s during a job, and
   switch networks during another job. Confirm that each stops the runners, with the
   reason in the status, and that `tart list` then shows no job VM. The Mac
   takes several seconds to enter sleep after the lid closes, so a close of about 5 s
   can sleep for less than the 3 s a sleep needs to be seen; on the first host run it
   slept about 3 s and nothing stopped.
6. Stop the instance with its `stop`, then confirm with `tart list` that no job or
   build VM is left
   and that the golden VM stays.

**The macOS checks.** The
[first Mac check](https://github.com/axross/runners/issues/80#issuecomment-6816684419),
on macOS 27.0 with Tart 2.41.1 and without using Softnet, booted clones of the pinned macOS base
without `marshal`. It recorded `tart get`'s `"OS" : "darwin"` for a macOS VM, Tart's
exact VM-limit error with two macOS VMs running, a Linux VM booting beside them, so Linux VMs do not count against the limit, a
clone's disk growing to 80 GB at boot with the recovery partition kept, and the
guest agent running commands as `admin`. Mac verification
[steps 4.1 and 4.2](https://github.com/axross/runners/issues/80#issuecomment-6871092617)
then ran `marshal build` at `bc5542c`, with a macOS pool and the base already in
Tart's cache, so the pull found it cached, and read the golden macOS VM through the guest agent: its runner, the base's runner gone, Remote Login and Screen Sharing off,
and the automatic download and installation of updates off. Mac verification
[steps 4.3 to 4.11](https://github.com/axross/runners/issues/80#issuecomment-6889174071)
then ran a release build at `1400199`: a macOS job on a runner behind GitHub's
latest, which did not update itself because GitHub's JIT configuration sets
`DisableUpdate: true`; at most two macOS VMs, with Linux VMs starting beside them;
the limit wait for a VM Marshal did not create, and Tart's refusal at boot; an
80 GB `diskGb`; a sleep during a macOS job, which stopped every runner and left no
job VM; a hard kill, after which the shims deleted the job VMs; `stop` in 7.5 s; a
boot of 28 to 32 s; and a rebuild within the 100 GB reserve.

**What is checked without a Mac.** The CLI's tests run against fake `tart`, `docker`,
`softnet`, `caffeinate`, `route` and `arp` programs and a scripted signal source:

- the version parsing and sorting of both programs, and the judgment of Softnet's
  file and the search for it on `PATH`;
- the VM runtime's slot cycle, probes, run arguments, release and VM shim, and the
  diagnostics export from a VM;
- the golden VM build, with a failure scripted at each step and at a stop;
- `start` on the axmc profile as a process, through the test helper: Linux and macOS pools in VMs of
  their OS, a macOS slot waiting for the limit, a failed deletion retried, each
  refusal before any VM boots, a held slot, and a refused resume;
- the golden macOS VM build, with a failure scripted at each step and at a stop;
- the signals: a wake and a primary-network change that stop the instance with their
  reasons, a resume that records the network afresh, no VM cloned without a default
  route, and the keep-awake hold's state.

The [runtime test coverage](../marshal-cli/pool-runtime.md#what-the-tests-cover) identifies the suites that exercise the Tart backend and the VM shim.

**What no run has confirmed.** Each of these rests on a source or on the output
of an Apple tool as the source prints it, and not on a run:

- The guest agent runs commands as `admin`, and `admin` is in the `docker` group in a
  job VM without `sudo` (from the guest agent's source, not from a running VM).
- In the build VM, the image build works with `sudo -n docker`, and the build creates
  the `docker` group on the pinned base, which has no `docker` group with another id.
- A job step's `docker run` works in the runner container against the VM's own Docker
  Engine: the VM's socket is mounted and the `runner` user can use it through the group
  match that [Security](../security/vm-isolation.md#the-vm-local-docker-socket) describes,
  with no `--group-add`. The Docker client in the
  image is the pinned version, and the Engine accepts its API version.
- The image build fits the golden VM's 20 GB disk, with BuildKit's cache, and the
  golden VM, its build and Tart's cache fit the 40 GB reserve; record their sizes.
- `tart delete` removes a cache entry pulled by digest, which is how `build` removes a
  retired base pin. The retired list is empty today, so no run has needed it.
- `route -n get default` under a VPN that takes the default route prints a
  `gateway: link#<index>` or no `gateway:` line, which the CLI reads as an interface
  without a router; and `arp -n <router>` prints the form it reads.
- A sleep is detected: `Instant` stops across a sleep on the pinned toolchain, a short
  lid close of about 15 s and a long one of about 60 s each stop every slot within 10 s
  of the wake, and a network change stops them within 15 s.
- `caffeinate -i -w <pid>` holds the Mac awake while runners run and releases it after
  `stop`, as `pmset -g assertions` shows.
- A locked login keychain makes a boot fail; record what `tart run` printed and
  whether the logged text names [The Signed-In Session](./mac-setup.md#the-signed-in-session).
  Locking the screen after `start` does not stop a new job's VM from booting.
- Softnet's rules hold: from a VM, every blocked target that answers from the Mac is
  unreachable, and DNS and an HTTPS request to the internet succeed; with Softnet's
  SUID bit removed, `start` refuses and `tart run` boots no VM. No host run tests
  this, and the maintainer accepted the isolation as unverified
  ([decision record](https://github.com/axross/runners/issues/74#issuecomment-6784246305)).
- The VM has the pool's vCPUs, memory and disk, and no shared directory, Rosetta share,
  sound card or USB device, and the `admin` password login over SSH is refused.
- For Linux job VMs, `stop` ends every VM within 30 s, a hard-killed `start` leaves no
  running job VM after the next `start`, and a slot's next VM is cloned, booted,
  probed and registered within 60 s of the previous VM's deletion; record the
  turnaround. Mac verification steps 4.9 and 4.10 confirmed the first two for macOS
  job VMs.
- The modes of the control socket, the state, log and diagnostic files, and the
  refusal of another OS user, on macOS.
