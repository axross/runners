---
type: Specification
title: "Linux job VM contract"
description: "Identify the golden Linux VM, the network and Docker socket visible to a job, and which component deletes each VM."
---

# Linux job VM contract

## The Golden VM Contract

Each Linux job VM is cloned from the golden Ubuntu VM. [macOS guest pools](./macos-vm.md#the-golden-macos-vm-and-macos-pools) use a separate base. This section is the
contract between `build`, which makes it, and a runtime that clones it. The code
defines it, in `runtime::tart`, `golden` and `build::golden`.

- **Names.**
  - A job VM is `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`, in UTC, with the
    slot counted from 1.
  - A golden VM is
    `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`.
  - A build VM is `marshal-build-linux-<yyyyMMddHHmmssfff>`.
  - Only `build` creates golden and build VMs and deletes golden VMs, and only `build`
    and a build VM's own shim delete a build VM. Cleanup at `start` matches only the
    job VMs of configured pools by this exact form, so it never matches a golden VM,
    a build VM, a VM of a pool that is no longer configured, or a VM the runtime does
    not own.
- **The golden VM version** is a number in the CLI (`runtime::tart::names`),
  starting at 1. It goes up whenever the base digest, the provisioning steps, the
  launcher or the Docker Engine versions change. It is 2 with the `docker` group's
  fixed id, and the golden VM's image contract version is 2 with the runner image's
  Docker client, so a golden VM built before is refused as mismatched until
  the golden VM is built again.
- **Reading it.** The golden VM's name records its golden VM version, its image
  contract version, its runner version and its build time, so `start`, `restart`,
  each job launch and `status` read them from `tart list` without booting it. The
  newest golden VM by build time is the one used. It must carry the CLI's golden VM
  version and image contract version, or the runtime refuses it as a mismatched image
  is refused, naming the golden VM; a runner version that differs is not a mismatch.
  A missing golden VM is refused the same way, and the build time drives the
  freshness warnings: a golden VM 7 to 20 days old, or 21 days or more, asks for a
  rebuild.
- **The base image** is `ghcr.io/cirruslabs/ubuntu:24.04`, pinned by the digest of its
  manifest (`golden::pins::BASE_REFERENCE`): arm64 Ubuntu 24.04 with a 20 GB disk and
  the guest agent, and no Docker. The golden VM keeps that disk size, so a pool's
  `diskGb` is at least 20. `build` pulls it by digest, so a tag that moves changes
  nothing, and removes from Tart's cache only the digests of the retired base pins
  that the CLI lists. A change of the pin raises the golden VM version and adds the
  old pin to that list.
- **What the build installs.**
  - The Ubuntu packages are upgraded.
  - Docker Engine (`docker-ce`, `docker-ce-cli`, `containerd.io` and
    `docker-buildx-plugin`) comes from Docker's apt repository at pinned package
    versions, which apt holds, after the repository's signing key has matched a
    recorded fingerprint. Docker Compose is not installed. The `docker` group is
    created after the key check and before any Docker package is installed, with the id
    in `golden::pins::DOCKER_GROUP_ID`, and the build fails if the group exists with
    another id.
  - The launcher is `/usr/local/bin/marshal-launch`, owned by root, mode 0755.
  - The `admin` password is locked and SSH is masked, so the base image's
    `admin`/`admin` login cannot be used. The guest agent does not use SSH.
  - The runner image is built inside the VM under the configured `imageName`, which
    is `actions-runner:local` when the configuration gives none, and carries this
    CLI's image contract.
- **The Docker socket.** The Docker Engine in a job VM listens on
  `/var/run/docker.sock`, owned by the `docker` group. The job's runner container
  mounts that socket, so a job step can run `docker` against the VM's own engine. The
  socket belongs to the job's own VM, which is deleted when the job ends;
  [Security](../security/vm-isolation.md#the-vm-local-docker-socket) records why that is
  permitted, what it does not extend to, and why the runner needs no `--group-add`.
  The image's Docker client is installed from the same repository, at the same
  version.
- **The guest user.** The guest agent runs commands as `admin`, the golden VM's
  user, which the golden VM puts in the `docker` group, so `docker` needs no `sudo`
  in a job VM. The source for this is the unit file
  `packaging/tart-guest-agent.service` (`User=admin`) of
  [cirruslabs/tart-guest-agent](https://github.com/cirruslabs/tart-guest-agent) at
  commit `6d4bd9c`.
- **The launcher's input.** `tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>`
  runs the launcher with the arguments of a `docker run`, and with the JIT
  configuration on standard input only. The launcher reads it, up to 64 KiB, into
  the environment of the `docker` client it then starts, so it appears in no argument
  list. It refuses an empty input or a longer one with exit code 64.
- **A slot's cycle.** The [Tart Backend](../marshal-cli/tart-backend.md#tart-backend) in the CLI document
  has the budgets and texts. For each job, a slot:
  1. checks the golden VM and the host prerequisites, and waits until the Mac has a
     primary network;
  2. clones the newest golden VM that runs here to a job VM, with `tart clone`, and
     sizes it with `tart set --cpu <cpus> --memory <MB> --disk-size <GB>` from the
     pool's `cpus`, `memoryGb` and `diskGb`;
  3. boots it with `tart run`, held by a shim, with these options and no other
     network option, no shared directory and no Rosetta:
     `--no-graphics --no-clipboard --no-audio --no-usb-accessories --net-softnet --net-softnet-allow "out proto udp @host port 53,out proto tcp @host port 53" --net-softnet-block "out @host,224.0.0.0/4"`.
     A rule with a port outranks one without, so DNS to the gateway passes while
     every other packet to the gateway and all multicast do not;
  4. waits up to 120 s for the guest agent, then up to 60 s for the Docker Engine,
     and runs the CPU probe; a failed probe ends the cycle before a runner registers;
  5. registers a JIT runner, and runs the job container with `--pull never`, with
     the container's CPU quota and affinity covering all the VM's vCPUs and its
     memory limit equal to the VM's, and with the VM's Docker socket mounted;
  6. when the job ends, exports diagnostics for a diagnostics-enabled pool, stops the
     VM and deletes it, so no job VM outlives its job.
- **Who deletes what.** A job VM is deleted by its slot, on every path out of the
  cycle; by the stop sequence; by its shim when the CLI dies, within 15 s; and by the
  next `start`, which deletes a configured pool's leftovers without exporting their
  diagnostic evidence and reports the loss in the log. A build makes its VM under the
  build name, renames it to the golden name only when it is complete, and deletes
  every other golden VM only after that. A failed or interrupted build leaves the
  previous golden VM in place.
