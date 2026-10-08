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
- On Linux x64, the [Java, SDK, and Gradle inputs](#java-sdk-and-gradle-inputs)
  below. These inputs are owned by `runner` and
  writable inside each disposable container.
- Writable `~/.android/cache` for SDK download metadata.

It ends as `USER runner` and sets no entrypoint. The host supplies the command
(`/home/runner/run.sh`) and the registration.

## Opt-in diagnostics

The host's default command and automatic removal are unchanged. For an opted-in
entry, `/usr/local/bin/runner-diagnostics` supervises `run.sh`, forwards stop
signals, and reaps a separate observer. It never wraps a build tool or changes
JVM flags. The binary is compiled with the existing `build-essential` toolchain;
no additional package or version is installed. A missing binary falls back to
the normal runner with a diagnostic-gap warning.

The observer writes container-local `/tmp/runner-diagnostics/metrics.txt` every
ten seconds, for at most 24 hours and 8 MiB. It records cgroup v1/v2 memory,
peak, limit, OOM and swap counters, CPU quota and throttling, the process's
allowed CPUs, and at most 128 recognized JVM/compiler/linker/native-build
PID/PPID/RSS records. Unavailable fields are explicit. Counters carry separate
absolute and baseline-delta fields; a missing initial counter has no delta.
Start and end samples are labelled separately and do not attach to JVMs.
The launcher waits at most 500 ms for a saved start snapshot before starting
the runner. A startup timeout stops the late observer, reports a gap, and
starts the ordinary runner without manufacturing post-start deltas. Shutdown
signals are masked across the runner fork and handler reset, then forwarded
through a nonblocking wait loop so the check-to-wait window cannot swallow them.
These counters do not identify which child died. Attach uses only the baked
JDK's `jcmd VM.flags`, for up to four JVMs per sample and 500 ms each, retaining
only numeric heap/metaspace/processor flags and boolean `UseContainerSupport`.
No arguments, environment, arbitrary process names, or unfiltered tool output
are retained. Missing tools, attach failure and unsupported cgroup layouts are
gaps, not build failures. Each directory scan considers at most 1,024 entries.

The second, raw-record opt-in retains only regular, non-symlink reports from:

- `/home/runner/.gradle/daemon/<version>/daemon-<pid>.out.log`.
- `/home/runner/_work/<repository>/<repository>/hs_err_pid<pid>.log`.
- `/home/runner/hs_err_pid<pid>.log` and `/tmp/hs_err_pid<pid>.log`.

Collection has a three-second scan/copy deadline, a combined 32 MiB budget,
and at most 128 generated `raw-<index>.log` files. It omits oversized or changed
records rather than retaining a truncated report. Symlink ancestors, unrelated
workspace contents, heap dumps, and core dumps are excluded. Nonstandard Gradle
homes, deeper build directories and configured fatal-report destinations are
not searched. Gaps identify omitted records without echoing names or contents.
Raw data are private, potentially sensitive and never uploaded or printed.

Private host retention, quotas, incomplete export and operator interpretation
are owned by [Windows Runner Host](../../docs/operations/windows-runner-host.md).
Diagnostics supply evidence, not a resource remedy or tool-reuse correction.

## Android NDK contract

[`install-ndks.sh`](./install-ndks.sh) pins each numeric version, Google's
letter release, and the Linux archive's SHA-256. It verifies the archive before
extraction and checks its `source.properties` revision before adding matching
side-by-side `package.xml` registration. Archives and staging files stay out of
the final image. The SDK is writable inside each disposable container so setup
actions can add command-line tools, licenses, and packages. The Cache Volumes
section of [Windows Runner Host](../../docs/operations/windows-runner-host.md)
owns the Android storage exclusions.

An Android setup action MUST retain the incoming `ANDROID_SDK_ROOT` and export
both SDK variables to it; setting only `ANDROID_HOME` does not guarantee reuse.
There is no global
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

## Java, SDK, and Gradle inputs

The additional pinned inputs are:

| Input                          | Version                  |
| ------------------------------ | ------------------------ |
| Temurin HotSpot JDK            | `17.0.20.1+1`            |
| Android command-line tools     | `20.0`, build `14742923` |
| Android platform               | API 36, revision 2       |
| Android build-tools            | `35.0.0` and `36.0.0`    |
| Android platform-tools         | `37.0.1`                 |
| CMake with bundled Ninja       | `3.22.1`                 |
| Gradle binary distribution ZIP | `9.3.1`                  |

The [`Dockerfile`](./Dockerfile) pins each additional archive's exact URL and
SHA-256, verifies the bytes before extraction, and checks the installed JDK
and SDK revisions. Only selected installed directories cross into the final
image; acquisition staging files do not. Command-line tools and CMake receive
`package.xml` registration; the platform, build-tools, and platform-tools
retain their official `source.properties` for SDK-manager's legacy discovery.
License acceptance remains the setup action's responsibility.

The JDK occupies
`/opt/hostedtoolcache/Java_Temurin-Hotspot_jdk/17.0.20-101/x64`, with the sibling
`x64.complete` marker that `actions/setup-java` uses. Its cache label represents
`17.0.20+101`; its binary release is `17.0.20.1+1`. The image does not select a
global `JAVA_HOME` or change consumer setup-action inputs. A populated external
tool-cache mount can hide baked Java; the image does not synchronize that mount.

SDK packages occupy the standard versioned directories under the existing SDK
root. Matching setup actions and SDK-manager requests are intended to reuse
these installed inputs; a request for a missing or newer version retains normal
download behavior. Repository-metadata access, licenses, and other missing
packages can still require networking. This provisioning is not an offline guarantee for
an entire Android job.

The Gradle ZIP occupies
`~/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks/gradle-9.3.1-bin.zip`,
the ordinary wrapper location for
`https://services.gradle.org/distributions/gradle-9.3.1-bin.zip` with the default
Gradle user home. The wrapper extracts the ZIP itself; the image manufactures no
`.ok` marker. A different distribution URL or Gradle user home selects a
different location. The image pins the ZIP's checksum, but does not add a
wrapper checksum to consumers that omit one. Application dependencies and
generated build caches stay out of the image. The Cache Volumes section of
[Windows Runner Host](../../docs/operations/windows-runner-host.md#cache-volumes)
owns shared-volume policy and Android storage exclusions.

To refresh an input:

1. Confirm the production consumer's exact version.
2. Obtain the official archive and validate its published checksum. For Google
   SDK archives, verify SHA-1 first, then compute and pin SHA-256 from those same
   bytes. Adoptium and Gradle publish SHA-256.
3. Update the Dockerfile and smoke expectations. Keep the version list and
   build-cost figures in this README in sync.
4. Verify setup-action cache selection and ordinary SDK-manager requests.
5. Run a real matching wrapper invocation with networking disabled before
   claiming wrapper reuse. The smoke test's offline distribution execution is
   not itself a wrapper or setup-action integration test.

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
bash images/actions-runner/tests/diagnostics-test.sh
bash images/actions-runner/tests/check-download.sh
docker build --tag actions-runner:test images/actions-runner
bash images/actions-runner/tests/smoke-test.sh actions-runner:test
```

The standalone diagnostic test needs Linux `g++` (C++17) and `timeout`, but no
Docker daemon. It uses asymmetric cgroup fixtures, synthetic private markers,
raw-file boundaries, bounded attach subprocesses, and real launcher children.
Run tests and builds alone and sequentially in an orb. The smoke command also
runs these fixtures inside the actual image, followed by isolated diagnostic
container lifecycle checks without production registration or networking. That
command needs PowerShell 7 (`pwsh`) on the Linux test host for the real private
export path; the existing hosted image job supplies it.

CI runs these Linux x64 checks on every pull request without pushing the
result. The smoke test uses no network or mounts and checks:

- JDK compilation and execution.
- Offline SDK inventory and SDK executables.
- The Gradle ZIP's checksum and integrity, with offline execution of its extracted distribution.
- Arm64 C++ compile/link and ELF properties for both NDKs.
- Absence of image-declared volumes and entrypoints, and of generated Gradle user-home state.
- Isolation of SDK, JDK, and ZIP mutations between fresh containers.

The download test rejects corrupt
NDK input before extraction and checks that non-x64 NDK installation is skipped.
The new tool provisioning also skips non-x64 architectures before download or
execution; builds on those architectures are not covered by this Linux x64 smoke.

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
cost, not a measured final image delta. The additional Java, SDK, CMake, and
Gradle archives total 726,017,049 bytes, including the retained wrapper ZIP.
Record the candidate image identity, uncached build duration, and final image
size delta against the unchanged base image when verifying an update. These
download figures are not measured job-time or CPU savings.
The operator rebuild and rollback
procedure is in the Building the Runner Image section of
[Windows Runner Host](../../docs/operations/windows-runner-host.md).

The [initial image size/build-cost evidence](https://github.com/axross/runners/issues/13#issuecomment-5963050240)
records the exact source/image identities, raw layer bytes, uncached timings,
and environment limits. Those historical measurements are not a size promise
for a later base/version update or a runner-host speed prediction. Build caches
can retain the installation stage independently of the host's dangling-image
prune.
