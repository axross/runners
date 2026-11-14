#!/usr/bin/env bash
# writes a release version over the placeholder in the CLI's and the app's
# Cargo.toml and Cargo.lock, in the runner only, so the app executable's product
# version is the release's; refuses unless it is X.Y.Z or X.Y.Z-<prerelease> and
# each file has the version lines it expects, all holding the placeholder. uses a
# temporary file, not `sed -i`, for CRLF checkouts. see docs/operations/cli.md.
set -euo pipefail

placeholder="0.0.0-dev"

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: set-release-version.sh <version> [<repository root>]" >&2
  exit 2
fi
version="$1"
root="${2:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

number='(0|[1-9][0-9]*)'
release_version="^${number}\.${number}\.${number}(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$"
if [[ ! "$version" =~ $release_version ]]; then
  echo "error: ${version} is not X.Y.Z or X.Y.Z-<prerelease>, with no leading zero and no build metadata" >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/set-release-version.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT

# each function copies its file to <copy> with the version written over the
# lines it selects, and prints how many lines it selected, then the names of the
# values that are not the placeholder, then the names it selected.

# manifest_version <file> <copy> <table>: the version line in the <table> table.
manifest_version() {
  awk -v version="$version" -v out="$2" -v table="$3" -v placeholder="$placeholder" '
    {
      line = $0
      cr = sub(/\r$/, "", line) ? "\r" : ""
    }
    line ~ /^[[:space:]]*\[/ { in_table = (line ~ ("^[[:space:]]*\\[" table "\\][[:space:]]*$")) }
    in_table && line ~ /^[[:space:]]*version[[:space:]]*=/ && match(line, /"[^"]*"/) {
      found++
      value = substr(line, RSTART + 1, RLENGTH - 2)
      if (value != placeholder) wrong = wrong " " value
      sub(/"[^"]*"/, "\"" version "\"", line)
    }
    { printf "%s%s\n", line, cr > out }
    END { print found + 0 "|" wrong "|" }
  ' "$1"
}

manifest_version_package() { manifest_version "$1" "$2" package; }
manifest_version_workspace() { manifest_version "$1" "$2" 'workspace\\.package'; }

# lockfile_version <file> <copy>: the version line of every package without a
# source, which are the workspace's own crates and its path dependencies.
lockfile_version() {
  awk -v version="$version" -v out="$2" -v placeholder="$placeholder" '
    function flush(   i) {
      for (i = 1; i <= count; i++) {
        if (i == version_at && !has_source) {
          found++
          names = names " " name
          if (value != placeholder) wrong = wrong " " name "=" value
          printf "version = \"%s\"%s\n", version, crs[i] > out
        } else {
          printf "%s%s\n", lines[i], crs[i] > out
        }
      }
      count = 0; version_at = 0; has_source = 0; name = ""; value = ""
    }
    {
      line = $0
      cr = sub(/\r$/, "", line) ? "\r" : ""
    }
    line ~ /^\[/ { flush() }
    { count++; lines[count] = line; crs[count] = cr }
    line ~ /^name = "/ { name = substr(line, 9, length(line) - 9) }
    line ~ /^version = "[^"]*"$/ && name != "" && !version_at { version_at = count; value = substr(line, 12, length(line) - 12) }
    line ~ /^source = / { has_source = 1 }
    END { flush(); print found + 0 "|" wrong "|" names }
  ' "$1"
}

failed=0
# check <file> <what> <required names> <command...>
check() {
  local file="$1" what="$2" required="$3" result count wrong names name
  shift 3
  if [[ ! -f "$file" ]]; then
    echo "error: ${file} does not exist" >&2
    failed=1
    return
  fi
  result="$("$@" "$file" "${work}/$(echo "$file" | tr '/' '_')")"
  IFS='|' read -r count wrong names <<<"$result"
  if [[ -z "$required" && "$count" -ne 1 ]]; then
    echo "error: ${file} has ${count} ${what} lines, not exactly one" >&2
    failed=1
  elif [[ -n "$wrong" ]]; then
    echo "error: ${file} holds${wrong} as ${what}, not the placeholder ${placeholder}" >&2
    failed=1
  fi
  for name in $required; do
    if [[ " ${names} " != *" ${name} "* ]]; then
      echo "error: ${file} has no ${what} line for ${name}" >&2
      failed=1
    fi
  done
}

check "${root}/cli/Cargo.toml" "[package] version" "" manifest_version_package
check "${root}/cli/Cargo.lock" "path package version" "marshal-cli" lockfile_version
check "${root}/app/Cargo.toml" "[workspace.package] version" "" manifest_version_workspace
check "${root}/app/Cargo.lock" "path package version" "marshal-app marshal-cli" lockfile_version
if [[ $failed -ne 0 ]]; then
  exit 1
fi

# cat over the original, not mv, so the files keep their mode and owner.
for file in cli/Cargo.toml cli/Cargo.lock app/Cargo.toml app/Cargo.lock; do
  cat "${work}/$(echo "${root}/${file}" | tr '/' '_')" >"${root}/${file}"
done
echo "Set the CLI's and the app's version to ${version} in cli/ and app/ under ${root}."
