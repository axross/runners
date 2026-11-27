---
type: Procedure
title: "Build and update the Windows host’s runner image"
description: "Build an image from the installed release, distinguish an OS refresh from a runner upgrade, and handle freshness warnings."
---

# Build and update the Windows host’s runner image

## Building the Runner Image

Build the image once at install, after every update of Marshal, and at each routine
rebuild. Building is refused while runners run, so from Marshal's menu choose
**Stop all runners**, then **Build image**, and when the build has finished,
**Start all runners**. A build needs a configuration in use; it uses that
configuration's image name and reads no file.

The build builds the image definition embedded in the installed
Marshal, under the configured global `imageName`, with
`docker build --pull --no-cache`, and reads nothing from a checkout or the working
directory. The runner version and base image digest come from that definition, so
a newer runner needs a newer release; see [Updating the Runner](#updating-the-runner).
The cache is ignored so that the operating system packages are installed again and
pick up their updates. The build goes to a temporary tag, and only a successful
build is tagged as the configured name; a failed build leaves the previous image in
place. After a successful build `build` removes the earlier images it built that the new
build left untagged: the prune is limited to images carrying the label `com.github.axross.runners.image=actions-runner`,
so it does not touch other dangling images or images built by hand. It does not
reclaim installation stages, build caches, or tagged rollback images. A failed prune
is a warning, not a failed build. [CLI](../marshal-cli/build-images.md#build) gives every step, and
its section The Agent's Configuration gives what the agent's build
logs.

While it builds, the window says the image is building, and **Stop the Build** or
**Stop all runners** aborts it within 30 s; an abort before the retag leaves the
configured name on its previous image. CI runs the library's build only against a
small test definition on Linux; a full build of the image on Docker Desktop was
verified on axpc with the command line's `build` (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)).

The image carries a Docker client at a pinned version, which `build` installs from
Docker's apt repository after checking its signing key. The Docker runtime mounts no
Docker socket into a job container, so the client reaches no engine here; the image
has it for the axmc host, whose job VMs mount their own. The image contract version
is 2 with it, so an image built before the release that carries contract 2 is refused
until the image is built once; [CLI](../marshal-cli/runner-images.md#the-image-contract) has the check.

The Android NDK contract, Java/SDK/Gradle inputs, and Build sections of the
[image README](../../images/actions-runner/README.md) own version/checksum
maintenance, the consumer fallback contract, and additional image-build costs.

For an authorized image update, rollback, or fixed-image comparison:

1. Pause the routine rebuild, and do not rebuild by hand during the window.
2. Record the stable current image ID with
   `docker image inspect <configured-image-name> --format '{{.Id}}'`. Before an
   update, retain it with `docker tag <saved-image-id> actions-runner:rollback`;
   this tag keeps it out of the dangling-image prune.
3. Build the approved new image with the release that embeds it, or roll
   back by tagging the saved image ID as the configured image name again. Record
   the image ID actually used. Existing containers finish on their original image;
   subsequent containers use the retagged or rebuilt one. The image check before
   each job accepts the retagged image only when its contract label matches the
   installed Marshal; see [CLI](../marshal-cli/runner-images.md#the-image-check).
4. A retag alone lasts only until the next image build, which builds the
   definition the installed release embeds. Install the release intended for
   subsequent builds before resuming the routine rebuild.
5. Remove the rollback tag with `docker image rm actions-runner:rollback` only
   after the update is accepted and that recovery image is no longer needed;
   this is not a general image/cache prune.

For build-cache reclamation, inventory `docker system df -v` and identify the
builder holding the installation stages first. Obtain separate approval for
its unused-cache operation. For the daemon builder, an age-filtered option is
`docker builder prune --all --filter until=168h`; it is not image-label-scoped
and can invalidate other projects' build caches. This command's syntax is
checked locally, but its real-host behavior and any automatic garbage-collection
limit are unverified; do not assume the image prune bounds that storage.

No registration or cache-volume change is needed. Rollout and rollback are
separately authorized operator actions, not effects of an image-source pull
request.

## Rebuilding the Image for Operating-System Updates

No task rebuilds the image, so the operating-system packages in it are only as
fresh as its last build. Rebuild it weekly, and also whenever the window or the
agent's log shows a build-age warning: from 7 days that the image should be rebuilt,
and from 21 days that its operating-system packages are that old. An image that
Marshal did not build, such as one built by CI or by hand, has an unknown build age
and a warning that says so. At a quiet time, choose **Stop all runners**, then
**Build image**, then **Start all runners**.

Building is refused while runners run, so a rebuild stops the pools for the length
of the build: a job in flight fails, and queued jobs wait. A failed build leaves the
previous image, and the runners run on it.

The runner-release-age warning,
`runner <v> was released <d> days ago, ...`, is different: a rebuild does not clear
it. The runner version comes from the image definition embedded in the installed
Marshal, so a rebuild with the same release builds the same runner. That warning
requires [updating the runner](#updating-the-runner).

## Updating the Runner

This app-update procedure applies when app publication resumes. During F0 recovery,
the fixed historical CLI does not incorporate runner bumps on main; use
[CLI Recovery](../runner-recovery/index.md) for the bounded release route, not an app update.

A newer runner reaches the host only through a new release, because the app embeds
the image definition:

1. Merge the Dependabot pull request that bumps the `FROM` line in
   `images/actions-runner/Dockerfile`, with the edits it needs by hand; see
   [Development Workflow](../releases/runner-update.md#bumping-the-runner). Keep its
   `build(deps)` title when merging: it releases a patch, as
   [Version Rules](../releases/versions.md#version-rules) describes.
2. The maintainer dispatches `release.yaml` on main, as
   [Each Release](../releases/publish.md#each-release) describes.
3. Quit Marshal from its menu, which stops its runners, then install that release
   with the procedure in [CLI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back). A
   host that still runs the command line moves to the app first, as
   [Installing Marshal](./windows-setup.md#installing-marshal) describes.
4. Start Marshal, build the image from its menu, then start the runners. Containers
   already running when Marshal stopped them have ended; the next ones start on the
   new image.

Merge runner bumps promptly, then release, install and build the image soon after.
The runner in the image never updates itself: Marshal registers every runner through
GitHub's JIT configuration, which disables automatic updates, so the runner stays at
the version the image was built with. GitHub stops assigning jobs to a runner that is
not updated within 63 days of a newer release and, when a critical security update
is required, assigns none until the runner is updated; see
[GitHub's self-hosted runner reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners).
