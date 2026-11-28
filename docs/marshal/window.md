---
type: Specification
title: "Window and notification-area presentation"
description: "What do the window, menu icons, fonts and colours show?"
---

# Window and notification-area presentation

## The Window's Page

The windows load only bundled files from
[`app/crates/app/frontend/`](../../app/crates/app/frontend), with no npm package, as
ES modules, with no inline script, no `style` attribute and no `eval`. The content
security policy is in [`tauri.conf.json`](../../app/crates/app/tauri.conf.json) and
[Security](../security/app-trust.md#the-app-surface) states it. What the window and
menu show for a snapshot is computed in the `marshal-view` crate, which has no
operating system dependency; the page only draws the view it is given and sets every
string through `textContent`.

A runner row writes its state out beside a mark: "Container running" with a check,
"Saving diagnostics" with a check while the runner's diagnostic container is
finalising, "Registering" with a ring, "Backing off" and "Held" with the history mark,
and "Stopped" with a square. A backing-off or held runner shows the agent's reason under
its state, and a held one is a runner whose job host a host prerequisite refuses, so
its reason says which. A state the app does not know shows the runner's own reason as
its words, with a dash, or the state's word when there is no reason, as the version
rules in [CLI](../marshal-cli/agent-protocol.md#version-rules) require.

The agent gives a skipped pool's reason after its place in the configuration, as
`pool[0]: no token.` (`repositories[0]` from an agent that read `config.json`), and the view
reads the cause after that place. A pool
skipped for want of a token says "No token for owner/app. Start All skipped this
pool." with Set Token. A pool skipped because the token store failed, or because there
is none, keeps the agent's reason on its card, and its repository's row shows the
store's words, such as "The token store failed: access was refused (kind
access_refused, code 5).", instead of "No token", with no "No repository has a token
yet" banner; Set Token stays, since a save goes through the same store and says if it
fails too. A token save or removal the agent has not answered says so, and the row
then follows the snapshot, since the agent may still do it.

| File                                                | Holds                                                                                                                                  |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `index.html`, `main.js`, `revision.js`              | The page, the first `read_view`, the `marshal:view`, `marshal:confirm` and `marshal:hidden` listeners; the order views are drawn in    |
| `render.js`, `pools.js`, `tokens.js`, `settings.js` | The header, tabs and footer; the pool cards, runner rows and diagnostics; the token rows; the settings                                 |
| `controls.js`, `tooltip.js`, `dialog.js`, `live.js` | Buttons that stay focusable when unavailable; the balloon; the confirmation; the live region                                           |
| `dom.js`, `icons.js`                                | The element builder, which refuses `style`, `href`, `src`, `srcdoc`, `action`, `formaction` and `on*` attributes, and the inline icons |
| `main.css`, `tokens.css`, `fonts.css`               | The window stylesheet; the colour tokens; the one place that names the font files                                                      |
| `confirmation.html`, `confirmation.js`              | The confirmation window's page, with Cancel focused                                                                                    |
| `config-editor.js`, `editor.js`                     | The editor's unapplied draft and its calls to the agent; its fields, review, notice, bar and footer                                    |

A balloon opens below its control, or above it near the bottom of the window. When
the scrolling area has room on neither side, a balloon opened by keyboard focus
scrolls the area just far enough to show it whole, keeping its control in view; one
opened by the pointer stays where it is, so nothing moves under the pointer.

Esc closes the first of these that is open and does nothing more: a tooltip balloon,
then a confirmation dialog, then the open token form or removal question the focus is
in, then the editor's Read Again or Reload question, which shows on every tab, then
the editor's review or saved result, which shows on the Pools tab only and so is
cancelled only while that tab is on show, and only then hides the window. The page
clears a token field as soon as it calls `set_token` or `set_organization_token`,
whatever the outcome, and keeps the token in no variable; `save-token.test.mjs` loads
the page over a stand-in for the document and drives that path with a call that
resolves and one that rejects. It also clears every open token field when the app
sends `marshal:hidden`, which it does at every place it hides the window, and when
the web view reports the page hidden, a second route; the form stays open and empty.

From `app/`, `cargo run -p marshal-view --example dump_view -- <fixture.json>` prints a
recorded snapshot's view as JSON, with options to set the connection, a pending
action, an open token form, a token result, the settled count, and a recorded `github`
value or an agent without one, which the example's header comment lists, so the pages
can be rendered outside the app.

### Fonts and Colours

IBM Plex Sans is bundled as three WOFF2 files under `frontend/fonts/`, so nothing
is downloaded at run time. [`fonts/README.md`](../../app/crates/app/frontend/fonts/README.md)
records the release, its source, the archive's SHA-256 and each file's, and says how
to check and update them; `.gitattributes` keeps the files byte for byte. The colour
values in `tokens.css` are copied from Radix Colors 3.0.0, and the file's header
records the package, its licence and the roles each theme fills; the package's MIT
notice is kept as `colors-LICENSE.txt` beside it. Both licences ship in the licence
[licence notices](./notices.md#licence-notices). A font or colour update changes the record in the same commit.

A token field has a 1 px border in `--border-neutral-unselected-control`, which
turns `--text-destructive-low` when the field is invalid; a Windows contrast theme
keeps its own system colours. The ratios, by the WCAG
formula, against the card (`--list-card-fill`) and the field's fill
(`--component-neutral-rest`) are recorded here, all above the 3:1 a control's
boundary needs; a change to these tokens measures them again:

| Theme | State   | Border    | Against the card | Against the fill |
| ----- | ------- | --------- | ---------------- | ---------------- |
| Light | Rest    | `#80838d` | 3.78:1           | 3.33:1           |
| Light | Invalid | `#ca244d` | 5.43:1           | 4.77:1           |
| Dark  | Rest    | `#696e77` | 3.43:1           | 3.10:1           |
| Dark  | Invalid | `#ff949d` | 8.36:1           | 7.56:1           |

## The Icons

The notification-area icon has five states, all up, some not up, stopped, stopped by
an error and Marshal not in control of the runners, each in white for a dark taskbar
and in near-black for a light one, at 16, 20, 24, 28 and 32 px. The images are
masks in text, [`white.txt`](../../app/crates/tray/glyphs/white.txt) and
[`near-black.txt`](../../app/crates/tray/glyphs/near-black.txt) under
`app/crates/tray/glyphs/`, embedded with `include_str!` and turned into RGBA when
used, so the app links no image decoder. A file holds 25 blocks, one for each state
and size: a line `<state> <size>`, then `size` rows of `size` characters, where a hex
digit is a pixel's alpha in fifteenths (`f` is opaque) and `.` is none. The shapes were laid out for
each size on its own pixel grid, not scaled, and the white set is drawn a little
lighter than the near-black set because white on a dark ground looks heavier.

To change a glyph, edit the text in both files and run `mise run test:app`. The
tests in `app/crates/tray/tests/glyphs.rs` fail on a missing or malformed block, an
edge pixel, a mostly blank image or two states whose glyphs of one colour differ in
fewer than 12 whole pixels at any size, which is how the five states stay apart. The app polls the
taskbar's mode and the primary display's scaling once a second and replaces the
icon's image only when one changes. An unreadable or absent mode counts as dark, and
is logged once. A change is put on the icon from the event loop's thread, which reads
the newest look decided when it runs (`Watch::current`) and not the one that queued
it, so two looks decided on two threads and queued out of order leave the newer one
on the icon.

Two choices go beyond the design record and are the maintainer's to confirm. The
record draws stopped by an error as a red disc with an exclamation mark; the glyph
is an exclamation mark inside the outline frame in the glyph's own colour, because
the images have two colours only and one mask format. The app icon is the record's
Iris tile with three lines, with 20 and 40 px sizes added, not an outline glyph.

The app icon's source is
[`app-icon.txt`](../../app/crates/tray/glyphs/app-icon.txt). From `app/`,
`cargo run -p marshal-tray --example icons -- app-icon crates/app/icons` writes
`icon.ico` and `icon.png`, at 16, 20, 24, 32, 40 and 48 px, and
`cargo run -p marshal-tray --example icons -- sheet <file.png>` writes the icon
sheet: every state at 16 to 32 px in both colours on a light and a dark taskbar, two
mismatched pairings, and the app icon. The files are written without compression so
the output is the same every time, and `tests/app_icon.rs` fails if the checked-in
icon files differ from what the source produces. The maintainer approves the sheet
before the app's pull request is ready; the approval is recorded on the tracking
issue, not here.
