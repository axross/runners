---
type: Specification
title: Release version selection
description: Determine the version from tags and commit subjects, including the fixed-source recovery release.
---

# Release version selection

## Version Rules

semantic-release reads the subject of each commit on main since the last `v*` tag
with the `conventionalcommits` preset. A pull request's squash-merge title becomes
that subject, so the title decides the release:

- `feat` releases a minor version.
- `fix`, `perf`, and `revert` release a patch.
- A breaking marker, `!` after the type or a `BREAKING CHANGE:` footer, releases a
  major version. On `0.x` it releases `1.0.0`, not the next minor. The base tag
  `v0.1.1`, below, keeps the F0 recovery release on `0.x`.
- `build(deps)` releases a patch, through the `releaseRules` in
  [`release/.releaserc.json`](../../release/.releaserc.json). That is the subject
  Dependabot writes for the runner image, the CLI's crates and the toolchain, so
  such a bump raises the release version. During CLI recovery, the fixed
  historical program excludes main's dependency updates. Release notes list the
  bump under Dependencies through the notes generator's `presetConfig.types` in
  the same file. Dependabot's
  entry for the app's crates writes the same subject, so until the release carries
  the app, a bump of a crate only the app links releases a patch that changes
  nothing in the released `marshal`.
- Every other subject releases nothing. That includes `build(deps-dev)`, since
  dev-dependencies are not in the binary, though such a bump can still change a
  crate the binary shares in `Cargo.lock`; the CLI has no dev-dependencies today.
  It also includes Dependabot's `ci(deps)` action bumps and its `chore(deps-dev)`
  bumps of `release/`.

The repository has one version, so a `feat` or `fix` of any scope, such as
`feat(host)`, raises the CLI's version. No check enforces titles, and a mistitled
squash merge releases the wrong bump or none, so correct a title before merging.
Releases are never prereleases, and nothing is committed back to main: the version
lives in the tag and the release.

The maintainer decided on 2026-11-20 to keep releases on `0.x`. Three
breaking-marked commits follow `v0.1.0`: #93 (`2b2393e`), #94 (`1adf7cb`) and #117
(`954d349`). Counted from `v0.1.0`, they would make the next release `1.0.0`. So
before the F0 recovery dispatch, the maintainer pushes `v0.1.1`, a bare tag with no
release, on `954d349`, as `v0.0.0` was pushed for the first release.
semantic-release takes the highest semver `v*` tag merged into main as the last
release, so it reads only the commits after `954d349`. They include the `feat`
commits #118 and #139, so unless a breaking change merges first, the F0 recovery
release is `0.2.0` from `0.1.1`. Nothing is released as `0.1.1`; the tag only
marks where counting starts. Deleting it before the recovery release is published
puts the three breaking commits back in the count. Afterwards it no longer affects
the version, and it stays as the record of where counting started.

The recovery release's notes list only commits after `954d349`. None of them is in
the historical tree that release ships, `dc8e1eb`, and #93 and #94, which are in
that tree, do not appear. The notes describe app and later work, not the contents
of the command-line files.

The breaking-marker rule is unchanged. A breaking change merged after `v0.1.1` makes
a later plan job print `1.0.0`, and until the recovery release is published, so can
a `v0.1.1` that is missing or on another commit. When a plan job prints a version
the maintainer did not expect, the maintainer cancels the run before approving
`sign-macos`, which is before any job pushes a tag, and decides then how to proceed.
A release rule that maps a breaking change to a minor, and a version input on the
dispatch, are not in place: the base tag changes no release configuration or
workflow, and the release configuration keeps the default mapping. How a later
release stays on `0.x` after another breaking change is the maintainer's decision
when a plan job prints `1.0.0`, not made now.

`release/package.json` pins `conventional-changelog-conventionalcommits` to the
`9.x` line. `@semantic-release/release-notes-generator` renders notes with
`conventional-changelog-writer` 8, and a `10.x` preset needs writer 9: with it,
the notes step fails, the plan job fails, and nothing is released. CI never runs
semantic-release, so it would not catch that. `.github/dependabot.yml` therefore
ignores the preset's `10.x` and later; remove that rule once the notes generator
moves to writer 9.
