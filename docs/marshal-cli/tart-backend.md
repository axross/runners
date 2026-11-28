---
type: Architecture
title: "Tart runtime and golden VM provisioning"
description: "How do the golden VM, job VM, shim and host prerequisites work together?"
---

# Tart runtime and golden VM provisioning

## Tart Backend

`runtime::tart::backend::TartBackend` is the VM runtime. It gives each job a VM
cloned from the golden VM, runs the run contract's Docker commands inside it, and
deletes it. `start` constructs it for the axmc profile, with the VM shim as its
launcher; `build` does not use it. The in-process tests build it through
`cli/tests/support/rig.rs` against the fake `tart`, `docker` and `softnet`, and
`cli/tests/axmc_start.rs` runs it through `start`. The `tart` client library under it is documented where its
parts are: [Host Prerequisites](#host-prerequisites),
[Golden VM Provisioning](#golden-vm-provisioning), and the argument lists in
`tart::args`.

**Availability.** `wait_until_available` runs `tart list` and, while it fails, warns
`Tart is not responding yet (not installed, or not answering) - waiting.` and tries
again every 5 s. A Tart that does not answer is therefore never a failure of the
runtime core. A missing `tart` is refused at `start` before this wait is reached, as
[Host Prerequisites](#host-prerequisites) maps it. `check_image` runs the same listing within the image check's budget and
reads the golden VM from the VM names alone ([The Image Contract](./runner-images.md#the-image-contract)
describes the check's outcomes). A listing that fails or overruns is
`Unavailable`, which registers nothing and goes into the backoff.

**The golden VM check.** `tart::golden_check` reads `tart list --format json` and no
VM's contents. A golden VM's name,
`marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`,
records what the image's labels record, so nothing boots. The newest golden VM by its
build second decides, whatever its versions. It matches when its golden VM version
and image contract version are this CLI's, and a runner version that differs is not a
mismatch, as for an image. It feeds the image check's outcomes, so the texts and
refusals are today's, naming the golden VM instead of an image: no golden VM is
missing, and one built for another golden VM version or contract version is
mismatched and asks for a rebuild. A unit test in `image::check` pins the case of a
golden VM whose old runner asks for a rebuild.

**A slot's cycle.** `prepare_job_host` takes these steps and answers `Ready`,
`Failed`, or `Held`:

1. A host this slot could not release earlier is released first, within 30 s, so a
   slot never owns two VMs. Its failure fails the preparation.
2. When the instance's signals have recorded no primary network, the preparation
   waits at the network gate first; a stop ends the wait. Then the prerequisites run:
   Tart's version, then Softnet's file and version, as
   [Host Prerequisites](#host-prerequisites) maps their causes, so a boot after a long
   wait uses a check made after it. A Tart that is
   missing or does not answer warns `Tart is not available (<message>) - waiting.`
   and tries again every 5 s; a refusal of Tart's version or of Softnet is `Held`
   with the refusal's text. The versions in use are logged once, as
   `Using Tart <v> and Softnet <v>.`, and again when they change; each warning is
   logged once until it changes.
3. The newest golden VM that runs here is listed and cloned to
   `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`. The name is recorded before the
   clone runs, so a clone that fails halfway is still deleted.
4. The clone is sized with `tart set --cpu <vcpus> --memory <MB> --disk-size <GB>`
   from the pool's `cpus`, `memoryGb` and `diskGb`.
5. `tart run` (headless, with Softnet) is started by the [VM shim](#the-vm-shim),
   and the backend asks the guest agent whether it answers, every second with each
   question bounded at 5 s, until it does, `tart run` ends, or the 120 s boot
   deadline passes.

A boot that fails is `Failed`, with the text
`boot failure: <tart run for the job VM <vm> ended with exit code N (<Tart's own error>) | the guest agent of the job VM <vm> did not answer by the boot deadline (<last error>)>`.
When `tart run` ended, a hint follows if its whole output, read before the text is
cut to 500 characters, carries one of these:

- `SecKeyCreateRandomKey_ios failed`, `Failed to generate keypair` or
  `Interaction is not allowed with the Security Server`, the texts Tart's FAQ lists
  for a locked login keychain:
  `; Tart's error points to a locked login keychain, see the "The Signed-In Session" section of the macOS Runner Host document`.
- `Softnet process terminated prematurely` or `softnet not found in PATH`, the two
  Softnet errors of Tart 2.41.1 that the runtime's `tart run` can produce:
  `; Tart's error points to Softnet, see the "Setting Up Softnet" section of the macOS Runner Host document`.
- At least one of each:
  `; Tart's error points to a locked login keychain, see the "The Signed-In Session" section of the macOS Runner Host document, and to Softnet, see the "Setting Up Softnet" section of the macOS Runner Host document`.

Any other boot failure has no hint, including Virtualization's generic
`The virtual machine failed to start.` and a guest agent that did not answer. The
sections are [The Signed-In Session](../runner-hosts/mac-setup.md#the-signed-in-session)
and [Setting Up Softnet](../runner-hosts/mac-setup.md#setting-up-softnet).

**Probes.** `select_cpuset` finds the slot's VM from the probe's name and waits up to
60 s for the Docker Engine in it (`docker info` every 2 s, each call bounded at
10 s). The wait ends early, with `CPU discovery: the job VM was released.`, once the
VM's release has begun. A stop ends it this way: the slot does not watch for the
stop here, and the release that the stop sequence runs deletes the VM. Then it runs the CPU probe's `docker run` through the guest agent within the 30 s the
probe has. The output is read as the container backend reads it, capped at 64 KiB,
ASCII only, and selected with the same function, so a VM's affinity is chosen from the
VM's own CPUs. A probe that fails registers nothing, and the cycle releases the VM. The
VM has the pool's whole `cpus` as vCPUs, so the quota and the affinity of the job
container cover all of them and its memory limit equals the VM's.

**The job container.** `run_container` runs
`tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>` where the
arguments are `docker_args::vm_job_run_arguments`, the container backend's own
`docker run` list with the VM's Docker socket added: `--pull never`, the
name, quota, affinity and memory options,
`--mount type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock`, and
`-e ACTIONS_RUNNER_INPUT_JITCONFIG` with no value. The JIT configuration is written to the client's standard input and
the pipe is closed. It is in no argument list, in no environment variable of `tart`,
and in no log line. The launcher reads it from standard input, exports it, and
`exec`s `docker`. Standard output and error are forwarded to the log line by line
and masked, as for the container backend. A volume is refused in a VM
(`volumes are not supported in virtual machines`), and `validate` already refuses
one on axmc. The text a guest command returns to the backend is cut at 64 KiB; the
one exception is the diagnostics archive, which the Diagnostics paragraph below
streams through the strict archive reader and the bundle limits instead.

**The VM-local socket.** From image contract 2, the runner container mounts the Docker
socket of the VM it runs in, so a job step can run `docker` against that VM's own
Docker Engine. The socket belongs to a VM that one job uses and that is deleted
afterwards, and never to a host. The Docker runtime's list, `job_run_arguments`,
mounts none, and `vm_job_run_arguments` adds the mount to it and to nothing else:
the CPU probe and the volume commands still carry no socket mount. Tests in
`docker_backend.rs`, `tart_backend.rs` and `runtime::docker_args` hold both sides.
The socket's group is the VM's `docker` group, which the golden VM creates with
`golden::pins::DOCKER_GROUP_ID` and which the runner image's `docker` group matches;
[Golden VM Provisioning](#golden-vm-provisioning) says where each build checks it,
and [Security](../security/vm-isolation.md#the-vm-local-docker-socket) says why the
match needs no `--group-add`.

**Which user runs it.** `tart exec` reaches the guest agent,
[cirruslabs/tart-guest-agent](https://github.com/cirruslabs/tart-guest-agent) at
commit `6d4bd9c`. Its systemd unit, `packaging/tart-guest-agent.service`, sets
`User=admin`, and `internal/rpc/exec.go` starts the command as the agent's own user
with no user switch, in a process group of its own that it kills when the call is
cancelled. A command therefore runs as `admin`. The golden VM adds `admin` to the
`docker` group, which applies from the agent's start, so `docker` runs without
`sudo` and the backend never calls it.

**Stop and release.** `stop_containers` and `kill_containers` run one `docker stop
--time <n>` or `docker kill` per VM through the guest agent, all in parallel, and
`remove_containers` runs `docker rm -f` in the container's VM. A name that is a job
VM, which only a leftover from an earlier run is, is stopped forcibly and deleted.
`release_job_host` gets rid of the slot's VM within the budget it is given:

- with a VM that is running, it asks for a graceful stop, stops it forcibly when that
  fails, deletes it, and, when the deletion fails, lists the VMs to see whether it
  is already gone. The budget is split in tenths: 5 for the graceful stop (its
  `--timeout` is a second less, so Tart ends its own wait first), 2 for the forced
  stop (`stop --timeout 0`), 2 for the deletion and 1 for the listing. A VM whose
  holder has ended is deleted without a graceful stop, and a deletion that fails
  then is retried once after a forced stop, because a killed VM shim leaves its
  `tart run` running and Tart refuses to delete a running VM;
- it is idempotent and safe to run twice at once: the second waits for the first
  and finds the host released;
- a release that succeeds forgets the host, its containers, and the holder of its
  `tart run`. A release that fails or runs out of its budget keeps all of them, so
  the next preparation of the slot or the next release deletes the VM.

**The release budget split.** The stop sequence of [Stopping](./foreground-commands.md#stopping) takes the
backend's reserve out of its container steps. The Tart backend reserves 10 s
(`release_budget`), so the container stop is ended at 12 s with a grace of 7 s, the
removal takes 6 s more, and the release the remaining 10 s, which ends at 28 s, the
exit deadline; a diagnostic container's stop, export and removal end 6 s earlier for the
same reason. The container backend reserves nothing, so its steps are unchanged.
The 10 s is the CLI's: 5 s graceful, 2 s forced, 2 s deletion and 1 s listing. A VM
the CLI could not delete within it is deleted by its [VM shim](#the-vm-shim), which
has its own 15 s once the CLI has dropped the liveness pipe, even after the CLI has
exited. The worst path is 12 s + 6 s + 10 s = 28 s, with no slack, so a double fault,
a `tart` call that hangs to its bound, can leave a VM for its shim, which deletes it
within 15 s after `start` exits, or for the next `start`. A VM that neither deleted
is a leftover, and the next `start` deletes it.

**Leftovers.** At the start of a pool's preparation, `list_pool_containers` finds
the local VMs named `marshal-job-<pool>-<slot>-<17 digits>` for the configured
pools, and the runtime removes them, each by a forced stop and a deletion
(`deletes_leftovers_unexported`). Nothing is exported from a leftover VM. The log
says `Deleting job VM '<name>' left over from an earlier run.`; for a
diagnostics-enabled pool, it also says that its diagnostic evidence is gone, in
today's gap wording. A leftover that cannot be deleted skips its pool, as a labelled
container that remains does on the container backend: the warning says the pool "is
not started and gets no slots" and names the VMs, and the other pools start. Only the
pool's own VMs are matched, so another pool's, a golden VM and a build VM are left
alone.

**Diagnostics.** A diagnostic container is recorded as retained when it starts, and
the runtime finalises it, exporting before it removes the container, and releases
the host only after that, as the [worker's order](./pool-runtime.md#names-and-the-slot-loop) has it.
For a VM, the export runs the exporter child with `--vm <name>`, which reads the
evidence through `VmCopy` (`tart exec <vm> docker inspect` and `docker cp`), with
the identity cap, the strict archive reader, and the bundle limits of
[Diagnostics](./diagnostics.md#diagnostics), and `count_bundles` counts the pool's bundles through
the same child. The VM is the retained container: it stays alive after its job
until the export has run, and the removal of a retained container is the deletion
of its VM, by the release above, within the removal's 24 s clock, where a container
backend would run `docker rm -f`. A failed deletion keeps the container retained
for `diagnostics`, as a failed removal does. The worker does not release a stopping
slot's diagnostic host itself (`host_awaits_finalisation`), because the stop
sequence exports it first and then releases it; without that, the slot's own
release would delete the VM while the export reads it. The warnings and gaps are
today's.

**Evidence a VM loses.** A diagnostic VM is deleted without an export in three
cases: a second SIGINT, which skips the export and deletes the VM where a container
would be killed and kept; a `start` that ends while it still holds the VM, whose
[VM shim](#the-vm-shim) deletes it when the liveness pipe closes; and the next
`start`, which deletes a leftover. The shim also deletes a VM that the CLI's release
could not delete before `start` exited; for a diagnostic VM the export was attempted
in the stop sequence before that release, so this is not a fourth case, but the shim
cannot tell that the export succeeded, so its gap line can name evidence that stands
in the sink. Each reports the loss with today's gap wording,
`Diagnostic gap in pool <pool>, job VM <vm>: <text>`, or, for a forced stop,
`container <name>` in place of the VM. A forced stop keeps the container names in
its result, so `start` exits 1 as for a kept container and its last line says that
the job VMs were deleted without an export. A container whose export had succeeded
is not among them: when a release deletes its VM, the backend forgets it, so that
line names only evidence that was lost. The forced stop's warning says the VM
"will be deleted" when the hosts are released, because the deletion can still fail.

Every VM of a diagnostics-enabled pool that is deleted in these cases is reported as
a diagnostic gap, whether or not a job ran in it, because the CLI cannot tell a VM
that holds evidence from an idle one without booting it. The next `start` reports a
leftover before it deletes it (`delete_leftover_host`), so a leftover whose deletion
then fails is still reported, and the VM shim reports a VM only once it has deleted
it, and none that was already gone.

## The VM Shim

The VM shim is the CLI run again as the hidden `marshal vm-shim --name <vm> -- <tart>`
on Unix. It starts `tart run` for the VM, with the arguments of `tart::args`, in a
process group of its own. Its standard input is the read end of a liveness pipe whose
write end only the CLI holds. It holds no lock: the instance lock is never passed to
it, and a test shows the lock free while a shim lives.

- When `tart run` ends while the pipe is open, the shim relays what Tart printed, its
  standard error and then its standard output, to its own standard output, and exits
  with Tart's code, or 128 plus the signal number when a signal ended it. It deletes
  nothing, because the CLI is still there and deletes the VM itself: a job VM when its
  slot releases it, and a build VM when the build fails (one that stopped after a
  successful build is renamed to the golden name instead). A VM the CLI could not delete is left to the next
  `start`, for a job VM, or the next `build`, for a build VM.
- When the pipe closes, the shim stops the VM (`stop --timeout 5` within 8 s, then
  `stop --timeout 0` within 3 s when that fails), deletes it within 4 s, and exits 0.
  It exits 1 when the deletion fails or the 15 s run out. A shim started for a
  diagnostics-enabled pool's VM (`--diagnostic-pool <pool>`) also writes
  `WARNING: Diagnostic gap in pool <pool>, job VM <vm>: <text>` to its standard
  error, which it shares with the CLI, once the VM is deleted. It cannot tell a VM
  holding evidence from an idle one, so it reports each VM it deletes, and none that
  was already gone. None of its `tart` calls
  prunes, which `TART_NO_AUTO_PRUNE=1` sets.

The shim takes any VM name, so the build command can hold a build VM the same way.
`HeldVm` is the CLI's end: it starts the shim, keeps the write end of the pipe, and
closes it when it is dropped.

## Host Prerequisites

The axmc runtime runs two programs it does not ship: `tart`, and `softnet`, which
Tart runs. `macos::prerequisites` checks both and returns a `Check`: a pass, a
warning, or a refusal that carries a `Cause` and its message. The
[Tart Backend](#tart-backend) calls it before each boot, `start`, `restart` and
the resume of a stopped instance call it before anything is started, and
[`build --profile axmc`](./build-images.md#the-golden-vm-build) calls it before it creates any VM. The tested versions are constants in `macos::version`, Tart 2.41.1 and Softnet
0.25.0, and the host document records them in
[Installing Tart and Softnet](../runner-hosts/mac-setup.md#installing-tart-and-softnet);
a change of either is a change of the constant, the document, and the fake programs'
default output.

| Check             | How                                                                                                                                                                                                              |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tart's version    | `tart --version` through the Tart client, within 5 s (2 s at a restart or a resume). The output is exactly three dot-separated numbers and one optional final newline                                            |
| Softnet's file    | The first `softnet` on the `PATH` the Tart client carries, found as Tart finds it (see below); a regular file owned by root, with the SUID bit set and no write permission for its group or others               |
| Softnet's version | `softnet --version`, run on that file, within 5 s (2 s at a restart or a resume). The output is `softnet <three numbers>-<short commit>` and one optional final newline: clap's form for Softnet's release build |

Each version sorts into the tested version, which runs; a newer patch of the tested
minor version, which runs with a warning that names the tested version; or a refusal
that names the tested version and the host document's install section. An older
patch, another minor or major version, a fourth number, a suffix other than
Softnet's commit, and output with no version are refused. Each number has no sign and
no leading zero, and the commit is 7 to 40 lowercase hexadecimal digits. Softnet
0.25.0 is a patch of 0, so an older patch of its minor version cannot exist. The
version reads and the Tart client share one bounded process wrapper: their own
process group, killed when the call is dropped, standard input null, output cut at
64 KiB, and the JIT variable cleared. A `softnet` read gets the same `PATH`.

Tart takes no path for Softnet. It runs the first `softnet` on its own `PATH`, found
by `resolveBinaryPath` in Tart 2.41.1's `Sources/tart/Utils.swift`: each entry in
order, empty entries skipped, the first where the file exists with links followed.
It tests existence alone, so a directory named `softnet` matches, and a link that
leads nowhere does not. `macos::softnet::find_on_path` does the same and the file
check reads that file's type, owner and mode with the standard library's metadata,
which follows links. The judgment is a pure function over the file's type, owner and
mode (`setup_problem`); the first failing requirement in the order type, owner, SUID
bit, group write, other write gives the message. The file is judged before it runs,
so a file that fails never starts. The reader of owner and mode is Unix code; on
another platform it returns an error, which is a refusal.

A refusal carries a cause, so a caller can map it. A missing program or one that
cannot start is `Missing`, a program that does not answer within the bound is
`NotAnswering`, a version that does not run is `Version`, and a file that is not set
up is `Setup`. A missing `tart` or `softnet` at `start` or `build` is refused naming
the install section. A `tart` that does not answer, or goes missing after `start`, is
Tart unavailable and its slots wait. A `softnet` that does not answer counts as a
refusal.

**At `start`, `restart` and a resume.** The instance asks the core to check the
prerequisites once, through the backend's `check_prerequisites`, when the core runs
a plan with a Linux pool on axmc; a plan of macOS pools alone, and every axpc plan,
pass without a check. Every refusal of Tart's check but `NotAnswering` is a
refusal, and so is every refusal of Softnet's, a missing or unanswering `softnet`
included; a `tart` that does not answer is not, because the slots wait for it. Then:

- At a fresh `start` a refusal ends the instance before the runtime core runs: the
  log warns `The instance ends: <the refusal's message>`, which names the host
  document's section, and `start` exits 1. Nothing but `tart --version` and
  `softnet --version` has run, so no VM was cloned, set, or booted.
- At a `restart` or a resume, including a `start` that resumes a stopped instance, a
  refusal leaves the instance stopped with the refusal's message as its reason,
  exactly as an image that cannot run jobs does, and the client exits 1. The check
  runs before the image check. A restart has by then stopped every running job.
- The check at a first `start` gives each version read 5 s. At a `restart` or a resume
  a client is waiting, so each read gets 2 s and the two run together, which keeps the
  reload inside the client's wait, as [The Image Check](./runner-images.md#the-image-check) counts. A
  `softnet` that takes longer than 2 s to answer is a refusal there, as one that takes
  longer than 5 s is at `start`.
- The warnings of a passing check, a newer patch of Tart or Softnet, are logged once
  and shown by `status` as `warning: <message>` lines, listed on every call, after
  the image lines, as of the latest check that passed.

After `start`, the Tart backend maps the causes this way before each boot: a Tart that is
`Missing` or `NotAnswering` makes the slot wait, a Tart `Version` refusal holds the
slot, and every refusal of Softnet holds it, a missing or unanswering `softnet`
included.

A debug build replaces the owner and mode read from Softnet's file with
`MARSHAL_TEST_SOFTNET_OWNER_MODE=<user ID>:<octal mode>`, such as `0:4755`, for the
process tests, because CI cannot create a root-owned file. The
file's type is still read, links followed. A release build ignores the variable, and
a debug build refuses a malformed value. `Check`'s tests are in
`cli/tests/host_prerequisites.rs`, against the fake `tart` and `fake-softnet`
(`cli/helpers/fake_softnet.rs`, documented in its header, which prints Softnet's real
form), and the parsers and the file judgment have unit tests in `macos`.

## Golden VM Provisioning

The `golden` module holds what `build --profile axmc` uses to provision the
golden VM; [The Golden VM Build](./build-images.md#the-golden-vm-build) sequences it. `golden::pins` records the base image by its
digest (`ghcr.io/cirruslabs/ubuntu`, tag `24.04`, arm64), the list of retired base
pins, Docker's apt repository and signing-key fingerprint, the four Docker Engine
packages at their pinned versions (Compose is not installed), the id of the `docker`
group (`DOCKER_GROUP_ID`, 123), and the launcher's path in the VM,
`/usr/local/bin/marshal-launch`. A change to the base digest, the
provisioning scripts, the launcher or the Docker Engine versions raises
`GOLDEN_VM_VERSION`, and a change of the base digest also adds the old one to the
retired list. A test asserts the current base is a digest reference and is not on
that list.

`golden::steps` builds each guest call as a `tart exec` command and the standard
input it takes, from scripts embedded in the binary. The calls run in this order:

1. upgrade the Ubuntu packages and install `ca-certificates`, `curl` and `gnupg`;
2. download Docker's key to `/etc/apt/keyrings/docker.asc`;
3. list the key with `gpg --show-keys --with-colons`;
4. install the apt source, create the `docker` group with the id `DOCKER_GROUP_ID` and
   refuse a group that has another, install the four packages at their pinned versions, hold
   them, and add `admin` to the group (`install-docker.sh`);
5. install the launcher, owned by root, mode 0755;
6. lock the `admin` password and mask `ssh.service` and `ssh.socket`.

`golden::key` compares the listing from step 3 with the recorded fingerprint. It
accepts only one primary key, listed with its fingerprint record, and refuses a
mismatch, a second primary key and anything it does not recognise. Its result is
the argument the call for step 4 requires, so nothing from Docker's repository is
installed before the comparison passes.

The launcher reads the JIT configuration from standard input to its end, sets
`ACTIONS_RUNNER_INPUT_JITCONFIG` in the environment of the `docker` client it
then `exec`s with its own arguments, and refuses an empty input or one over 64 KiB
with exit code 64, without printing the input. The Tart backend calls it as
`tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>`.

The runner image's Dockerfile installs `docker-ce-cli` at the version pinned here,
from the same repository, after the same key's fingerprint matches, and asserts that
its `docker` group has the id `DOCKER_GROUP_ID`; a unit test in `image::contract` holds the
Dockerfile and the smoke test to `golden::pins`: the `docker-ce-cli` pin, the key's
fingerprint and URL, the repository's URI and suite, and the group id. So the image's
client, key and group id cannot differ from the golden VM's `docker-ce-cli` pin, key and
group id.

`golden::steps::build_vm_docker_command` is the prefix the build uses to run `docker`
in the build VM, `sudo -n docker`, because the `docker` group applies only from the
guest agent's next start; see [The Golden VM Build](./build-images.md#the-golden-vm-build).
