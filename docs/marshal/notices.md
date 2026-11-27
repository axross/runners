---
type: Procedure
title: "Generating licence notices"
description: "How are the platform notices produced and verified before packaging?"
---

# Generating licence notices

## Licence Notices

The Windows installer will install a notices file, so the notices are checked in.
[`app/notices/`](../../app/notices) holds one file for each tree the app is built
for, `THIRD-PARTY-NOTICES-windows.txt` for `x86_64-pc-windows-msvc` and
`THIRD-PARTY-NOTICES-macos.txt` for `aarch64-apple-darwin`, and what writes them:

| File or folder              | Holds                                                                                                                                                                                                |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `generate.sh`               | `write` or `check`: runs `cargo about generate --frozen --fail` for each tree, checks its clarifications applied, fills in the copyright lines below, adds ring's part, then appends the files below |
| `about.toml`, `notices.hbs` | cargo-about's accepted licences, which are `deny.toml`'s allowlist and its two exceptions, and its template                                                                                          |
| `licences/`                 | The WebView2 loader's licence and the `webview2-rs` crates' licence, for the Windows file only                                                                                                       |

Each file lists every crate the executable links, and every procedural macro run
while it compiles, grouped by licence, with each MPL-2.0 crate's published source
named; then ring's part below, IBM Plex Sans, Radix Colors and, for Windows, the
WebView2 loader. `mise run check:notices` regenerates both files and
fails on any difference from the checked-in ones, so a lockfile change that adds,
drops or bumps a crate fails the Check job until `mise run notices:app` writes them
again and the change commits them. Each file's header names the cargo-about version,
so another version fails the check too.

`cargo-about` is a mise tool pinned in [`mise.toml`](../../mise.toml) and locked in
`mise.lock` like the others, installed from its release as a prebuilt binary, so the
notices need no compile in CI. It runs with `--frozen`, offline, so a crate that
ships no licence file gets the licence's standard text with its template line
`Copyright (c) <year> <copyright holders>`, and the notices never hold that line:
`generate.sh` refuses output with either placeholder, so `check:notices` and
`notices:app` fail on it. cargo-about also prints the line for some crates whose
licence file it does not read the copyright line from; `about.toml` clarifies those,
the `windows-rs` crates, `dpi`, `tauri-plugin-single-instance`,
`tauri-plugin-opener`, `tauri-plugin-updater` and `minisign-verify`, naming each
crate's own licence files with their SHA-256. It clarifies `ring` the same way, to its
`LICENSE-other-bits` (ISC) and `LICENSE-BoringSSL` (Apache-2.0), since cargo-about
would otherwise list each of its source files' licence headers as a text of its own.
ring's `LICENSE` says its new code is under the ISC text, that code from BoringSSL is
under the Apache-2.0 text or under ISC with the licence at the top of each file, and
that code from `once_cell` is under that polyfill's own `LICENSE-APACHE` and
`LICENSE-MIT`. So `generate.sh` adds a part for ring, from the source Cargo fetched:
its `LICENSE` in full, every distinct copyright line in its published source outside
its `tests` folder, `Portions Copyright` lines included, sorted, and the polyfill's MIT text.

cargo-about drops a clarification whose checksum no longer matches with only a
warning, so a bump that changes a clarified file would go unnoticed. `generate.sh`
looks for each clarified crate's known copyright line, held in the script as the
SHA-256 of the line without its surrounding blanks, among the lines of the licence
texts that name it (the line of each crate's clarified licence file: one for the
`windows-rs` crates, and one each for `dpi`, `tauri-plugin-single-instance`,
`tauri-plugin-opener`, `tauri-plugin-updater`, `minisign-verify` and `ring`), and
checks the crate appears under no more
texts than its clarification names files. It fails when either does not hold, or
when `about.toml` clarifies a crate it has no line for, until the clarification
names the new file.

Two groups ship no licence file, and `generate.sh` fills in their line itself:

- `webview2-com`, `webview2-com-macros` and `webview2-com-sys` come from one
  repository, `wravery/webview2-rs`. `licences/webview2-com-sys-LICENSE.txt` is its
  `LICENSE` at the git commit the 0.39.1 crate was published from (`edc2caf`,
  recorded in the crate's `.cargo_vcs_info.json`), and the three crates' notice takes
  its copyright line from that copy. `generate.sh` does so only while the copy's
  words, without that line, are the licence text cargo-about printed for them, and
  fails otherwise.
- The `objc2` crates, from `madsmtm/objc2`. The versions in `app/Cargo.lock` ship no
  licence file, and the repository's `LICENSE.md` at the commits they were published
  from (in each crate's `.cargo_vcs_info.json`) names no copyright holder; a later
  licence there, from commit `ee9a7ad`, names one, and applies once a release carries
  it. Their notice is the licence text without the template line, after one sentence
  saying so. `generate.sh` lists the eleven name and version pairs checked, so any
  other version of these crates keeps the template line and fails the run until its
  licence is checked again.

A crate outside these groups that gets the template line fails the run.

The Windows file's part for the WebView2 loader repeats `webview2-com-sys`'s notice
from the same copy. The loader's text is Microsoft's `LICENSE.txt` from the NuGet
package `Microsoft.Web.WebView2` 1.0.3800.47, which is the SDK that
`webview2-com-sys` 0.39.1 bundles; it was identified by matching the SHA-256 of
`x64/WebView2LoaderStatic.lib` across NuGet releases. `generate.sh` fails when any
licence file it appends, these two included, is missing or empty, so a notice cannot
ship as a heading over nothing. When `webview2-com-sys` is
bumped, compare the new bundled loader's SHA-256 with the NuGet releases the same
way and, if the SDK changed, replace the loader's licence text and the version in
`generate.sh` in the same commit.
