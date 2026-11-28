---
type: Architecture
title: "Local agent and credential boundaries"
description: "Trace who can reach the resident agent, which process handles credentials and which evidence can leave a host."
---

# Local agent and credential boundaries

The resident agent holds the lock, handles credentials and runs jobs. The app and same-user clients reach it over a local channel. These boundaries govern configuration changes, job clients and private diagnostics.

## One Instance per Machine, Reached Only by Its User

The CLI's single-instance lock and control channel decide who can start, stop, or
query the process that holds every pool's token, so who can reach them is a
security rule. [CLI](../marshal-cli/foreground-instance.md#the-instance-layer) owns how each is
built: its names, modes, and checks.

- The lock is machine-wide and no child process inherits it. Every local user can
  try it, and anything planted at its path is refused rather than opened or waited
  on. The lock does not say who holds it, so another local user can hold it or take
  its name first and so keep `start` from running. On Unix the lock file's owner
  can also replace the file while an instance holds it, so a second instance can
  start: the [Unix residual case](../marshal-cli/foreground-instance.md#single-instance-lock). Both
  are accepted on a single-user host, and `start` reports a held lock.
- The control channel is per user: only the instance's own user can reach it, and
  no remote client can. Another user who takes this user's pipe name first stops
  the channel from opening, which is also accepted on a single-user host.
  On Windows the agent's client does not rely on the name alone: it checks that the
  process serving the pipe runs as the current user before it sends a byte, and a
  server it cannot show to be that user is refused. `start`'s commands do not
  check, and a squatted name there is the accepted cost above.
- No request carries a configuration path for the instance to read, and no
  response carries a token or a JIT configuration. A resume names a path only so
  the instance can compare it with the one it started with.
- The resident agent takes the same lock and opens the same channel with the same
  access rules, so only its user reaches it, and it serves protocol version 2
  there; [Agent protocol version 2](../marshal-cli/agent-protocol.md#protocol-version-2) owns the protocol. A first line
  that is not a `hello` is refused and the connection closed, so no version 1
  request reaches the agent. A silent or stalled client cannot hold the agent:
  every connection limit is a timer, a client that stops reading is closed after
  5 s, and the agent ends and frees its lock without waiting for the pipe crate's
  blocking flush thread that a stalled Windows client can keep alive (a process
  test shows it). Such a thread is at most one for each final message to a client
  that never reads and never closes, which is accepted on a single-user host.
- Both Windows channels, `start`'s and the agent's, take their pipe name, byte
  mode, refusal of remote clients and the only-user descriptor from one function,
  `instance::windows::control_pipe_options`, so the two cannot differ. A test on
  Windows shows a second pipe of the same name is refused as taken.
- Only a debug build can move the lock, the channel, the log directory, or their
  roots, for the tests; a release build always uses the host's fixed locations,
  and a CI step checks in the release profile that it ignores the request. The
  `instance-helper` binary that the tests and CI run does nothing in a release
  build, including its mode that runs the agent. The `commands-helper` binary,
  which runs the library's foreground `start`, `build` and clients for the tests,
  runs in both profiles so the release-profile tests can show the library ignoring
  those moves; it moves nothing itself, and a release never ships it.

## The Agent Keeps Tokens in the Operating System's Store

The resident agent reads each token from the operating system's
credential store, not from a file, and it receives a token from a client over its
control channel. [CLI](../marshal-cli/token-store.md#the-token-store) owns how the store is
named and used.

- On Windows a token is a generic credential in the user's Credential Manager,
  named `app.axross.marshal:github.com/<owner>/<name>` for a repository or
  `app.axross.marshal:github.com/<org>` for an organization, with **local** persistence.
  The crate's default persistence is the enterprise one, which roams with the user's
  profile and so would carry a repository's token to other machines of a domain
  account; the store asks for the local one, and a test on Windows reads the
  persistence back. Any process of the same user can read the credential, as any
  can read a token file the user owns: the Credential Manager is not a boundary
  against the user's other programs.
- On macOS a token is a generic password in the user's login keychain, with the
  service `app.axross.marshal` and the account `github.com/<owner>/<name>` for a
  repository or `github.com/<org>` for an organization; its label, which Keychain
  Access shows, is `Marshal: <owner>/<name>` or `Marshal: <org> (organisation)`. The
  agent is its only writer and reader. It adds each item with no access object of its
  own, so macOS trusts the adding program by its designated requirement and its
  team's partition, `teamid:<team>`. The requirement names the identifier
  `app.axross.marshal`, the Developer ID markers and the team, not a path or a
  certificate, so a later release signed by the same team reads the item; changing
  the identifier or the team breaks that trust. An ad-hoc build, such as a dry run's,
  is matched by its own code hash and reads only the items it stored. Setting a token
  deletes the old item and adds a new one, so the item trusts the build that stored
  it last.
  - Any program the same team signs under `app.axross.marshal`, including a signed
    `marshal` command line still on the Mac, can read the items. Another program of
    the same user can ask for one, and macOS then asks the user, naming its label:
    the Keychain is a boundary only as far as the user answers that dialog.
  - The agent turns the Keychain's user interaction off for its whole life, so it
    never shows a dialog. A read or write that would need the user, such as on a
    locked keychain, fails at once with the kind `needs_user`, and the pool is
    skipped with that reason rather than waiting on a dialog that an unattended Mac
    would leave unseen.
  - macOS refuses Keychain access to code changed on disk after it started; the
    agent's changed-executable rule already ends such an agent.
- On Linux the agent has no store, so it skips every pool with "no token store"
  rather than falling back to a file. On no OS does it read `tokenPath`.
- The `token_set` request is the first message in which a secret travels, from the
  client to the agent. The token is held as a `Secret` once the request is parsed,
  is masked in the log from then on, and is never repeated: not in the
  response, an event, a snapshot, an error, or a log line. An error about a token
  names its cause, such as an empty or too long token, and never the params. A
  store failure names its kind and the platform's code, which hold no token. The
  request is only as private as the channel: only the agent's user can send it, and
  on Windows the client sends it only after it has checked that the pipe's server
  runs as that user, so a squatter of another account receives nothing and a failure
  to verify is a refusal. The check is made once at connection, before any request,
  so it covers every request on the connection. A method added under protocol
  version 2's version rules that carries a secret is handled as `token_set` is. The
  residual risks are a squatter of the same user, which
  is accepted on a single-user host, and the tiny window in which a process ID is
  reused between the connection and the check.
- A token longer than 1280 UTF-16 code units or empty is refused before it reaches the
  store, so each token's size is bounded. The repository is not limited to the
  configuration's: `token_set` accepts any well-formed `owner/name`, because a token
  may be entered while the configuration is invalid.

Residual risks, accepted on a single-user host:

- A same-user client can create store entries for any well-formed key, one token of
  at most 1280 units each, and the agent never removes one it did not hear of. The
  agent's in-memory record of which keys have a token also gains an entry for each
  new name a request names, until the next configuration read replaces it; each key
  is bounded by its form, `owner/name` for a repository or a login for an
  organization.
- Another process of the same user that connects to the channel can set or remove
  a token, stop the runners or quit the agent, as it can for `start`'s
  commands.
- The agent holds a token in memory for as long as a registration or a GitHub status
  check needs it, and a
  JIT configuration a runtime-core panic's message carried would reach the log
  unmasked, as for `start`.

## Tokens Stay Out of the Log, and Job Clients End With `start`

`start` holds every pool's token and each slot's JIT configuration for as long as it
runs, and it writes a log that lives after it. [CLI](../marshal-cli/foreground-commands.md#running-a-host)
owns how the log is written and masked and how its children are contained.

- No token or JIT configuration reaches a log line, the terminal, a debug form, an
  error message, a command line, or a control-channel response.
- Every job container's `docker` client ends when `start` ends, however it ends.
- On Unix a diagnostic container's shim ends its container with `docker kill` and
  keeps it, rather than removing it, so its evidence survives for the next
  finalisation. A forced stop and the Windows console close do the same on purpose.
- Each export runs in an exporter child that ends at its deadline or when its
  liveness pipe from `start` closes, and kills its own clients first: on axpc its
  `docker` clients, and on axmc its `tart exec` clients, which run `docker` in the
  VM. Its clients run in process groups of their own, killed when their call is
  dropped, with the JIT variable cleared, and on Windows the child is also in
  `start`'s Job Object. `start` closes the pipe at the
  export deadline and kills the child 1 s later if it has not ended, so the child
  outlives neither its deadline nor `start`.
- The log directory and its files are per user and private to that user.

Residual risks, accepted on a single-user host:

- On Unix a shim outlives `start` by up to 5 s, and a shim that is itself killed
  leaves its container running until the next `start` removes it. On Windows a
  container outlives its killed client, with the same backstop.
- On Windows, logoff and shutdown events never reach an interactive process, so
  containers left by them wait for the next `start`.
- On Windows the log directory's access control is not checked; it relies on the
  user profile's.
- A JIT configuration that a runtime-core panic's message carried would reach the
  log unmasked, because its mask ends as the panic unwinds.
- A kept diagnostic container, ended but not removed, still holds its JIT
  configuration in its environment until it is finalised, where anyone who may run
  `docker inspect` on the host can read it. The ephemeral runner it configured has
  already ended with its job. On Windows a hard kill of `start` leaves
  a diagnostic job running, as it does an ordinary one, until the next `start`.
- An exporter child that `start` has to kill, because it did not end within 1 s of
  its pipe closing, cannot kill its clients: its `docker` clients on axpc, and its
  `tart exec` clients on axmc, which run `docker` in the VM. On Unix they run until
  each ends by itself, and macOS has no Job Object; on Windows the Job Object ends
  them when `start` ends. They carry no JIT configuration.

## The TOML Configuration Holds No Token and Quotes No Line

A configuration file is untrusted input, and an operator can paste a token into one
by mistake. [TOML configuration](../marshal-cli/toml-configuration.md) owns the format, its problems, and the write
path; these are the properties it keeps.

- The TOML format has no key for a token or a token file. `token_path` is an
  undefined key, and the conversion from JSON leaves `tokenPath` behind, so a file in
  this format cannot name a token's location.
- A syntax error and a wrong-type problem carry only the key path, the line, the
  column, and fixed text. The reader never uses the parser's display text, which
  quotes the source line, so a token pasted into a value, or on a line of its own, is
  not repeated in a problem, a log, or a terminal. A test pastes a token at every
  position of a valid file and shows it appears only as the name of an unknown key.
- The write path replaces the file in one step: a flushed temporary file in the same
  folder, renamed over `config.toml`. A reader sees the old file or the new one and
  never part of one. On Unix the temporary file is created with mode 0600, and the new
  file takes the old file's mode. A file error carries a path and an error kind, and an
  edit that cannot apply is named by the edit alone. A result that breaks the rules
  carries the validator's problems, which quote only what the first residual risk
  below describes.
- The clean-up of leftover temporary files removes only regular files named exactly
  like a temporary file of the write path, so it cannot remove a file the operator
  keeps beside the configuration.

Residual risks, accepted on a single-user host:

- A token pasted as a key name is echoed as the name of an unknown key, because
  that is how the key is found. A rule that compares a value, such as a duplicate
  label or a name that collides, names that value too, as the JSON problems do.
- The write path compares the file with the bytes it read just before the rename.
  That narrows the window in which an editor's save could be overwritten, and cannot
  close it.
- The write path does not flush the folder after the rename. A crash right after a
  write can leave the old file, and cannot leave a partial one.

## Editing the Configuration Through the Agent Moves No Trust Boundary

The agent's `config_read`, `config_edit` and `config_reload` methods change the
configuration, which decides the repositories runners register to, their labels and
the folder diagnostics are exported to. [CLI](../marshal-cli/config-protocol.md#editing-the-configuration)
owns the methods; these are the properties they keep.

- Only the agent's own user reaches the channel, under
  [One Instance per Machine, Reached Only by Its User](#one-instance-per-machine-reached-only-by-its-user),
  and that user can already edit the file. No request names a file for the agent to
  read or write: the agent edits only `config.toml` at its fixed path.
- No request or response carries a token as such. `config_read` sends only the keys
  the schema defines that the file sets with a value of the right type, also while
  the file breaks other rules; an undefined key or a wrong-typed value appears only as
  a problem, which quotes no value. A token pasted on a line of its own, under a key
  of its own, or into a number or a boolean therefore does not reach the window. A
  problem names a key path, as the first residual risk of
  [The TOML Configuration Holds No Token and Quotes No Line](#the-toml-configuration-holds-no-token-and-quotes-no-line)
  describes.
- An edit goes through the write path and then through the checks the agent loads
  with, the checkout rule at the diagnostic directory's own place included, so a file
  the app writes is one the agent would load. An edit or a reload that sets or
  changes a pool's `diagnostic_directory` also runs the exporter's sink checks on it
  (it exists, is a directory, is no link, lies outside the checkout, and is private
  on Unix) and is refused when it fails them; the problem names the key and the
  check, not the path. A directory the configuration in use already names is left
  to the exporter's own checks, as before.
- An edit of `schema_version` or of `[lifecycle]` is refused before the file is
  read.
- A `config_edit` whose `revision` no longer matches the file is refused, and the
  write path compares the file again just before it replaces it.

Residual risks, accepted on a single-user host:

- A same-user client can point a pool's diagnostics at any folder that passes the
  sink checks, as it could by editing the file.
- A token pasted into the value of a defined text key, such as a pool's name or a
  label, is a value of the right type, so `config_read` sends it to the window, which
  shows it in that field. The window is the same user's, who wrote the file.
- The revision is std's default hasher over the file's bytes, with their length:
  SipHash with fixed keys today, which std does not promise to keep. It detects an
  edit made elsewhere; it is not meant to resist a file crafted to collide, and a
  same-user writer needs no collision to change the file. An agent built with
  another Rust may compute another revision for the same bytes, which a client sees
  as `config_changed` and reads the file again.
- A repository no pool names any more keeps its stored token until the client
  removes it with `token_remove`; the agent does not remove it on its own.

## Diagnostic Evidence Stays Private

A diagnostic container's bundle holds what the collector observed during a job, and
the job ran code from the repository. [CLI](../marshal-cli/diagnostics.md#diagnostics) owns
how a bundle is written.

- The sink is a directory the operator created, outside the checkout and private
  to the user who runs the CLI, so the evidence reaches no other user and none of
  the repository's files. The CLI refuses a sink that fails its checks and never
  creates, repairs, or relaxes one; the checks and the modes a bundle is written
  with are under Sink checks in [CLI](../marshal-cli/diagnostics.md#diagnostics).
- For `start` the checkout is the one above the configuration file; the agent's
  configuration lives outside any checkout, so the agent finds the checkout from
  the sink itself, the first directory at or above it that holds both `.git` and
  `cli/Cargo.toml`, and refuses a sink at or under it, with the same lexical,
  link-free comparison and the per-profile case rule ([The Checkout
  Rule](../marshal-cli/json-validation.md#the-checkout-rule)). A sink the agent could not look at
  from its own place, such as a path of another host, is not compared.
- On Windows the CLI does not check the sink's owner or access rules, because no
  safe call reads them; a bundle inherits the sink's rules. The operator's ACL step
  in
  [Windows Runner Host](../runner-hosts/windows-diagnostics.md#collecting-private-diagnostics)
  is the only protection on a host that runs the CLI. A sink whose rules admit
  another user would expose the evidence to that user. This is accepted on a
  single-user host, with the ACL step kept in the procedure.
- The sink is never mounted into a job. The container's evidence leaves it only
  through the identity allowlist, which never reads the environment, and an archive
  reader that accepts only the collector's regular files, by name, within fixed
  sizes, so a job cannot write a link, a path outside its bundle, or more than
  40 MiB.
- A bundle is untrusted evidence. The CLI never prints or logs its contents, and a
  warning names only the pool, the container, and the gap.
- A labelled container is removed only by a finalisation or by the manual
  recovery, so a later `start` with the pool's diagnostics off does not destroy its
  evidence. [CLI](../marshal-cli/diagnostics.md#diagnostics) owns when a finalisation
  exports, and the cases where it removes a container without an export.
- A diagnostic VM on axmc is the exception, by design: the VM is the retained
  container, and it is deleted without an export in three cases only, a forced stop
  (a second SIGINT), a `start` that ends while it still holds the VM, whose VM shim
  deletes it, and a leftover at the next `start`. Each reports a diagnostic gap that
  names only the pool, the VM or container, and the gap. The paragraph "Evidence a
  VM loses" in the [Tart backend](../marshal-cli/tart-backend.md#tart-backend) section of the
  CLI document owns the cases.

## `build` Builds Only What the Binary Embeds

The runner image every job runs in is built by `build` from the definition the CLI
embeds when it is compiled, so what a host builds is what was reviewed and merged.
[CLI](../marshal-cli/build-images.md#build) owns the steps.

- `build` reads nothing from a checkout, the working directory, or the
  environment for its context: the embedded Dockerfile and the files it copies,
  streamed as an archive the CLI writes, are the whole context. A unit test fails
  when the embedded set is not exactly the Dockerfile and every file it copies.
- Only a debug build can name another definition, for the real-Docker test, and a
  release build ignores the request, as it ignores the other test overrides; a CI
  step checks this in the release profile.
- The context's base image is pinned by digest, and `--pull --no-cache` rebuilds
  every layer from it, so no cache left by another build feeds the image.
- The image CI builds is tagged on the hosted runner and never pushed, and the job
  holds no credential.
- `build` holds the instance lock, so it cannot replace the image under a running
  instance, and the instance checks the image's contract before each job, so an
  image replaced by one with another contract, or removed, registers no runner. A
  replacement between that check and `docker run` is accepted on a single-user
  host.
