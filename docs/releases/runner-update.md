---
type: Procedure
title: Update the GitHub Actions runner
description: Update the runner version, digest and image together and drain running jobs before rollout.
---

# Update the GitHub Actions runner

## Bumping the Runner

Dependabot's `docker` entry proposes a new runner base for
[`images/actions-runner/Dockerfile`](../../images/actions-runner/Dockerfile), and
its pull request stays red until five edits are made by hand on its branch, which
the tests in `cli/src/image/contract.rs` and `cli/src/image/runner.rs` ask for by
name:

1. Add the new version's entry to `RUNNER_RELEASES` in `cli/src/image/runner.rs`,
   with its release date from the
   [actions/runner releases](https://github.com/actions/runner/releases) page,
   never recalled.
2. In the same entry, record the SHA-256 of
   `actions-runner-osx-arm64-<version>.tar.gz`, copied from that release's notes,
   never recalled. It is the digest the macOS golden VM's build checks the
   downloaded archive against.
3. Set `DECLARED_RUNNER_VERSION` in the same file to the new version.
4. Set the `com.github.axross.runners.runner-version` and
   `com.github.axross.runners.runner-released` labels in the Dockerfile to the new
   version and that date.
5. Set the `FROM` line and the two labels of
   [`cli/tests/build-definition/Dockerfile`](../../cli/tests/build-definition/Dockerfile)
   to the same base and values.

Nothing else needs an edit for the version: the tests that depend on the declared
runner version and date read them from the two Dockerfiles.

Before the bump merges, check that the new runner still prints the two lines the
update drain reads, `Listening for Jobs` and `Running job:`, in the same form: search
for both strings in the new release's source on the
[actions/runner](https://github.com/actions/runner) repository, at its tag. A runner
that drops or rewords `Running job:` but keeps `Listening for Jobs` makes a drain
stop busy runners mid-job, as [CLI](../marshal-cli/agent-lifecycle.md#the-update-drain) describes; if either
line changed, change the line marks in `cli/src/runtime/job_marks.rs` in the same
pull request.

A runner bump changes no part of the run contract, so the contract version stays.
When the new runner changes something the run contract relies on, such as a path or
the user it runs as, [REVIEW.md](../../REVIEW.md) asks for the contract version to
be raised, as [CLI](../marshal-cli/runner-images.md#the-image-contract) describes. The `docker` group id
is one such thing: the Dockerfile fails the build when a new base gives the group
an id other than `DOCKER_GROUP_ID`, which the golden VM's `docker` group has, so a bump that
changes it also changes `DOCKER_GROUP_ID`, the Dockerfile and the smoke test
together, and raises the golden VM version and the contract version.

During temporary CLI recovery, runner bumps on main do not enter the fixed
historical program input. Dispatching a recovery release does not deliver those
bumps to a CLI host. The Temporary CLI Recovery section of
[Security](../security/release-authority.md#temporary-cli-recovery) governs that bounded route, and
[CLI Recovery](../runner-recovery/index.md) owns its operator procedure.
Keep the pull request's `build(deps)` title when merging: it releases a patch,
as [Version Rules](./versions.md#version-rules) describes. After app publication
is restored, the maintainer dispatches a release and each host updates the app
with the procedure in
[CLI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back).
