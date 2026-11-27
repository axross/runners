---
type: Specification
title: "Runner image compatibility and freshness"
description: "Which image labels, checks and ages permit a pool to start?"
---

# Runner image compatibility and freshness

## The Image Contract

The CLI runs jobs only in an image built for its run contract: the argument lists,
the launcher, and the labels under [Backend](./docker-backend.md#backend), and what they need from the
image. The image declares the contract it was built for, and the CLI checks it
before any pool runs and before each job.

| Module                | Holds                                                                                    |
| --------------------- | ---------------------------------------------------------------------------------------- |
| `image::contract`     | The label names, and this CLI's contract version, read from the embedded Dockerfile      |
| `image::definition`   | The image definition embedded in the binary, and the debug-only test definition          |
| `image::archive`      | The build context: a ustar archive of the definition, written by the CLI itself          |
| `image::check`        | The image check's format and outcomes, the refusal, the freshness warnings, and `status` |
| `image::runner`       | The runner record: each declared runner version's release date and osx-arm64 SHA-256     |
| `image::macos_base`   | The macOS base record and its age warnings                                               |
| `image::macos_golden` | What `status` and the snapshot say about the golden macOS VM, and its age warnings       |

### Labels

An image `build` makes carries five labels. The Dockerfile's final stage declares
the first three, so an image built from the definition another way, in CI or by hand,
carries those three only; `build` adds the last
two on its command line, because they describe the build, not the definition.

| Label                                       | Value                                           | Set by         |
| ------------------------------------------- | ----------------------------------------------- | -------------- |
| `com.github.axross.runners.contract`        | The image contract version, a positive integer  | The Dockerfile |
| `com.github.axross.runners.runner-version`  | The runner version the base image pins          | The Dockerfile |
| `com.github.axross.runners.runner-released` | That release's date, `YYYY-MM-DD`, at 00:00 UTC | The Dockerfile |
| `com.github.axross.runners.image`           | `actions-runner`, which the prune filters by    | `build`        |
| `com.github.axross.runners.built`           | The build time, in Unix seconds                 | `build`        |

The CLI's own contract version is read from the same embedded Dockerfile when the
binary is compiled, so the two cannot drift inside one binary; a Dockerfile without
a well-formed version does not compile. The version rises when the run contract
changes or the image must provide something it did not. The two runner lines the
[update drain](./agent-lifecycle.md#the-update-drain) reads are not part of it: they come from the
runner, and a runner bump keeps the contract version, so the bump checks them
instead, as [Bumping the Runner](../releases/runner-update.md#bumping-the-runner)
describes.
Contract 1 is the run
contract as Phase 2 left it, plus the diagnostic variant's use of the collector's
path and output directory. Contract 2 adds what the VM runtime needs of the image: a
Docker client at the version the golden VM's Docker Engine is at, installed from
Docker's apt repository after its signing key matches a recorded fingerprint, and a
`docker` group with the id `DOCKER_GROUP_ID`, so the runner can use the VM-local socket
the VM runtime mounts. The Docker runtime's arguments are unchanged and it still mounts no
socket, but an image of contract 1 is mismatched, so axpc needs one image build
after the release that carries contract 2. The golden VM's name records the contract,
so a golden VM built for contract 1 is mismatched the same way.

Unit tests in `cli/src/image/contract.rs` keep the declaration honest:

- `APPROVED_CONTRACTS` pins a fingerprint of the run contract's argument lists, the
  launcher, the diagnostic variant's argument lists and launcher, the VM runtime's
  argument lists with the VM-local socket mount, the label keys and the inspect
  format, against the version they were approved for. Changing any of them without
  raising the contract version fails, and the failure names the row to add with the
  new version. A new version is approved by adding that row, with the fingerprint the
  failure prints.
- The Dockerfile must hold the golden VM's pins, as
  [Golden VM Provisioning](./tart-backend.md#golden-vm-provisioning) says, and the image smoke test
  must check the same client version and group id.
- `RUNNER_RELEASES` in `cli/src/image/runner.rs` is the runner record: each
  declared runner version's release date and the SHA-256 of its osx-arm64 archive.
  The declared runner version, `DECLARED_RUNNER_VERSION`, must equal the base
  image's tag and the `runner-version` label, have an entry, and declare the
  entry's date. A Dependabot runner bump stays red until both labels, the declared
  version, and the entry are updated by hand; see
  [Development Workflow](../releases/runner-update.md#bumping-the-runner).
- The embedded set must be exactly the Dockerfile and every file it copies, and
  `cli/tests/build-definition/Dockerfile` must start from the same base and declare
  the same labels.

### The Image Check

The check reads the configured image's labels with one
`docker image inspect --format [{{json (index .Config.Labels "<label>")}},...]`, for
the contract, build-time, and runner labels. A name whose last path part has no tag
is checked as `<name>:latest`, as `docker run` resolves it. When inspect fails, a
`docker image ls --quiet <name>` tells a missing image from Docker not answering.
Both calls share a bound of 4 s. A resume's reload is the configuration read, then
on axmc the [host prerequisites](./tart-backend.md#host-prerequisites), whose two version reads run
together and get 2 s each, and then this check, whose one listing also judges the
golden macOS VM when a macOS pool is configured. It takes 4 s at the most on axpc and
6 s on axmc, so it fits inside `start`'s 10 s wait. It follows the container steps of
a `restart`, which take 28 s at the most, so it ends at 32 s on axpc and 34 s on axmc,
inside the 35 s wait.

| Outcome            | When                                                                         |
| ------------------ | ---------------------------------------------------------------------------- |
| Matching           | Inspect succeeds, and the contract label equals the CLI's                    |
| Mismatched         | Inspect succeeds, but the contract label is missing, malformed, or different |
| Missing            | Inspect fails, and the listing succeeds with empty output                    |
| Docker unavailable | Any other failure, or the 4 s running out                                    |

On axmc the check reads the golden VM instead, from the VM names alone, as the
[Tart Backend](./tart-backend.md#tart-backend) describes: a matching golden VM carries this CLI's
golden VM version and image contract version, a missing one is `missing`, and any
other is mismatched. Their messages are
`the golden VM <name> has golden VM version <g> and image contract <m>, but this CLI needs golden VM version <G> and image contract <n>`
and `the golden VM is missing; this CLI needs one with golden VM version <G> and image contract <n>`.
A `tart list` that fails or overruns the same 4 s is the unavailable outcome.

Docker unavailable is never a mismatch. A mismatched image's message is
`the image <name> has image contract <m>, but this CLI needs image contract <n>`,
and a missing one's is
`the image <name> is missing; this CLI needs one with image contract <n>`. The
check runs:

- **At the first run of the core.** Once Docker answers, before any pool is
  prepared. An image that cannot run jobs ends the instance, and `start` exits 1.
- **At a resume, a `restart`, and a core restart.** As
  [Instance State](./foreground-instance.md#instance-state) describes: the instance stays stopped, with
  the stop, build, start sequence as its reason.
- **At each job launch.** Before the CPU probe and registration. An image that
  cannot run jobs registers nothing and goes into the slot's backoff with the
  message; a check Docker does not answer goes into the backoff as a Docker
  failure. `build` cannot run while an instance exists, so a mismatch here means the
  image was replaced another way, such as by a build made by hand.
  `docker run` resolves the name again after the check, and a replacement between
  the two is the accepted residual case.

### Freshness

The check also reads the build-time and runner-release labels. An age is
⌊(now − t) / 86,400 s⌋ in days, where t is the build time, or the release date at
00:00 UTC. An age under 7 days gives no warning, 7 to 20 days the first, and 21 or
more the second:

| Age        | Build age                                                                | Runner release age                                                                                                                                                                 |
| ---------- | ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 7 to 20    | `the image is <d> days old and should be rebuilt`                        | `runner <v> was released <d> days ago, and a newer runner may exist`                                                                                                               |
| 21 or more | `the image's operating-system packages are <d> days old; build it again` | `runner <v> was released <d> days ago and never updates itself; update Marshal to get a newer runner, then build the image again before GitHub stops sending jobs to this version` |

An image without a build-time label, such as one CI or a build by hand made, has an
unknown build age, and its warning says it was not built by
Marshal. `start` logs the current warnings at each run of the core, at
start, a resume, a `restart`, and a core restart, and not at a job launch. Neither
the warnings nor an unknown age block a start.

`status` shows the image as of the latest check, a job launch's included:

- `image: <name>`;
- `image-contract: <n>`, `mismatch (image <m>, CLI <n>)`, `missing`, or
  `not checked`, which shows only before the first check Docker answers; a later
  check Docker does not answer leaves the latest answer in place; on axmc with
  only macOS pools it is `not used, no Linux pool`;
- `image-built: <d> days ago`, or `unknown`;
- `runner: <version>, released <d> days ago`, or `unknown`;
- on axmc, `golden-vm: <name>` once a golden VM exists, and `mismatch (golden VM
version <g> and image contract <m>, CLI golden VM version <G> and image contract
<n>)` in place of the mismatch above;
- one `warning: <text>` line per freshness warning.

`image-built` and `runner` read `unknown` before the first check Docker answers, and
when the image is missing or its labels are missing or malformed, or when no Linux
pool is configured.
