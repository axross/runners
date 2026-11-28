---
type: Policy
title: "Marshal app and update trust"
description: "Review a change to the desktop client, its web view or the feed used to offer updates."
---

# Marshal app and update trust

The app presents the resident agent’s state and accepts operator input. Review the web-view boundary here and the [agent’s token handling](./local-agent.md#the-agent-keeps-tokens-in-the-operating-systems-store) at its owner.

## The App Surface

The Windows app is a notification-area icon, a menu and a web view that shows the
runners and takes each repository's or organisation's token. It runs as the signed-in user, beside an
agent of the same executable that holds the pools' tokens, so the question is what
the app may do with that reach. [App](../marshal/index.md) owns how it works.

- **The app process is a client only.** It never holds the instance lock, reads the
  configuration, touches the token store or runs Docker: it talks to the agent in
  protocol version 2 and passes a newly entered token to it in `token_set` or
  `organization_token_set`, then drops it. One executable runs as both, so the operating system cannot enforce the rule
  for the app mode. Only the `marshal-modes` crate reaches the library's agent entry,
  and only when the executable is started in its agent mode; the app's session
  reaches the agent through the library's client alone. A review checks that no
  other app crate takes the lock, opens the store or starts `docker`.
- **The web view loads only bundled content.** Its pages, styles, scripts and fonts
  are the files the app embeds. It never navigates away from them, never opens a
  window from page content, and the release build has no developer tools. The policy
  is set in `tauri.conf.json`, in full:

  ```text
  default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none'
  ```

  Tauri adds the scripts' hashes to `script-src`. The pages use no inline script, no
  `style` attribute and no `eval`. The page's element builder refuses the `style`,
  `href`, `src`, `srcdoc`, `action` and `formaction` attributes and every `on*`
  attribute; any other way an element could load or run something is refused by the
  content security policy above. Text from the configuration, Docker or the agent is set as text,
  never as markup. A change to the policy, to the files it governs or to the element
  builder needs the maintainer's review.

- **Four capabilities grant what a window may call, two of them on macOS only.** The app declares its commands, so
  each works only in a window whose capability grants it. The main window's
  capability grants `core:event:allow-listen` and `core:event:allow-unlisten` and the
  commands that read the view, start all, stop all, build, stop the build, try
  again, set a repository's token, remove a repository's token, set an
  organisation's token, remove an organisation's token, set a setting, hide the
  window, open a runner's job and forward configuration requests. `configuration`
  accepts a typed request for read, edit, reload or a separately chosen
  repository-token removal.
  The three configuration methods keep all file access with the agent; the page
  supplies no path, token, schema version or lifecycle field. Removal forwards the
  existing `token_remove` method, returns a store refusal separately from the saved
  edit, and reads no token. The confirmation window's capability grants the two
  commands that read its question and answer it. On macOS, where the
  unified title bar hides the windows' own (M2's option B), `macos-main-drag` grants
  the main window `core:window:allow-start-dragging` and
  `core:window:allow-internal-toggle-maximize`, to drag and zoom it by its header
  band, and `macos-confirmation-drag` grants the question window, which cannot be
  zoomed, only `core:window:allow-start-dragging`. None grants anything else, each is
  limited to its window, and a test shows that a call the
  capability does not grant is refused. The permission files Tauri generates are
  checked in, so a change to them shows in a diff, and Tauri's `dynamic-acl` feature
  is off, so no capability can be added while the app runs. Quit has no command: it
  runs from the menu, on the Rust side.
- **The page cannot open an address.** Open run names a pool and a runner, never an
  address. The app's `open_job` command looks the runner up in the agent's snapshot and
  the `github` topic, refuses a pool the agent does not list, and builds
  `https://github.com/<owner>/<repo>/actions/runs/<run_id>/job/<job_id>` itself from
  the pool's configured repository and the two numbers GitHub gave, then opens it in
  the default browser from the Rust side with Tauri's opener plugin. The plugin's
  page script, which sends link clicks to its own command, is off, and no capability
  grants any of the plugin's three commands, which a test shows are refused.
