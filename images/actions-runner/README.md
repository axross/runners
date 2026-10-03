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
- `/opt/hostedtoolcache`, set as `RUNNER_TOOL_CACHE`, and the `~/.cargo`,
  `~/.rustup`, and `~/.npm` directories, all owned by `runner`. They are the
  mount points the host configuration's cache volumes usually target, and being
  pre-created keeps a fresh volume writable.
- On Linux x64, Android NDK `27.0.12077973` (r27) and `27.1.12297006` (r27b)
  under `/home/runner/.android/sdk/ndk`, owned by `runner`. Both
  `ANDROID_HOME` and `ANDROID_SDK_ROOT` select that SDK root. Other base-image
  platforms keep an empty SDK, not an incompatible x64 toolchain.
- Writable `~/.android/cache` for SDK download metadata.

It ends as `USER runner` and sets no entrypoint. The host supplies the command
(`/home/runner/run.sh`) and the registration.

## Android NDK contract

[`install-ndks.sh`](./install-ndks.sh) pins each numeric version, Google's
letter release, and the Linux archive's SHA-256. It verifies the archive before
extraction and checks its `source.properties` revision before adding matching
side-by-side `package.xml` registration. Archives and staging files stay out of
the final image. The SDK is writable inside each disposable container so setup
actions can add command-line tools, licenses, and packages. The Cache Volumes
section of [Windows Runner Host](../../docs/operations/windows-runner-host.md)
owns the Android storage exclusions.

Java and Android command-line tools are not preinstalled. An Android setup
action MUST retain the incoming `ANDROID_SDK_ROOT` and export both SDK variables
to it; setting only `ANDROID_HOME` does not guarantee reuse. There is no global
`ANDROID_NDK_HOME`: consumers select their own exact required release and use
ordinary SDK-manager installation, just as on a standard Linux x64 runner:

```bash
sdkmanager --sdk_root="$ANDROID_SDK_ROOT" --install "ndk;$required_version"
```

SDK-manager recognizes the image's registered packages without downloading
their NDK archives again, and acquires a missing version normally until the
image catches up. The image owns archive verification, package registration,
SDK environment, ownership, and compiler health tests. Consumers need no image
flag, directory/revision/Clang branch, fixed SDK path, or SDK-manager wrapper.
This still invokes SDK-manager and may fetch repository metadata; it does not
eliminate all SDK/network work. Do not restore an NDK cache over a preinstalled
directory or replace an application's dependency-selected version with another
installed release.

To add or replace a release, obtain its official Linux archive from
[Google's NDK downloads](https://developer.android.com/ndk/downloads), check
Google's published archive checksum, compute SHA-256, and update the installer
row, both test versions, the version list under What it adds, and the archive
size figures under Build together. Run the checks below and repeat a real
SDK-manager inventory/install request for each release: a successful Clang
compile alone does not prove package discovery. Confirm consumers' exact
requirements before removing an older release; two installed versions are not
a request to unify consumer dependencies.

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
bash images/actions-runner/tests/check-download.sh
docker build --tag actions-runner:test images/actions-runner
bash images/actions-runner/tests/smoke-test.sh actions-runner:test
```

CI runs these Linux x64 checks on every pull request without pushing the
result. The smoke test uses no network or mounts, checks the existing runner
and cache properties, compiles and links an Android arm64 C++ shared library
with each NDK, and verifies its ELF architecture and exported symbol. It also
rejects image-declared volumes that would introduce anonymous mounts. A
mutation in one disposable container must be absent in a second fresh
container. The download test rejects corrupt input before extraction and
checks that non-x64 installation is skipped.

Windows Docker Desktop verification remains the maintainer's post-merge
responsibility. The [historical Linux SDK interoperability evidence](https://github.com/axross/runners/issues/13#issuecomment-5963049851)
includes ordinary requests for both installed packages with networking disabled;
it is not a Windows or full-application result. Repeat the build/smoke commands
above on the intended desktop and record the source revision and image identity
before validating a consumer. Use a separate candidate tag, not the host's
production tag, and do not mount a shared SDK/home or runner socket. Image
rollout remains a separate operator decision.

The host's uncached weekly rebuild downloads both archives again: together
1,327,934,693 bytes (about 1.33 GB), containing 4,107,528,665 bytes of extracted
file content before filesystem/layer overhead. This is a download/content
cost, not a measured final image delta. The operator rebuild and rollback
procedure is in the Building the Runner Image section of
[Windows Runner Host](../../docs/operations/windows-runner-host.md).

The [initial image size/build-cost evidence](https://github.com/axross/runners/issues/13#issuecomment-5963050240)
records the exact source/image identities, raw layer bytes, uncached timings,
and environment limits. Those historical measurements are not a size promise
for a later base/version update or a runner-host speed prediction. Build caches
can retain the installation stage independently of the host's dangling-image
prune.
