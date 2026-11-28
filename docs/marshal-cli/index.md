# marshal-cli

The [marshal-cli library](../../cli) supplies the agent in [Marshal](../marshal/index.md). The current workspace has no command-line target. Its foreground commands run through a test helper. The last command-line release, the CLI recovery release, keeps its installers and operating procedures at its own tag; [The Last Command-Line Release](./release-artifacts.md#the-last-command-line-release) identifies it. axmc awaits the [macOS app release](../runner-hosts/index.md#macos-with-tart).

## Entry points and configuration

- [Which executable runs the library and which entry points are test-only?](./entry-points.md)
- [Which host profile, checkout boundary and validation errors apply to a JSON configuration?](./json-validation.md)
- [Which TOML keys, validation errors, conversions and comment-preserving edits are supported?](./toml-configuration.md)
- [What does the agent do when a client reads, edits or reloads configuration?](./config-protocol.md)
- [Which lock, channel and log names identify the same host across releases?](./host-names.md)

## Pools, images and diagnostics

- [How does the runtime choose a backend, register a runner, place CPUs and recycle a slot?](./pool-runtime.md)
- [Which Docker commands, isolation and resource limits implement a container job?](./docker-backend.md)
- [How do the golden VM, job VM, shim and host prerequisites work together?](./tart-backend.md)
- [Which image labels, checks and ages permit a pool to start?](./runner-images.md)
- [Which sizing, base-image and VM-count rules apply to building and running a macOS job?](./macos-pools.md)
- [What does a build replace, and what happens to the previous image on failure?](./build-images.md)
- [When is evidence exported, retained or lost after a job or a host failure?](./diagnostics.md)

## Process ownership and client contracts

- [How do the lock, private files, control channel and state machine enforce one foreground instance?](./foreground-instance.md)
- [What do the test-only foreground commands do on startup, stop, signal or forced close?](./foreground-commands.md)
- [How do host signals reach a foreground instance and prevent an unsafe resume?](./mac-signals.md)
- [How does the resident agent serialize requests, maintain configuration and handle failure?](./resident-agent.md)
- [Which lifecycle settings and drain conditions govern stopping, resuming or updating the agent?](./agent-lifecycle.md)
- [Which credentials can the agent read, replace or remove on Windows and macOS?](./token-store.md)
- [Which messages and snapshot fields can a client rely on, including when versions differ?](./agent-protocol.md)
- [Which runner and job states can polling establish, and when does it back off?](./github-status.md)

## Build, release and installation

- [Which toolchain and CI gates exercise the library and app workspaces?](./toolchain.md)
- [Which crates and licences are permitted, and where is an addition reviewed?](./dependencies.md)
- [How should a contributor change the fixtures when validation changes?](./configuration-fixtures.md)
- [Which artifacts and jobs does the release workflow build, check and publish?](./release-artifacts.md)
- [Which signatures and identities bind the CLI, package and planned app artifacts?](./signing.md)
- [Which roadmap slice may extend the release graph, and what else must change with it?](./release-extensions.md)
- [How does an operator verify, install, repair or remove an app release?](./install-on-windows.md)
