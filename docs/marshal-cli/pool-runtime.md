---
type: Architecture
title: "Pool runtime and slot cycle"
description: "How does the runtime choose a backend, register a runner, place CPUs and recycle a slot?"
---

# Pool runtime and slot cycle

## The Runtime Layer

The library holds the code that runs a host's pools: the container backend, JIT
registration, and the loop of one slot. `start` reaches it through the instance
layer, and the tests also drive it through its seams.

| Module                                                   | Holds                                                                                                                     |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `affinity`                                               | The CPU offsets `validate` and the backend share, and the selection of a slot's CPUs                                      |
| `runtime::backend`                                       | The backend seam                                                                                                          |
| `runtime::select`                                        | The runtime a pool's operating system selects on its host                                                                 |
| `runtime::docker`, `runtime::docker_args`                | The container backend and the exact `docker` argument lists                                                               |
| `runtime::tart`, `tart::args`, `tart::client`            | The `tart` client: the exact argument lists, and the bounded process wrapper                                              |
| `runtime::tart::list`, `tart::names`                     | The reader of `tart list --format json`, and the VM names, parsers and matchers                                           |
| `runtime::tart::backend`                                 | The Tart backend: the slot cycle's host steps, the probes, the job container, and the release                             |
| `tart::golden_check`, `tart::guest`                      | The golden VM check from VM names alone, and the commands run in a booted VM through the guest agent                      |
| `tart::boot`                                             | The wait for a booted VM's guest agent and the boot failure text, shared by a job VM and the build VM                     |
| `tart::hold`, `tart::teardown`, `vm_shim`                | The holder of a VM's `tart run`, the stop-and-delete within a budget, and the VM shim                                     |
| `golden`, `golden::pins`, `golden::key`, `golden::steps` | The golden VM's pins, the check of Docker's signing key, the launcher and the guest calls that provision it               |
| `macos::version`                                         | The tested Tart and Softnet versions, the readers of their `--version`, and the sorting                                   |
| `macos::softnet`                                         | Finding Softnet on `PATH` as Tart does, the judgment of its file, and its version read                                    |
| `macos::prerequisites`                                   | The host checks: Tart's version, then Softnet's file and version, each a pass, warning or refusal                         |
| `runtime::tart::macos`                                   | What the Tart backend does differently for a macOS pool: the clone source, the VM-limit refusal, and the probe's judgment |
| `macos::count`, `macos::place`                           | The count of macOS VMs against the limit of two, and the lock under which a slot takes the last place                     |
| `macos::name`                                            | The names of the golden macOS VM and a macOS build VM                                                                     |
| `golden::macos`                                          | The guest calls that provision the golden macOS VM, and the launcher and probe calls of a macOS job VM                    |
| `runtime::registration`, `token`, `scrub`, `secret`      | JIT registration, the token file, error text, and the type that hides a secret                                            |
| `runtime::clock`, `runtime::log`                         | The time source and the sink for the runtime's messages, each a seam the tests replace                                    |
| `runtime::name`                                          | Runner and container names, and the test for a pool's own                                                                 |
| `runtime::pool`                                          | Pools from a plan, and their preparation                                                                                  |
| `runtime::worker`                                        | The slot loop and its backoff                                                                                             |
| `runtime::job_marks`                                     | What a runner's output says about its job, idle, busy or unknown, which a drain reads                                     |
| `runtime::retained`                                      | The diagnostic containers the backend retains, their records, and who finalises each                                      |
| `runtime::diagnostic`                                    | Finalising diagnostic containers at job end, in the container steps, and at startup                                       |
| `diagnostics`, `diagnostics::exporter`                   | The exporter child and the lines and exit codes it answers with                                                           |
| `diagnostics::transport`                                 | The transport seam through which the exporter reads a container's evidence                                                |
| `diagnostics::sink`, `archive`, `identity`               | The sink checks and admission, the strict archive reader, and the identity readback                                       |
| `runtime::board`                                         | Each pool's and each slot's state, which `status` reports                                                                 |
| `macos::signals`, `macos::mac`                           | The signal-source seam and its real macOS source                                                                          |
| `macos::sleep`, `macos::network`, `macos::watch`         | The sleep judgment, the `route` and `arp` readers, and the primary network's judgment                                     |
| `macos::keep_awake`, `macos::probe`, `macos::reason`     | The `caffeinate` hold, the bounded system-program call, and the stop-reason texts                                         |
| `macos::monitor`, `macos::gate`                          | The instance's signals on axmc: the watch, the keep-awake hold, and the gate that holds back the first clone              |
| `macos::fake`, `macos::scripted`                         | The in-process signal source for tests, and the one a debug build reads from a directory                                  |
| `runtime::fake`                                          | In-memory stand-ins for the seams, used by tests; a debug or test build only                                              |

