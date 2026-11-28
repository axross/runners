---
type: Specification
title: "Marshal configuration and pool editor"
description: "How do token entry, file diagnostics and applying or discarding a pool edit work?"
---

# Marshal configuration and pool editor

## Configuration and Tokens

The agent reads the TOML configuration from `%LOCALAPPDATA%\marshal\config.toml` on
Windows and `~/Library/Application Support/marshal/config.toml` on macOS, which is
also the macOS control socket's folder, so the agent keeps it private (0700).
[The Configuration File](#the-configuration-file) says what the window shows of it,
and [The Agent's Configuration](../marshal-cli/resident-agent.md#the-agents-configuration) how the agent
loads it. The agent reads each pool's token
from the token store when it admits the pool and at each registration, never from a
file: it ignores the configuration's `tokenPath`, which the shared validation still
requires of a JSON configuration and the TOML format does not have. On Windows a token is a Credential Manager entry
for the repository, and on macOS a login-keychain item, with the names
[The Token Store](../marshal-cli/token-store.md#the-token-store) gives; either is entered in the main
window's Tokens tab, and the app never shows a stored token and never reads the store
itself.

When the configuration names organisation pools, the Tokens tab separates
Repositories from Organisations, with headings above the lists. An organisation
row uses the organisation's lower-case login and names every pool sharing its
token. Set Token on a skipped pool's card selects that row; its form and removal
question work like a repository's. The card names the organisation and gives the
runner group its own line under the name. Without an organisation pool, the tab
keeps its single repository list without section headings.

Today's token files are not imported. A host that moves from the command line to the
app enters each repository's token in the window, once.

## The Configuration File

K1 ([#77](https://github.com/axross/runners/issues/77)) owns this section. The agent
runs `config.toml`, migrates a `config.json` it finds alone at its start, and keeps the
last valid configuration in use when a Start All finds the file broken or gone, or a
Reload finds it broken (a Reload of a missing file is refused as `config_unusable`).
The window takes all of it from the snapshot's `configuration`: `path`, `problems`,
`status`, `in_use` and `migration`, as [The Snapshot](../marshal-cli/agent-protocol.md#the-snapshot) lists
them. It adds no state, control or banner kind; the wording lives in the
`marshal-view` crate.

- **The footer** names `config.toml` and, after a migration, says when this agent
  migrated `config.json` and that `config.json` is no longer read.
- **A footer warning** stands beside the image warnings while the agent holds a
  configuration that is not the file's: the last valid one, naming the file's problems
  (or that it is missing) and when the configuration in use was read; or `config.json`'s,
  after a migration that could not write `config.toml`, with why. The warning for the
  last valid configuration, and the one for `config.json`'s when `config.toml` has
  appeared since, end with the next step below; the one for a migration that could not
  write `config.toml` says instead that Marshal tries again when it next starts. A
  migration still waiting to happen is a warning of its own: quit Marshal and launch it
  again.
- **The stopped banners** keep their roles. With no configuration the banner names
  `config.toml`, and says to launch Marshal again when `config.json` waits to be
  migrated. When `config.json` broke the rules at its migration, the invalid
  configuration banner names `config.json`, its problems in the JSON wording, and that
  a fix takes effect when Marshal starts again. The other banners for a stop for the
  file end with the next step below.
- **The next step.** Every place that names the step after `config.toml` is fixed, or
  the step that makes Marshal read an edit made in the file, names the same one: the
  footer's warnings, the [Settings tab's](./settings.md#the-settings-tab) introduction,
  unavailable reason and notes, the announcement and log line of a refused setting, and
  the banners of a stop for the file. While a migration of `config.json` is pending, or
  `config.json` was rejected at its migration, it is to quit Marshal and launch it
  again. Otherwise, while the runners run or resume, it is "Reload config.toml" when the
  agent offers the editor and no migration failed, and "Stop All and Start All" when it
  does not or a migration failed. In every other state, building, stopping and quitting
  included, it is "Start All". The editor's own "take effect at the next Start All"
  results after a write on a stopped agent are not this step: they describe pool and
  host changes, which wait for Start All.

Build needs a configuration in use: it uses that configuration's image name and reads
no file. `app/crates/view/tests/view/configuration.rs` covers each wording, and the
recorded snapshots name `config.toml`.

**Rolling back.** A release without K1 reads `config.json` as it was when it was
migrated; changes made in `config.toml` since then are not carried back. After rolling
forward again, `config.toml` is read as it was, and edits made to `config.json` in the
meantime are ignored while `config.toml` exists. Delete `config.toml` to migrate
`config.json` again at the next start.

## The Configuration Editor

K3 adds Edit to each pool card. A card's form replaces its runner rows, not the
running pool: the draft changes nothing until Apply. Host Settings opens the
host-wide fields in the footer. A file the editor has not read, or whose read a
change on the Settings tab has made stale, is read first: Edit, Add Pool and Host
Settings read `config.toml` again, with the read's busy state and announcement, and
open the form only once the read has found the file editable; an uneditable file
keeps this control unavailable with its reason. The profile selects the fields; the page offers
neither `schema_version` nor `[lifecycle]`. An unset value stays unset unless edited,
so displaying a default never adds it to the file. The page sends only changed
keys and array-entry operations, leaving comments and layout to the agent's write
path.

The pool fields are those of a repository pool. An organisation pool's
`organization` and `runner_group` keys have no field, so the form neither shows nor
changes them, and its Repository field reads as not set; its other fields edit as
any pool's. The agent's check refuses a repository on it, since a pool has one
target, and refuses volumes on it. A new pool is added with a repository. Set an
organisation pool's target in a text editor. Removing a pool offers to remove the
token of a repository no remaining pool names, never an organisation's. Remove an
organisation's token from the [Tokens tab](#configuration-and-tokens) before removing
the last pool that names it; after that, the tab has no row for it.

An on-demand pool's `scale_set` key has no field either, so the form neither shows
nor changes it; its other fields edit as any repository pool's. The agent's check
refuses a non-empty `labels` on it, since its scale-set name is its only label. Add,
change or remove `scale_set` in a text editor.

`config-editor.js` keeps the unapplied draft across view updates and window hides.
`editor.js` draws its fields, problems, change bar and in-list review. Apply first
asks the agent to check, then lists the agent's effects for every pool. A change
that stops runners names the pools and warns that their running jobs fail. Cancel
gets focus; Esc cancels and returns focus to the opener. Confirmation sends the
same operations at the original file revision, never at the check's preview
revision. A transition, lost contact or a different running/stopped state drops
the review and keeps the draft. Confirmation repeats the non-writing check; if
the effects changed, it shows the new review instead of writing. The agent
repeats validation before writing. A successful or refused confirmation keeps the
same logical focus on Done, with a saved or not-applied result. This also holds
while token removal or the final read finishes after a successful write.
Done or Esc closes the result and moves focus to the heading; starting another
edit or read also clears the result. A confirmed write the agent does not answer
in time, or whose connection ends before it answers, may still have been applied,
so it is reported as "Configuration may have been saved" rather than not applied.
The notice asks to read config.toml again before editing, and the draft stays.

An edit refused by the agent keeps the draft and shows structured problems beside
their fields, with the key and line as accessible descriptions. A problem with no
matching field requires a text editor. A value omitted because its type is wrong
has an explicit Remove Invalid Value action, so an optional key can be unset
without supplying its default. Locked fields keep their label and problem
description and explain the lock on hover and keyboard focus. Read Again asks before dropping a draft.
Reload reads the file now and uses the same check and confirmation flow. It does
not watch or poll the file. Availability comes from the negotiated methods and
the agent's snapshot; a transition, lost contact, migration, update drain,
pending apply, runtime restart or a setting being saved explains why Apply and
Reload are unavailable.

An applied Reload or Edit also puts the file's `[lifecycle]` settings in effect,
whether the runners run or are stopped, as Start All does, so the Settings tab shows
the file's values. On Windows the sign-in entry follows `start_at_login`, keep-awake,
the network stop and the resume follow the settings from then on, and a waiting
resume is cancelled when `resume_after_stop` is now off. A check, a refusal and a
failure change no setting in effect. On a Mac, which has no `[lifecycle]`, nothing
changes. When the written change was made on an agent stopped because
`config.toml` was missing or invalid, the agent becomes stopped by you, as after a
successful Build: the window drops its configuration banner for the stopped-by-you
one, Build is available and the Settings tab follows the file. The runners stay
stopped until Start All. A `config.json` rejected at its migration keeps its stop,
because the agent refuses Reload and Edit then. After a written or applied Edit or
Reload the window also removes the refusal, lost-contact and unanswered notes under
the settings, which that read has made obsolete.

**The Settings tab and the editor.** Both change `config.toml`, so the page keeps
their changes apart. While the editor shows a form, holds unapplied changes, shows a
review or a question about dropping changes, or has a read, check or apply in flight,
every Settings checkbox is unavailable, with the reason
[The Settings Tab](./settings.md#the-settings-tab) lists. From the moment a checkbox
sends a change until its call returns, the editor's controls that open a form, read,
check, apply or reload are unavailable, with "Unavailable while a setting is being
saved." After the call returns, whatever came of it, the editor's read is stale until
a read that began afterwards completes; a read already in flight when the call
returns leaves it stale. The next form therefore reads first, and a draft made in it
is checked against the file as the setting left it, so it needs no Read Again.

**Remaining limit.** The window does not watch the file. A hand edit made while a
draft is open, or a setting change the agent saves after the 10 s the window waits
for its answer, moves the file's revision, so the agent refuses that draft as changed
elsewhere (`config_changed`). Read Again reads the file anew and still asks before it
drops the draft.

The review may offer to remove the tokens of repositories no remaining pool uses.
Keeping them is the default. Removal uses the existing `token_remove` method only
after the configuration write succeeds. Its result travels separately through
`configuration` so a store refusal can show its cause and offer Try Again even
after the repository has left the Tokens tab. A failed removal never undoes the
saved configuration, and retrying it never reapplies the edit. A removal the agent
does not answer, or whose connection ends first, is neither removed nor failed: its
row says the agent has not answered, that the token may still be stored, and offers
no retry. No stored token is read or displayed. A successful retry keeps its
focusable Removed control and does not offer another removal. A later read
cancels an old removal choice when any pool names that repository again,
ignoring letter case. The focusable
Cancelled result keeps the same control key and cannot retry; removing the pool
again does not restore the old choice. A retry waits until the configuration
check and review finish, and locks configuration actions until its answer arrives.
A successful write followed by a failed read is reported as saved, with a request
to read again, not as an unapplied draft. Retained token-removal retries are
unavailable until the configuration can be read and edited again.

The page tests exercise minimal edits, source revisions, refusal retention,
discard confirmation, optional token removal and retry, labelled problems,
draft persistence across views and hides, and Cancel/Esc focus. They also exercise
the Settings checkboxes' reasons and their precedence, the editor's lock while a
setting is being saved, and the read before a form after a setting change
(`tests/frontend/settings-editor-page.test.mjs`). These Node tests
use a document stand-in: they establish neither rendered layout nor native web
view or screen-reader behaviour.

`mise run test:app-render` renders the bundled page in the pinned Chromium on
Linux x64. [Development Setup](../operations/development-setup.md#rendering-gate) says how to
prepare it, [Testing](../conventions/testing.md) where it runs, and
[the dependency pinning convention](../security/dependency-pins.md#pinning-convention) how its controller and browser are
pinned. It drives `playwright-core`'s browser API, not the Playwright test runner
or Puppeteer, and writes its screenshots and measurements under
`app/target/render/output/`.

The renderer uses `dump_view --editor-profile axpc|axmc` over recorded running,
stopped, transition and lost-contact snapshots, and a fake Tauri command bridge
with shaped protocol replies, not captured native command executions.
For the Settings tab it uses `dump_view --editor-profile axpc --agent-offers-editor`
over `running_lifecycle.json`, the one recorded snapshot with a `lifecycle` part.
The bridge retains pool membership edits in memory for read-back, not on disk;
it does not reproduce other field writes or live runner effects.
It covers the editor states in both themes, with and without forced colours, at
880 and 480 px. It also renders the Settings tab on the axpc profile, in the same
themes and widths, while the editor holds unapplied changes: each setting's
checkbox stays focusable, does not toggle, is reported disabled in Chromium's
accessibility tree, and carries the editing reason in its accessible description
and balloon.
It measures horizontal overflow, the scroll/footer boundary, a balloon cut off by
the scrolling area, text contrast (4.5:1), field boundaries and keyboard focus
(3:1), and Chromium's accessibility tree for field names, problem/reason
descriptions and pool headings.
Keyboard checks retain focus through held writes, confirmation refusals, token
removal retries and failed read-back. They keep uneditable Host Settings inert
with an explained reason. Pool heading expectations come from fixture names,
including pools marked for removal, not from the headings already in the DOM.
Deliberate overflow, low-contrast, broken-name, missing-heading and
missing-reason mutations must fail those checks. The job retains screenshots and
measurements for 14 days. This is browser evidence, not a native Windows or Mac
app, IPC, contrast-theme or screen-reader run; those stay manual checks on a real
host.
