#!/usr/bin/env bash
# Runs check-release-tag.sh against a temporary git repository: a matching tag on a
# commit at the default branch's tip, a matching tag on an older commit of that
# branch, a tag that does not match the version, a prerelease tag that matches its
# version, and a commit off the default branch.
# Each case checks the exit status and the cause the script names.
set -euo pipefail

check="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/check-release-tag.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
repo="${work}/repository"

# The user's own git configuration must not sign, hook or rename anything here.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid

commit_version() {
  mkdir -p "${repo}/cli"
  printf '[workspace]\nmembers = ["."]\n\n[package]\nname = "example"\nversion = "%s"\n' "$1" >"${repo}/cli/Cargo.toml"
  git -C "$repo" add cli/Cargo.toml
  git -C "$repo" commit --quiet --allow-empty --message "$2"
  git -C "$repo" rev-parse HEAD
}

git init --quiet --initial-branch=main "$repo"
older="$(commit_version 1.2.3 'older commit on main')"
tip="$(commit_version 1.2.3 'tip of main')"
git -C "$repo" switch --quiet --create side "$older"
off_main="$(commit_version 1.2.3 'commit off main')"
git -C "$repo" switch --quiet --create candidate "$tip"
prerelease="$(commit_version 0.2.0-rc.1 'prerelease version')"
git -C "$repo" switch --quiet main

failures=0

# expect <case> <exit status> <text the output must contain> <check arguments...>
expect() {
  local name="$1" want_status="$2" want_text="$3" output status=0
  shift 3
  output="$(cd "$repo" && bash "$check" "$@" 2>&1)" || status=$?
  if [[ $status -ne $want_status ]]; then
    echo "FAIL ${name}: exit ${status}, expected ${want_status}: ${output}" >&2
    failures=$((failures + 1))
  elif [[ "$output" != *"$want_text"* ]]; then
    echo "FAIL ${name}: the output does not contain '${want_text}': ${output}" >&2
    failures=$((failures + 1))
  else
    echo "ok   ${name}"
  fi
}

expect "a matching tag at the default branch's tip" 0 "matches cli/Cargo.toml's version" \
  v1.2.3 "$tip" refs/heads/main
expect "a matching tag on an older commit of the default branch" 0 "is on refs/heads/main" \
  v1.2.3 "$older" refs/heads/main
expect "a tag that does not match the version" 1 "the tag must be v1.2.3" \
  v1.2.4 "$tip" refs/heads/main
expect "a commit off the default branch" 1 "is not on the default branch" \
  v1.2.3 "$off_main" refs/heads/main
expect "a prerelease tag that matches its version" 1 "is not vMAJOR.MINOR.PATCH" \
  v0.2.0-rc.1 "$prerelease" refs/heads/candidate

if [[ $failures -ne 0 ]]; then
  echo "${failures} release tag check case(s) failed." >&2
  exit 1
fi
echo "All release tag check cases passed."