### Runtime Seam

A pool's operating system, on the host that runs it, selects its runtime. The
selection is `Runtime::of_pool` in `runtime::select`:

| Host | Pool               | Runtime                                                                                              |
| ---- | ------------------ | ---------------------------------------------------------------------------------------------------- |
| axpc | every pool (Linux) | The Docker Desktop container runtime, the [Docker backend](../marshal-cli/docker-backend.md#backend) |
| axmc | `os: "linux"`      | The VM runtime, a Tart VM per job, run by `start`                                                    |
| axmc | `os: "macos"`      | The VM runtime, a Tart macOS VM per job, run by `start`                                              |

The [Docker backend](../marshal-cli/docker-backend.md#backend) implements the Docker Desktop runtime. The runtime core takes nothing from
the Docker modules: it drives the containers, and the per-job host steps under
[Backend](./docker-backend.md#backend), through the backend seam, and the run command and the engine
poll interval are values the backend provides. `start` constructs the backend the
host's profile selects: the Docker backend on axpc, and on axmc the
[Tart Backend](./tart-backend.md#tart-backend). The shim constructs the Docker backend directly.

The core runs a runtime that gives each job a virtual machine. A plan of axmc
Linux pools becomes pools: each slot's host has the pool's whole `cpus` as
vCPUs, the pool's `memoryGb` and the pool's `diskGb`, and the job container's CPU
quota and affinity cover all of the host's vCPUs, 0 to `cpus` - 1, with its memory
limit equal to the host's memory. The image defaults to `actions-runner:local` when
`imageName` is absent. `start` and the core apply that default, and `validate` and
its summary do not. Each Linux or macOS pool of a plan is made into a VM pool. The VM
runtime is the [Tart Backend](./tart-backend.md#tart-backend), which `start` runs, and
`build --profile axmc` makes its golden VMs:

- `start --profile axmc` runs every pool on the Tart backend: a Linux pool's job VM
  is cloned from the golden Linux VM, and a macOS pool's from the golden macOS VM.
  `start` refuses a configuration with a macOS pool when no golden macOS VM of the
  current version exists, naming `marshal build`, as it does for a Linux pool. On a
  Windows host, `start --profile axmc` exits 1 first, before taking the lock or
  calling any engine, with `error: the axmc profile runs only on macOS`; a Linux
  host is refused as unsupported, as under [Start](./foreground-commands.md#start).
- `build --profile axmc` makes the golden VM, as
  [The Golden VM Build](./build-images.md#the-golden-vm-build) describes. On a Windows host it exits 1
  first, before taking the lock, with `error: the axmc profile runs only on macOS`.

A refusal prints on standard error, as every `start` and `build` error before
the log opens. axpc is unaffected: its commands, messages, exit codes, and the run
contract keep their text.

The diagnostics export reads a retained container's evidence through the
`EvidenceTransport` seam in `diagnostics::transport`: the allowlisted identity
text, and the evidence archive, which the strict archive reader writes into the
bundle. The exporter child's admission, bundle files, limits, exit codes, and
deadline do not depend on the transport. `DockerCopy` runs `docker inspect` and
`docker cp` as [Backend](./docker-backend.md#backend) lists them. `VmCopy` runs the same two commands
inside a job VM as `tart exec <vm> docker ...`, with the same identity cap and
archive limits. The exporter child picks the transport from its arguments: with
`--vm <name>` it reads that VM through the Tart program it is given, and without it
it reads the container with `DockerCopy`, so axpc's exporter command line is
unchanged. Both backends start the child, take its admission lock, and read its
`admitted` and `bundles` lines through `runtime::exports`.

A unit test in `runtime::select` pins the table. `axmc_start.rs` runs a
configuration of Linux and macOS pools, and one of macOS pools alone, through
`start` as processes, unit tests in `profile` and `build` pin the Windows refusal,
and `golden_build.rs` runs the golden VM build. Unit tests in `diagnostics::exporter` write a complete bundle, a
gap, and the failed bundles of an unreadable identity and a failed archive through a
scripted transport, and the
diagnostics tests listed under [Diagnostics](./diagnostics.md#diagnostics) run the exporter through
`DockerCopy` with unchanged expectations. `tart_diagnostics.rs` and
`axmc_diagnostics.rs` run it through `VmCopy` against the fake `tart` and the fake
`docker` in its VMs.

### CPU Placement

A slot starts at its pool's offset: the CPUs of every earlier pool's slots, then
the CPUs of its own earlier slots, each slot counting `ceil(cpus)` CPUs. The summary
`validate` prints and the backend call the same functions in `affinity`, and a test
compares what the summary prints with what the pools carry.

The probe is a short container with no network, no mounts, no registration and no
pull, which prints the CPUs it may use. A leftover probe of the same name is
removed first, and only when `docker ps` finds it by its exact name and the
`runners.cpu-probe=1` label and prints exactly that name. The probe has a budget of
30 s, of which 1 s is kept to end a client that overran it. Removing a leftover
probe has 5 s. Output is capped at 64 KiB, and one byte more is refused. Standard
error is discarded. The removal runs again after the probe whatever happened, and a
failed removal outranks the probe's result. Any doubt is an error; the backend never
returns an unrestricted set.

The selection starts at the offset modulo the number of allowed CPUs and takes
consecutive IDs from the sorted list, wrapping. It writes them as `a,b-c`. The list
must be ordered, distinct ranges of IDs that fit a 32-bit signed integer, with no
space, sign or leading zero. The syntax of the whole list is judged before any ID's
range or order, so a list with several defects is refused as invalid. A test pins
the golden case: the inventory `2,5-6,11,14-15` with five slots of two CPUs gives
`2,5`, `6,11`, `14-15`, `2,5`, and `2,15`, and
`the_rejected_inventories_are_rejected` in `cli/src/affinity.rs` lists the
inventories the selection refuses.

### Registration

A repository registration is `POST /repos/<owner>/<repo>/actions/runners/generate-jitconfig`
to `https://api.github.com`, with these headers
(`Authorization: Bearer <token>`, `Accept: application/vnd.github+json`,
`X-GitHub-Api-Version: 2022-11-28`, `Content-Type: application/json`) and this body
(`name`, `runner_group_id` 1, and the pool's labels). It reads `encoded_jit_config`;
a response without one is a failure. The client also:

- it sends `User-Agent: marshal/<version>`;
- it follows no redirect, so a redirect is a failure;
- it fails a connection after 10 s and a whole request after 30 s;
- it fails a response larger than 1 MiB, whether the size is declared or streamed.

The repository's owner may be a personal account or an organisation; the request and
the pool's one-repository token are the same for both. The Fine-Grained Token section
of [Windows Runner Host](../runner-hosts/registration.md#the-fine-grained-token) gives the token steps for an
organisation-owned repository.

Each failure goes into the slot's backoff. The error text has the token replaced by
`<token>`, whitespace collapsed, and is cut to 500 characters followed by `...`.
The token is read again from its file at every
registration, trimmed, and held in a `Secret`; the header that carries it is marked
sensitive. The HTTPS client is built without reqwest's `json` feature: the body is
three validated fields written by a small writer with the escapes RFC 8259
requires, and the response is read with the crate's own parser. The client keeps
reqwest's handling of the `HTTPS_PROXY` and `NO_PROXY` environment variables, and
does not read the Windows system proxy; no test covers the proxy variables.

TLS uses rustls with the `ring` provider, installed explicitly by
`install_tls_provider` before the client is built, and the platform's certificate
verifier. Installing twice is fine. A failure to install it or to build the client
is a setup error for the caller to report, never a panic. A test builds the client
on every operating system in CI; a handshake with GitHub, and a real registration,
are verified on axpc (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)).

The base address can be overridden only by a debug build, and only to
`http://127.0.0.1:<port>`, for the loopback tests; a debug build refuses any other
value. A release build ignores the request and always uses GitHub, so a release
binary cannot send a token anywhere else. `tests/base_address.rs` shows both halves,
and CI runs it again in the release profile.

An organisation registration first reads
`GET /orgs/<org>/actions/runner-groups?per_page=100` and finds the configured
`runner_group` by exact name. Every required page must succeed. It accepts only a
non-default, non-inherited group with `visibility: selected` and
`allows_public_repositories: false`; `restricted_to_workflows` may be either
value. It does not read the group's selected repositories, so an empty group is
accepted. Missing, renamed, ambiguous or refused groups fail registration with
their cause and enter the slot's backoff.

The group check uses each page's last `ETag`, and a cached page is usable only
after an authorised `304` for that request. A failed read never reuses an old
permission. Next pages must have the first request's origin and either its exact path
or GitHub's `/organizations/<id>/actions/runner-groups` form of it, with one id for
the whole read; cycles, more than ten pages, missing flags and invalid responses fail closed.
The check repeats before every registration, outside the display queue and its
budget. A group fixed in GitHub is accepted at the next attempt, without a
restart. Only then does the registrar send
`POST /orgs/<org>/actions/runners/generate-jitconfig` with that check's group id.
Both the reads and the POST keep the outbound limits and secret handling above.

### Names and the Slot Loop

An always-on runner is named `<pool>-<slot>-<yyyyMMddHHmmssfff>` in UTC, and its container has
the same name. A name is recognised as a pool's own only when it is the
pool's name, a hyphen, a slot number, a hyphen, and exactly 17 ASCII digits.

An always-on slot loops until it is asked to stop. Each attempt checks the image, then prepares
the job host, then probes the CPUs, then registers, then runs the container, then,
for a diagnostics-enabled pool, exports it, and finally releases the host. An image
that cannot run jobs, or a check Docker does not answer, registers nothing and goes
into the backoff. A failed or held preparation, a failed probe or registration, or a
non-zero exit releases the host and then waits
5 s, then 10 s, doubling to 300 s; an exit of 0 resets the delay and starts the
replacement at once. A wait ends early when a stop is asked for, and a stop that
arrives as a registration completes starts no container. Time and sleeping
come through a `Clock` seam, so the tests do not wait.

Preparing the pools puts the image check between the wait for Docker and the
pools' cleanup. A pool whose token file is
missing or blank is skipped with a warning that names the pool and the reason, and
never the token. Docker is waited for once. Each remaining pool's own stale
containers are cleaned up and its volumes prepared; a pool that fails either is
skipped. When no pool is left there is nothing to run. Stale cleanup removes only
containers that match the pool's own pattern and do not carry the
`runners.diagnostic-lifecycle=1` label; a failed removal is a warning only, and a
failed listing skips the pool. The backend's own retained containers keep their
records and are left alone. Any other labelled container is finalised with the
pool's current settings when the pool's diagnostics are on; when they are off it
gets a bounded stop and the pool is refused, as [Diagnostics](./diagnostics.md#diagnostics)
describes.

### What the Tests Cover

`docker_backend.rs` runs the real backend against the `fake-docker` helper, a
program built by Cargo because Windows finds only `.exe` commands. It checks the
example configuration's argument lists, the JIT variable reaching only the `run`
client and no argument, that no client is given a socket mount, the process groups, the probe limits, the stale cleanup and
the wait for Docker. `registration.rs` runs the client against a loopback stub for
the request, the error text, a redirect, a stalled response and an oversize
response. `slot_worker.rs`, `pool_preparation.rs` and `cpu_affinity.rs` use the
fake seams. `job_host.rs` scripts a failure at each step of the slot cycle on the
fake backend and shows the host released exactly once before the backoff, a failed
probe registering nothing, a held slot on the board retrying on the backoff
schedule, and the cycle's order; `pool_preparation.rs` also shows an axmc Linux
pool's quota, affinity, memory, disk and default image, and
`instance_state.rs` and `core_restart.rs` the release at a stop and at a core
failure. `image_check.rs` drives the image check through the fake backend,
the instance, and the real backend against `fake-docker`. The diagnostics tests are
listed under [Diagnostics](./diagnostics.md#diagnostics). `tart_client.rs` runs the `tart` client
against the fake `tart`: the exact argument lists, a run's options and Softnet rules,
the environment and the standard output every call gets, the JIT configuration
reaching the guest on standard input only, the 64 KiB cut, the budgets, and the VM
names and the listing reader. `host_prerequisites.rs` runs the prerequisite checks
against the fake `tart` and `fake-softnet`. `golden_provisioning.rs` runs the golden
VM's guest calls against the fake `tart` in their order, with a key of another
fingerprint stopping them before anything is installed, and `golden_launcher.rs` runs
the launcher as a shell script against a stand-in `docker` (Unix only).
`golden_install_docker.rs` runs the install-docker script as a real shell script
against stand-in system tools (Unix only): a missing `docker` group is created with
`DOCKER_GROUP_ID`, a group with that id is accepted with or without members, and a group
with another id is refused before apt runs.
`tart_backend.rs` runs the
[Tart Backend](./tart-backend.md#tart-backend) against the fake `tart`, which forwards a guest
command to `fake-docker` as the launcher would, and `fake-softnet`, through the rig
in `tests/support/rig.rs`: availability and the golden VM check, the cleanup of
leftovers, each step of a slot's cycle and its failures, the boot failure text, the
probes, the JIT configuration reaching the guest on standard input alone, the run
arguments and the VM's one socket mount, bounded and masked output, the stop and release within their budgets,
and a release run twice at once. `tart_diagnostics.rs` and `axmc_diagnostics.rs` run the
diagnostic VM cases listed under [Diagnostics](./diagnostics.md#diagnostics). `vm_shim.rs` runs the VM shim as a real process,
for a job VM and for a build VM, and `tart_slot.rs` runs the slot loop on the
backend with the fake programs, for the order of a cycle and what each failure does
to the VM and the registration. Both are Unix only. `real_docker.rs` runs a container and the
probe through a real Docker daemon on Linux, and exports a container's small
`metrics.txt` through the exporter child from a real `docker cp` stream; it is
`#[ignore]`d for a local `cargo test`, and CI runs it in its own step, where it
fails when Docker or the image is missing. Not verified by these tests: a real
Docker daemon outside that CI step, a real Tart, Softnet and guest agent (the fakes
stand in for them, and the guest user comes from the agent's source, not from a
run), the TLS handshake with GitHub, and Windows and macOS runtime behaviour beyond
what the hosted runners show. The handshake and a
real registration are verified on axpc instead, on the maintainer's report, as the
TLS paragraph above says.