- **A token passes only through `token_set` or `organization_token_set`.** The window's token field is a password
  input without autocomplete or spell checking. The page clears it when the token is
  submitted, whatever the outcome, and keeps the token in no variable once it has
  called the command. A token half typed is cleared when the window hides: the app
  sends the page `marshal:hidden` before every hide, which needs no permission beyond
  listening, and the page also clears on the web view's own report. The Rust side
  trims it, refuses an empty one or one over 1280 UTF-16 code units, wraps it in the
  library's `Secret`, sends it once and drops it.
  No view state, event, log line or error text of the app holds a token, and the app's
  log masks a word shaped like a GitHub token as defence in depth. The app never
  displays a stored token.
- **No telemetry, no crash reports.** The app's only requests read the update feed
  and download an update, as the next bullet says; opening a job's page hands its
  address to the default browser. The agent's only requests go to GitHub's API.
  Neither sends usage data or a crash report, and
  no dependency may; see [The App's Dependencies](./dependency-pins.md#the-apps-dependencies).
- **Updates come only from the signed feed.** See
  [Updates Come Only From the Signed Feed](#updates-come-only-from-the-signed-feed).
- **The debug-only test variables are ignored in a release build.** A debug build's
  agent and app read `MARSHAL_TEST_ROOT`, `MARSHAL_TEST_DOCKER` and
  `MARSHAL_TEST_GITHUB_API`, so a test can run the executable against a scratch host;
  on axmc the agent also reads the CLI's debug-only `tart` and signal variables, and
  on the Mac the app reads `MARSHAL_TEST_ANY_LOCATION`, which lifts the rule that
  only a copy in Applications controls an agent. A release build ignores all of them,
  so no environment can move the lock, channel, log, configuration, `docker` or
  `tart` program or GitHub address of an installed Marshal, or let a copy outside
  Applications start an agent, as for the CLI under
  [One Instance per Machine](./local-agent.md#one-instance-per-machine-reached-only-by-its-user). The
  `app-macos` job tests the location rule in the release profile with the variable
  set.
- **The app starts its agent from a file it checked.** It starts it from its own
  executable's absolute path as a plain detached child, never through Tauri's shell
  plugin, and never from a file that is missing, that it could not read when it
  started, or that is no longer the file the app started from. The agent ends within 5 s of the file it started from being moved,
  renamed, replaced or deleted, with the reason `executable_changed`, and the app ends
  with it.
- **The WebView2 runtime is the operating system's.** Windows 11 ships it and
  updates it, and the app bundles no copy, so the web view's engine is as current as
  Windows keeps it; the loader the app links is covered by the licence notices.

Residual risks, accepted on a single-user host:

- Any program running as the user can read a token's Credential Manager entry, can
  connect to the agent's channel and stop the runners, and can read the web view's
  data folder. The Credential Manager and the channel are not boundaries against the
  user's other programs; [The Agent Keeps Tokens in the Operating System's
  Store](./local-agent.md#the-agent-keeps-tokens-in-the-operating-systems-store) says the same of the
  store.
- An agent that hangs cannot be ended by Quit, so the goal that quitting leaves no
  Marshal container or process holds only while the agent answers. A Quit the agent
  never answers keeps the app saying it is quitting until the agent's `ending` or the
  connection's end, so the user ends `marshal-app.exe` in Task Manager. The window
  says how only for an agent that does not answer its handshake. The next start
  removes the containers it left.
- Whether WebView2 enforces the policy, whether the agent, the exporter or one of
  their `docker` clients shows a console window (the agent is created with
  `DETACHED_PROCESS`, and the exporter and the clients with `CREATE_NO_WINDOW`, each
  asserted at its command builder, but no check observes the window), and whether the menu opens from the keyboard are shown only on axpc;
  [App](../marshal/availability.md#verification-status) lists what has and has not been
  shown.
- The app checks its executable and then starts it by path, in two steps, so a file
  put at that path between the two is started. Whoever can do that can already
  replace the executable before the app starts.

## Updates Come Only From the Signed Feed

The Windows app installs a new release by itself, so whoever can put a file in front of
it could run code as the user. [App](../marshal/updates.md#updates) owns how an update
runs; these are the rules it is reviewed against.

- **Only the app mode updates.** Only the app mode registers Tauri's updater plugin, on
  Windows; the agent, exporter and shim modes never do. The flow runs in Rust, and no
  capability grants the web view an updater command, which a test of the capability
  files shows.
- **One feed, one key.** The feed is
  `https://github.com/axross/runners/releases/latest/download/latest.json`, the only
  endpoint `tauri.conf.json` names. The app verifies every downloaded MSI against the
  one public key committed there, with `requireSignedVersion`, so the signature's
  trusted comment must name the feed's version. A build with no committed key, or a
  development build at `0.0.0-dev`, never checks.
- **Separate release trust.** The publication check MUST use the hash-checked feed
  check with the public key embedded from `app/crates/feed/release-public-key.txt`,
  not the newly built app's key. The separate source lets a transition app carry a
  new key while the installed apps and the publication check still trust the old
  signing key. Missing, malformed or wrong release trust MUST prevent publication,
  with no fallback to the app's key. The release workflow MUST NOT override that
  key or bypass the signature, signed-version, build-hash or release-address
  checks. Only the merge checks use the throwaway-key override; the rotation
  procedure is in [Updates](../marshal/updates.md#the-update-feed-key). The feed
  check and its tests exist; the release run does not use them until W4's last pull
  request adds the feed to it. That pull request also adds the release shape check's
  exact-text guard for this rule; until then `--public-key` exists only for the merge
  checks' throwaway-key round trip, and no release workflow step calls the feed check.
- **Only newer, only from this repository.** The app takes a release only when its
  version is newer than its own, and only when the download address is `https`, on
  `github.com` with no port or credentials, under
  `/axross/runners/releases/download/`; redirects from it are followed, since the
  signature is what makes the bytes trusted. No `dangerous` option is set and
  downgrades are off, which a test of the configuration shows.
- **Bounded requests.** A check gives up after 30 s and a download after 10 minutes.
  Neither uses the GitHub REST API or a token.
- **The installer is started by the app.** The app starts `msiexec` only after the
  agent it asked to drain has ended and its process has exited, so the agent's Job
  Object cannot end the installer, and nothing installs from another client's drain.
- **The record across an update holds no secret.** It is a version and a time in
  Marshal's local data folder, applies for 30 minutes to that version only, and every
  launch removes it.

Residual risks, accepted:

- Anyone who can publish a release here and holds the update-feed key can ship code
  every installed app runs. The key goes in the `release` Environment, which admits
  `main` only and needs the maintainer's approval for each run; its offline backup,
  rotation and recovery are in [Updates](../marshal/updates.md#the-update-feed-key).
- Once `sign-update` is in the release run, the pinned `tauri-cli` and its locked
  dependencies run beside the key. The committed archive checksum and the binary's
  hash between the jobs fix what runs, not whether it is trustworthy; the roadmap
  accepts that, with the network cut during signing and the Environment's per-run
  approval. The advisories in the archive's lockfile that the maintainer accepted are
  listed in [`app/tauri-cli.deny.toml`](../../app/tauri-cli.deny.toml); every one is in
  a part of `tauri-cli` that `signer sign` does not use, and a new advisory stops
  `update-tool`.
- Tauri's updater keeps each downloaded MSI in the temporary folder
  (tauri-apps/plugins-workspace#2389); Windows' storage cleanup removes it.
