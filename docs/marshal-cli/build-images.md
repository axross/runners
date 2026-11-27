---
type: Specification
title: "Building the host image or golden VM"
description: "What does a build replace, and what happens to the previous image on failure?"
---

# Building the host image or golden VM

## Build

The current library exposes the foreground `build` entry point through the
[test-only commands helper](./entry-points.md#running-it). The lock, control
channel and exit codes below describe that foreground process. In the app,
the [resident agent](./resident-agent.md#the-agent) keeps its lock and accepts a
build request while runners are stopped. Windows operators can follow the
[Build image menu procedure](../runner-hosts/windows-image.md#building-the-runner-image).
For the historical CLI executable, use the [recovery instructions to find the guide for the installed tag](../runner-recovery/install.md).

`build` builds the image the configuration names, from the definition embedded in
the binary, and reads nothing from a checkout or the working directory. It does
these in order:

1. It takes the instance lock. When the lock is held it exits 1 and changes
   nothing, naming the instance's state when this user's channel answers within
   5 s, and otherwise giving `start`'s message for a lock held elsewhere.
2. It validates the configuration as `validate` does; an invalid one exits 1.
   `--profile axmc` then builds the golden VM instead, as
   [The Golden VM Build](#the-golden-vm-build) describes. An image name given by
   digest is refused, because it cannot be tagged. It prints the name it builds.
3. It contains its children as `start` does on Windows, and opens the control
   channel. While it builds, `status` answers `state: building`, `image: <name>`,
   and `elapsed: <s> s`; a `start`, a resume, `restart`, and `diagnostics` are refused, naming
   the build; `stop` aborts it.
4. Docker must answer `docker info` within 30 s, or `build` exits 1 before any
   build. A `docker info` that fails ends it at once, and the message gives the
   client's output, or why the client could not start. The foreground build returns this failure
   immediately. During startup, `start` waits for Docker.
5. It removes the temporary tags of the image's repository that carry the image
   label, which an earlier `build` that was killed left. A failed removal is a
   warning.
6. It runs one
   `docker build --pull --no-cache --label com.github.axross.runners.image=actions-runner --label com.github.axross.runners.built=<s> --tag <temporary> -`,
   with the context on standard input and the client's output on the terminal.
   The temporary tag is `<repository>:marshal-build-<yyyyMMddHHmmssfff>` in UTC,
   where the repository is the name without its tag: everything before the last
   `:` that follows the last `/`, so a registry port stays.
7. It tags the build as the configured name, which commits it. It then removes the
   temporary tag, and prunes the dangling images that carry the image label. A failed tag removal is a warning naming the
   tag, which the next `build` removes, and a failed prune is a warning; both still
   exit 0.
8. A failed build or a failed retag removes the temporary tag, leaves the
   configured name as it was, and exits 1. A retag that stalls for 120 s with no
   stop asked is ended, and a `docker image inspect` of the configured name, as
   below, decides whether it committed; one that committed still prunes.

The context is a ustar archive of the embedded files at their paths in
`images/actions-runner/`, sorted by path, with mode `0644`, owner and group 0, and
time 0, so the same binary always sends the same bytes. A debug build builds the
directory named by `MARSHAL_TEST_BUILD_DEFINITION` instead, for the real-Docker
test; a release build ignores it, and `tests/build_definition.rs` shows both
halves.

`stop`, SIGINT, SIGTERM, and SIGHUP on Unix, and Ctrl+C and Ctrl+Break on Windows
abort a build within 30 s of the request. The build client is killed at once; the
CLI has no graceful way to end it.

- **Before the retag.** The temporary tag is removed, by 26 s, and `build` exits 1.
  The configured name keeps its previous image. A failed removal names the tag.
- **During the retag.** The `docker tag` client gets up to 5 s from the stop. When
  it succeeds within them, the build has committed, and `build` exits 0. Otherwise
  a `docker image inspect` of the configured name, within 4 s, decides. This
  build's build-time label means the build committed, and `build` exits 0; another
  means it did not, and `build` exits 1. A failed or late inspect is "commit
  unknown", which names the image, and `build` exits 1.
- **After the retag.** The build has committed: the tag removal finishes, the prune
  is skipped for the next `build`, and `build` exits 0.
- At 28 s whatever still runs is dropped, and `build` exits 1.

An orphaned build client can still apply the temporary tag after `build` removed
it, on Unix always and on Windows until the Job Object closes, and a hard kill or a
console close can leave one too. The next `build` removes it.

| Exit | Meaning                                                                              |
| ---- | ------------------------------------------------------------------------------------ |
| 0    | The configured name holds the new image                                              |
| 1    | A refused, invalid, or failed `build`, Docker not answering, or an uncommitted abort |
| 2    | A usage error                                                                        |

On axmc, exit 0 means the golden VM holds the new image. Exit 1 is a refused, failed or
stopped build. It deletes its build VM and keeps the previous golden VM, with two
exceptions: at the exit deadline the build VM is left to its shim or the next build, and
when the commit is unknown the rename may have made a new golden VM. Stop and failure in
[The Golden VM Build](#the-golden-vm-build) describes both.

### The Golden VM Build

In the [foreground test helper](./entry-points.md#running-it), `build --profile axmc` makes the golden VM that every job VM is cloned from
(`build::golden`). It takes the lock, validates, contains and opens the control channel
as above, and answers `status` and `stop` the same way, but it calls `tart`, not
`docker`. A debug build runs the `tart` that `MARSHAL_TEST_TART` names, and a release
build always runs `tart` from `PATH`, as the [Tart Backend](./tart-backend.md#tart-backend) does. It
does these in order:

1. **Prerequisites.** It reads Tart's version, then finds Softnet on the `PATH` the Tart
   client carries and judges its file and version, as
   [Host Prerequisites](./tart-backend.md#host-prerequisites) says. A refusal, a missing `tart` or
   `softnet` included, exits 1 before any VM exists, with a message that names the
   host document's section ([Installing Tart and Softnet](../runner-hosts/mac-setup.md#installing-tart-and-softnet)
   or [Setting Up Softnet](../runner-hosts/mac-setup.md#setting-up-softnet)). A warning, such
   as a newer patch version, is logged and the build runs. `build` does not wait for
   Tart, as it does not wait for Docker.
2. **Leftovers.** Every local VM whose name is `marshal-build-linux-<yyyyMMddHHmmssfff>`
   is stopped forcibly and deleted, within 15 s each. These are the build VMs of
   a CLI that died: its [VM shim](./tart-backend.md#the-vm-shim) deletes its VM when the CLI is gone,
   and this deletes any that the shim could not. A VM that cannot be deleted is a
   warning.
3. **Pull and clone.** `tart pull` of the base image by its pinned digest
   (`golden::pins::BASE_REFERENCE`), within 30 min, and `tart clone` of it to a new
   build VM, `marshal-build-linux-<yyyyMMddHHmmssfff>`. The name is recorded before
   the clone runs, so a clone that fails halfway is deleted. The build VM keeps the
   base's CPUs, memory and 20 GB disk: the build does not run `tart set`, because no
   job needs more than the image build takes, and every job VM is sized from the pool
   when it is cloned.
4. **Boot.** `tart run` under the VM shim (this program run again, as for a job VM),
   then the wait for the guest agent, every second with each question bounded at 5 s,
   until it answers, `tart run` ends, or 120 s pass. A boot that fails has the job VM
   boot failure's text, naming the build VM, with the same hints. The wait is
   `tart::boot`, which the Tart backend's job VM boot shares.
5. **Provisioning.** The six guest calls of [Golden VM Provisioning](./tart-backend.md#golden-vm-provisioning),
   in order, each through `tart exec` with its own budget. Their output is read as data,
   cut at 64 KiB, and appears only in the message of a call that failed, on one line. A
   call that exits non-zero fails the build. The listing from the third call goes to
   `golden::key`; a mismatch, a second key, or an empty or oversize listing fails the
   build before the fourth call, which installs from Docker's repository.
6. **The image.** It waits up to 60 s for the Docker Engine in the VM (`docker info`
   every 2 s, each bounded at 10 s), then builds the runner image from the definition
   embedded in the binary, with the context on the standard input of
   `tart exec -i`, and follows the axpc flow as far as a new VM needs: the one
   `docker build` with exactly axpc's arguments under the temporary tag
   `<repository>:marshal-build-<yyyyMMddHHmmssfff>` in UTC, a `docker image inspect` of
   the labels that requires this CLI's image contract, a `docker tag` to the configured
   `imageName`, and the removal of the temporary tag (a failure of the removal is a
   warning). It does not remove earlier temporary tags or prune dangling images,
   because the VM is new. The build's output is forwarded to the log line by line,
   masked and cut as a container's output is.
7. **Commit.** `sync`, then `sync -f /`, as root in the build VM, within 2 min, then `tart stop` of the
   build VM with a 60 s wait, within 90 s, then the wait for `tart run` to end, then
   `tart rename` to the golden name,
   `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`
   with the build time: the time taken just before the in-VM `docker build`, the same
   value as the temporary tag's and the `built` label's, not the start of the build.
   **The rename is the commit.** `tart stop` ends the VM at once, with no shutdown
   inside the guest (Tart's `stop` interrupts `tart run`, which stops the virtual
   machine forcibly), so the `sync` is what puts the files the image build wrote last
   on the golden VM's disk.
8. **After the commit.** It deletes every other golden VM, whatever its versions, and
   removes from Tart's cache each image named in `golden::pins::RETIRED_BASE_PINS`, with
   `tart delete`; the current pin is never deleted, and the list never holds it. A
   failure of any of these is a warning that the next `build` tries again, and
   `build` still exits 0, because the newest golden VM is the one used.

`imageName` is optional on axmc. When it is absent, `build` names the image
`actions-runner:local` (`runtime::pool::DEFAULT_IMAGE_NAME`), and `validate` and its
summary do not change.

**The Docker group.** The fourth guest call creates the `docker` group with the id
`DOCKER_GROUP_ID`, so the socket the runner container mounts is owned by the group that
the runner image's `runner` user is in, and adds `admin` to it. A `docker` group that exists with
another id fails the build before any Docker package is installed. The group change reaches
`tart exec` commands only when the guest agent next starts, and the build VM is not
restarted. The image build therefore runs `sudo -n docker`
(`golden::steps::build_vm_docker_command`), where it could instead have rebooted the VM,
which would add a boot and a second wait for the agent. Job VMs, which boot from the
golden VM, run `docker` as `admin` without `sudo`.

**Stop and failure.** Any failure, and `stop`, SIGINT, SIGTERM and SIGHUP at any step
before the rename, drop the call in flight, which kills its `tart` client, and delete
the build VM: a graceful stop and then a forced stop if that fails, then `tart delete`,
within 15 s as one `teardown` budget (7.5 s, 3 s, 3 s and 1.5 s for a listing). The previous golden VM is not touched.
The exit deadline of 28 s from the stop applies as it does on axpc, and at it `build`
exits 1, naming that the VM shim or the next build deletes the build VM. The rename has
the same grace as axpc's retag: when a stop arrives during it, `tart rename` gets up to
5 s from the stop, its client is then ended, and a listing, within 4 s, says whether
the golden name exists. If it does, the build has committed and `build` exits 0; if it does not, the build VM is
deleted and `build` exits 1; if the listing fails, the exit is 1 with "commit unknown"
and the golden name to look for. The build VM is still deleted by its own name, which
removes it if the rename did not happen. If it did, there is no such VM to delete, and
with the listing failing too the deletion cannot be confirmed, so `build` warns that the
next build deletes it if it is still there. The 5 s grace, the 4 s listing and the 15 s
clean-up fit inside the 28 s deadline, and a unit test holds the constants to that. After
the rename, a stop skips the rest of the clean-up, and `build` exits 0.

**A CLI that dies.** A build VM is held by a [VM shim](./tart-backend.md#the-vm-shim), so a CLI that is
killed, or crashes, leaves its build VM to the shim, which stops and deletes it within
15 s. A shim that is killed too leaves it to the next `build`, which deletes every build
VM by name form.

**Keep-awake.** `build` holds no keep-awake and ignores the sleep and network signals,
which only a running instance reacts to. A Mac that sleeps during a build stops the VM's
work, and the build fails or is stopped; the host document tells the operator to keep
the Mac awake.

**Windows.** `build --profile axmc` on Windows exits 1 before the lock, the
configuration and any engine, with `error: the axmc profile runs only on macOS`. The
code compiles on Windows and holds the VM with `VmLauncher::Direct` there, which no run
reaches.
