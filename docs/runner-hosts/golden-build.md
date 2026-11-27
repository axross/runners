---
type: Procedure
title: "Build the golden Linux VM"
description: "Check capacity, session and lock ownership before building, and decide which VM survives a failure or interruption."
---

# Build the golden Linux VM

## Building the Golden VM

`build` on the axmc profile builds the golden VM and replaces the previous one only
when the new one is complete. It is needed after installing Tart and Softnet, after
any change of the library that raises the golden VM version or the image contract
version, when the runtime reports a missing or mismatched golden VM, and when the
freshness warnings ask for it. The historical CLI runs it as
[`marshal build --profile axmc --config <path>`](https://github.com/axross/runners/blob/dc8e1ebcc0722997206487f3dcca2ae7e519cf02/docs/operations/macos-runner-host.md#building-the-golden-vm).

**Before you start**

- No instance and no other `build` may run: the build takes the same instance lock
  as `start`, and refuses saying so otherwise.
- Tart and Softnet must be installed and set up as
  [Installing Tart and Softnet](./mac-setup.md#installing-tart-and-softnet) and
  [Setting Up Softnet](./mac-setup.md#setting-up-softnet) say. The build checks both before it
  creates any VM, and exits 1 naming the section when one is missing or wrong.
- Sign in at the Mac's screen and run the build in that session, as
  [The Signed-In Session](./mac-setup.md#the-signed-in-session) says: the build boots a VM too.
- **Keep the Mac awake for the whole build.** `build` holds no keep-awake and
  ignores the sleep and network signals that stop a running instance. A Mac that goes
  to sleep during the build stops the VM's work, and the build then fails or has to
  be stopped. Turn off the Mac's sleep for the time being, or run `caffeinate -i` in
  another terminal until the build ends.
- Leave room on the disk. The pull caches the 20 GB base image, the build VM is a
  clone of it that grows while the runner image is built, and the finished golden VM
  stays. This is the 40 GB that `validate` reserves beyond the pools' own disks. The
  build VM keeps the base's 20 GB disk, so the runner image and Docker's layers have
  to fit in it; a build that runs out of disk fails at the image build, with Docker's
  output in its log.

**What it does**

1. Checks the prerequisites above.
2. Deletes any build VM an earlier build left behind, `marshal-build-linux-<time>`.
3. Pulls the base image by its digest and clones it to a new build VM, which keeps
   the base's CPUs and memory.
4. Boots the build VM on Softnet and waits up to 120 s for its guest agent.
5. Upgrades the Ubuntu packages, checks Docker's signing key against its recorded
   fingerprint, creates the `docker` group with the id in `golden::pins::DOCKER_GROUP_ID`,
   installs Docker Engine at the pinned versions, installs the launcher, and disables the
   SSH login. A key whose fingerprint is not the recorded one fails the build before
   anything is installed from Docker.
6. Builds the runner image inside the VM, from the definition embedded in the library,
   with the arguments `build` gives it on axpc, under the configured `imageName`, and
   checks its image contract label. The image build runs `sudo -n docker`, because
   the `docker` group reaches guest-agent commands only after the agent restarts.
   This needs the VM to reach the network: Docker's repository, the Ubuntu archive,
   and the registries the Dockerfile pulls from, on Softnet's rules, which allow
   globally routable addresses.
7. Writes the VM's files to its disk with `sync`, stops it, and renames it to the
   golden name,
   `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<time>`.
   This rename is the moment the new golden VM takes over.
8. Deletes every other golden VM and the retired base images from Tart's cache. A
   failure here is only a warning; the next build tries again, and the newest golden
   VM is the one that is used.

`build` writes its steps to its standard error, and the image build's own output
line by line, masked as a job's output is.

**If it fails or is interrupted.** A stop, by a signal or a client's `stop`, and a
failure at any step before the rename delete the build VM within the 30 s stop
sequence and leave the previous golden VM as it was, so `start` is not affected.
After the rename the new golden VM stays, and a stop only skips the clean-up in step 8. If the build's process is killed or crashes mid-build, a helper process stops and deletes the
build VM within 15 s; if that fails too, the next `build` deletes it by its
name. `tart list` shows whatever is left: `marshal-build-linux-<time>` is a build VM
that is safe to delete with `tart delete`, and `marshal-golden-linux-...` is a golden
VM.

**Which host.** `build` on the axmc profile is only for macOS; on Windows it refuses,
saying that the axmc profile runs only on macOS.
