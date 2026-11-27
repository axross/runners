---
type: Procedure
title: "Size and maintain macOS guest pools"
description: "Configure the macOS base and disk, update an aging image and diagnose a VM slot waiting on the two-guest limit."
---

# Size and maintain macOS guest pools

## The Golden macOS VM and macOS Pools

A pool with `os: "macos"` runs each job in a macOS VM cloned from the golden macOS
VM, `marshal-golden-macos-g<golden VM version>-r<runner version>-<yyyyMMddHHmmss>`.
`build` makes it from the pinned Cirrus Labs base
`ghcr.io/cirruslabs/macos-tahoe-base`, after the Linux golden VM, whenever a macOS
pool is configured; [CLI](../marshal-cli/macos-pools.md#the-golden-macos-vm-build) gives its steps.
Build it again when the runtime reports it missing or mismatched, and when its
warnings ask for it.

- **At most two macOS VMs.** Apple's licence allows two macOS guests at once, so a
  macOS slot counts the macOS VMs before it clones one, and waits while two count:
  job and build VMs of macOS pools, a leftover job VM that `tart get` reports as
  macOS or whose OS it cannot read, and any other running VM that it reports as
  macOS or whose OS it cannot read, except the Linux golden and build VMs.
  The status shows the slot `waiting for the macOS VM limit`. The golden macOS
  VM counts only while someone runs it by hand, and a job VM of a configured Linux
  pool counts only when `tart get` reports it as macOS.
- **The host's macOS.** The Mac must run at least the base's macOS, 26.6.2 for the
  current pin; `build` refuses an older Mac and names both versions. Update the Mac,
  then build again.
- **The disk.** A macOS pool's `diskGb` is at least 50, the base's size. The base's
  layout is kept, recovery partition included, and the guest grows the disk to the
  pool's size at boot. Pulling the base takes about 26 GB of download, and the
  golden macOS VM and each job VM take up to their disk size.
- **Bumping the base.** When `status` warns that the pinned base is 30 or more days
  old, bump it: take the newest manifest digest of
  `ghcr.io/cirruslabs/macos-tahoe-base:latest`, its
  `org.cirruslabs.tart.upload-time` annotation's day and the macOS version it ships,
  and record all three in `MACOS_BASE` in `cli/src/image/macos_base.rs`, with the old
  digest added to `retired` so the next `build` removes it from Tart's cache. Raise
  `GOLDEN_MACOS_VM_VERSION`, review what Cirrus changed in the base, then rebuild.
  Rebuilding without a bump does not update macOS in the guest.
- **When macOS refuses another VM.** If the log says
  `macOS refused another macOS VM: …; if no other macOS VM is running, restart the Mac to release the place`,
  and `tart list` shows no other running macOS VM, restart the Mac: macOS can keep a
  VM's place taken after the VM has gone.
- **A VM that cannot be deleted** keeps counting, and the runtime deletes it again
  every 5 s, with its slot `waiting for VM <name> to be deleted`.
