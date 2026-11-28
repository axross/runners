---
type: Specification
title: "Lifecycle settings in the window"
description: "Which controls can the operator change and when do they take effect?"
---

# Lifecycle settings in the window

## The Settings Tab

W3 ([#82](https://github.com/axross/runners/issues/82)) owns this section. The tab
list reads "Pools | Tokens | Settings", and the Settings tab shows the five
`[lifecycle]` settings of [The Agent's Signals](../marshal-cli/agent-lifecycle.md#the-agents-signals) in the
design chosen on #82: two groups, "Starting Marshal" (`start_at_login`,
`start_runners_on_launch`) and "Sleep and network" (`keep_awake`,
`stop_on_network_change`, `resume_after_stop`), each setting a checkbox with its label
as a `<label for>`, so the text toggles it too. W1's banners lead the tab as they lead
the others, and the window never opens on it.

The tab is not the configuration editor's Host Settings. Host Settings holds the
routing label, the image name and, on a Mac, the budgets; the five `[lifecycle]`
settings have no field there.

- **State.** Each setting's description ends with its state against its Windows
  default, read from the snapshot's `lifecycle` part: "On (default).", or
  "Off (default: on)." when it differs. A setting the file sets to its default reads
  "(default)" too. A value the agent does not report, because `config.toml` cannot be
  read or a problem names that key, shows the checkbox neither on nor off.
- **A change** calls `set_setting`, which sends `lifecycle_set` with the key and the
  new value; the app never writes the file. While it is out every checkbox is
  unavailable, the changed one keeps the value chosen and reads "Being saved as
  off.", and its answer is said through the live region without moving focus:
  "Saved: "Keep the computer awake while runners run" is off.", or the refusal, which
  also stays under the setting until Start All, a new connection or a change in the
  snapshot's `lifecycle` part (its settings, the sign-in item, keep-awake or a
  resume); a saved value holds the checkbox until the snapshot shows it.
  `config_changed` says the file changed since Marshal read it, that the edits in the
  file were kept and Start All reads them, after Stop All while the runners run,
  before changing the setting again; `busy` says to change it again. Any other refusal
  gives the agent's words after "Not saved:", with `config_invalid`'s problems as the
  footer lists them, and a next step: `config_unwritable` says to close any program
  holding the file, `config_invalid` and `config_unusable` say to fix the file for Start
  All, and `config_file_form` or a refusal this version does not know says the setting
  can be changed in the file instead. A change the agent does not answer in 10 s
  says it may still be saved, and the checkbox follows the snapshot.
- **Unavailable.** While Marshal has lost contact, is quitting, does not know the
  agent's state, `config.toml` is missing, unreadable or has problems, a setting is
  being saved, or the agent reports no `lifecycle` part, every checkbox
  reports itself disabled, stays focusable, does not toggle and takes the neutral
  colour (`GrayText` in a Windows contrast theme), and the reason is part of each
  checkbox's description and shows in its balloon on hover and focus.
- **Notes** under a setting, part of its description: Windows' Startup apps keeping
  Marshal from starting at sign-in, naming "Windows Settings > Apps > Startup"; a
  failed Run key write, with Windows' words; the keep-awake warning; a resume waiting
  for the network the runners stopped on, or not armed for want of a gateway hardware
  address; and resuming too often.
- **"Needs attention"** marks the tab while something there needs the user: Startup
  apps blocking Marshal, a refused change, or `config.toml` with problems. A note that
  only informs, such as a waiting resume, does not.

The refusal for a changed file departs from the design mock's words, which said the
tab shows what the file says now: the agent keeps the settings in effect and does not
read the file again until Start All, so the window says that instead.

**Known limits** ([#164](https://github.com/axross/runners/issues/164)). A change
here rewrites `config.toml` through the agent, which moves the revision the
[configuration editor's](./configuration.md#the-configuration-editor) draft was made against. A draft
started before the change is then refused as changed elsewhere (`config_changed`),
and the editor's Read Again drops its unapplied changes, so apply or discard the
draft before changing a setting. The editor's Reload reads the file again but does
not put `[lifecycle]` changes in effect; only Start All does. After a hand edit of
`[lifecycle]`, choose Start All, not Reload.

`app/crates/view/tests/view/settings.rs` covers the states, defaults, notes, the chip
and each unavailable case from recorded snapshots,
`app/crates/session/tests/session/settings.rs` the call, the wait and a refusal, and
`tests/frontend/settings.test.mjs` that an unavailable or unknown checkbox does not
toggle and carries its reason and note in its description. Narrator, the contrast
theme on a real Windows desktop and the keyboard-only use of the tab are W3's
out-of-tree check on axpc.

## The Settings Tab on the Mac

M4 ([#90](https://github.com/axross/runners/issues/90)) owns this section: W3's tab
in the Mac wording approved on #90, chosen by `Input::macos`, which the session sets
where it keeps a login item. Windows is unchanged.

- **The network row is fixed.** It is checked, whatever `config.toml` holds for
  `stop_on_network_change`, reports itself disabled, stays focusable, does not toggle
  and is never dimmed; "macOS always stops the runners when the primary
  network changes." shows in W3's balloon and stays on the page as hidden text in
  the checkbox's description. The page never sends `lifecycle_set` for it.
- **"Login Items: <state>"** sits under the first setting: On, Waiting for approval,
  Removed from Open at Login, Could not be added (code N), Could not be removed (code
  N), or Off, as the latest bringing in step, or reading when the window is shown, left
  it. While the setting is on and the item is not, or the setting is off and the item
  is still on, waiting for approval or not removed, W3's note and an Open Login Items
  Settings button follow; the button calls `open_login_items`, which opens Login Items
  & Extensions. With the setting off, the note says the item is still in Open at Login
  and to turn it off there; after a failed removal it also says Marshal tries again.
  Approval and removal with the setting on mark the tab "Needs attention"; a failure to
  add or remove is an error note without the chip, and an item still there with the
  setting off is a warning note without it. Each change after the first is said as "Login Items: <state>.". A
  copy outside Applications shows no line.
  [Login item on the Mac](./session.md#the-agent-and-the-apps-session) says when the
  app registers and unregisters the item.
- **A login launch shows only the menu bar icon.** The setting's description says
  "Marshal opens with only its icon in the menu bar." and the app keeps it: the launch
  is recognised by the open-application Apple Event's login-item mark, and the window
  does not open. When macOS gives no mark, the launch opens the window like any launch.
  The mark is confirmed on a real Mac at the overhaul's final verification; until
  then the behaviour is unverified, and
  [the session](./session.md#the-agent-and-the-apps-session) says how the app reads it.
- **Increase Contrast** (`prefers-contrast: more`, under `data-platform="macos"`)
  gives tabs and buttons a text-colour border, dividers and lists the text colour, and
  the selected tab a 2 px accent-text border.

The view, session and page tests cover each state; the approval and removal paths
stay provisional until M4's Mac run.
