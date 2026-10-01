# actions-runner image

The runner image that [`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop/README.md)
starts, one throwaway container per job. It is GitHub's own
`ghcr.io/actions/actions-runner` image plus the build dependencies jobs commonly
assume are already installed. It is built on the runner host and never pushed to
a registry.

## What it adds

- `build-essential`, `ccache`, `libyaml-0-2`, and `libgmp10`, installed without
  recommended packages. The package index is removed in the same layer, so a
  package a job needs at run time must be added here, not installed by the job.
- `/opt/hostedtoolcache`, set as `RUNNER_TOOL_CACHE`, and the `~/.gradle`,
  `~/.cargo`, `~/.rustup`, `~/.cache/ccache`, and `~/.npm` directories, all
  owned by `runner`. They are the mount points the host configuration's cache
  volumes usually target, and being pre-created keeps a fresh volume writable.

It ends as `USER runner` and sets no entrypoint. The host supplies the command
(`/home/runner/run.sh`) and the registration.

## The base pin

The `FROM` line names the runner version and the digest of that version's
multi-architecture index, so a rebuild cannot silently change the base. To
refresh the digest by hand, request the index for the tag from the registry and
take the `docker-content-digest` header:

```bash
token=$(curl -sS "https://ghcr.io/token?scope=repository:actions/actions-runner:pull" | jq -r .token)
curl -sSI -H "Authorization: Bearer $token" \
  -H "Accept: application/vnd.oci.image.index.v1+json" \
  https://ghcr.io/v2/actions/actions-runner/manifests/<version> | grep -i docker-content-digest
```

Dependabot's `docker` entry proposes the same change as a pull request. Merge it
promptly; [Windows Runner Host](../../docs/operations/windows-runner-host.md#updating-the-runner)
explains why.

## Build

From the repository root:

```bash
docker build images/actions-runner
```

CI runs this build on every pull request without pushing the result, then runs
the image with no network to check that it runs as `runner`, has an executable
`/home/runner/run.sh` and a writable tool cache, and carries `ccache`.
