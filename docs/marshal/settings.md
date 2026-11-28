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
  also stays under the setting until Start All, a new connection, a change in the
  snapshot's `lifecycle` part (its settings, the sign-in item, keep-awake or a
  resume), or an Edit or Reload of the configuration editor that the agent answers as
  written or applied, which removes every refusal, lost-contact and unanswered note
  at once; a check, a refusal, a failure or an Edit or Reload the agent does not
  answer in time removes nothing. A saved value holds the checkbox until the snapshot
  shows it. `config_changed` says the file changed since Marshal read it, that the
  edits in the file were kept and names [the next step](./configuration.md#the-configuration-file)
  to read them, before changing the setting again; `busy` says to change it again.
  Any other refusal gives the agent's words after "Not saved:", with
  `config_invalid`'s problems as the footer lists them, and a next step:
  `config_unwritable` says to close any program holding the file, `config_invalid`
  and `config_unusable` say to fix the file, then take the next step, and
  `config_file_form` or a refusal this version does not know says the setting can be
  changed in the file instead, then to take the next step to read it. The
  announcement and the log line repeat the note with the agent's state when it
  answered. A change the agent does not answer in 10 s says it may still be saved, and
  the checkbox follows the snapshot.
- **Unavailable.** While Marshal has lost contact, is quitting, does not know the
  agent's state, `config.toml` is missing, unreadable or has problems, a setting is
  being saved, or the agent reports no `lifecycle` part, every checkbox
  reports itself disabled, stays focusable, does not toggle and takes the neutral
  colour (`GrayText` in a Windows contrast theme), and the reason is part of each
  checkbox's description and shows in its balloon on hover and focus. The reason for
  a file with problems ends with [the next step](./configuration.md#the-configuration-file).
  When the agent offers the [configuration editor](./configuration.md#the-configuration-editor)
  and none of those reasons holds, the editor's state makes the checkboxes unavailable
  in the same way, with the reason that fits, in this order: while it reads, checks or
  applies, "Settings cannot be changed while Marshal is reading, checking or applying
  the configuration."; while a change is in review, "Settings cannot be changed while
  a configuration change is being reviewed. Finish or cancel the review on the Pools
  tab first."; and while it shows a pool or Host Settings form, holds unapplied
  changes or asks about dropping them, "Settings cannot be changed while the
  configuration is open for editing. Apply, discard or close your changes first." A
  form the editor holds but hides, because it reports the file unusable, is not shown,
  and its unapplied changes still make the checkboxes unavailable until the editor's
  Discard ends them. The checkboxes work again as soon as the cause ends. With no
  editor offered, the tab behaves as the other reasons say.
- **Notes** under a setting, part of its description: Windows' Startup apps keeping
  Marshal from starting at sign-in, naming "Windows Settings > Apps > Startup"; a
  failed Run key write, with Windows' words; the keep-awake warning; a resume waiting
  for the network the runners stopped on, or not armed for want of a gateway hardware
  address; and resuming too often.
- **"Needs attention"** marks the tab while something there needs the user: Startup
  apps blocking Marshal, a refused change, or `config.toml` with problems. A note that
  only informs, such as a waiting resume, does not.

The refusal for a changed file departs from the design mock's words, which said the
tab shows what the file says now: the agent keeps the settings in effect until it
reads the file again, so the window names the step that makes it read it.

**Reading an edit made in the file.** An edit made in the file takes effect at the
step [the configuration page](./configuration.md#the-configuration-file) names, which
the tab's introduction says. The agent reads `[lifecycle]` at launch and at Start All,
and also when the editor's Edit or Reload is written or applied, whether the runners
run or are stopped: the tab then shows the file's values, and keep-awake, the network
stop, the resume and, on Windows, the sign-in item follow them. This replaces W3's
([#82](https://github.com/axross/runners/issues/82)) rule that a hand-edited setting
takes effect only at the next launch or Start All. A check, a refusal and a failure
change no setting in effect. While a running agent checks a new image for an editor
change, a setting change is refused as `busy`, so a Reload cannot put out of effect a
setting saved meanwhile.

**The tab and the editor.** The tab's change rewrites `config.toml` through the agent,
which moves the revision a draft of the
[configuration editor](./configuration.md#the-configuration-editor) is made against.
The page therefore keeps them apart: the checkboxes wait for open editor work, the
editor waits while a setting is being saved ("Unavailable while a setting is being
saved."), and after a setting change the editor reads the file again before it opens
its next form, so a draft made then is not refused as changed elsewhere.

**Remaining limit.** A hand edit, or a setting change the agent saves after the 10 s
the window waits for it, while a draft is open still moves the revision, so the
agent refuses that draft as `config_changed`; Read Again, which still asks before it
drops the draft, reads the file anew.

`app/crates/view/tests/view/settings.rs` covers the states, defaults, notes, the chip
and each unavailable case from recorded snapshots, and
`app/crates/view/tests/view/next_step.rs` that the footer, the tab and the banners
name the same step in each state. `app/crates/session/tests/session/settings.rs` covers
the call, the wait, a refusal, the notes an applied Edit or Reload removes and the
step a refusal's announcement names. `tests/frontend/settings.test.mjs` covers that an
unavailable or unknown checkbox does not toggle and carries its reason and note in its
description, and `tests/frontend/settings-editor-page.test.mjs` the reasons the editor
adds, their precedence, the editor's wait for a setting and the read before a form.
The agent's side, `[lifecycle]` taken at an Edit or Reload and a setting refused
while an image is checked, is in `cli/tests/agent_lifecycle.rs` and
`cli/tests/agent_signals.rs`. Narrator, the contrast
theme on a real Windows desktop and the keyboard-only use of the tab are W3's
out-of-tree check on axpc.
