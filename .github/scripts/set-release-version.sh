#!/usr/bin/env bash
# writes a release version over the placeholder in the CLI's Cargo.toml and
# Cargo.lock, in the runner only; refuses unless it is X.Y.Z or X.Y.Z-<prerelease>
# and each file has exactly one placeholder line. uses a temporary file, not
# `sed -i`, for the BSD tools of macOS and CRLF checkouts. see
# docs/operations/cli.md.
set -euo pipefail

placeholder="0.0.0-dev"

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: set-release-version.sh <version> [<cli directory>]" >&2
  exit 2
fi
version="$1"
cli="${2:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../cli" && pwd)}"
manifest="${cli}/Cargo.toml"
lockfile="${cli}/Cargo.lock"

number='(0|[1-9][0-9]*)'
release_version="^${number}\.${number}\.${number}(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$"
if [[ ! "$version" =~ $release_version ]]; then
  echo "error: ${version} is not X.Y.Z or X.Y.Z-<prerelease>, with no leading zero and no build metadata" >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/set-release-version.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT

# each function copies its file to <copy> with the version written over the
# line it selects, and prints how many lines it selected and the first value,
# which check compares with the placeholder. a trailing CR is set aside before
# matching and written back after.

# manifest_version <file> <copy>: the version line in the [package] table.
manifest_version() {
  awk -v version="$version" -v out="$2" '
    {
      line = $0
      cr = sub(/\r$/, "", line) ? "\r" : ""
    }
    line ~ /^[[:space:]]*\[/ { in_package = (line ~ /^[[:space:]]*\[package\][[:space:]]*$/) }
    in_package && line ~ /^[[:space:]]*version[[:space:]]*=/ && match(line, /"[^"]*"/) {
      found++
      value = substr(line, RSTART + 1, RLENGTH - 2)
      if (found == 1) first = value
      sub(/"[^"]*"/, "\"" version "\"", line)
    }
    { printf "%s%s\n", line, cr > out }
    END { print found + 0, first }
  ' "$1"
}

# lockfile_version <file> <copy>: the version line of the marshal-cli entry.
lockfile_version() {
  awk -v version="$version" -v out="$2" '
    {
      line = $0
      cr = sub(/\r$/, "", line) ? "\r" : ""
    }
    line ~ /^\[/ { in_package = (line == "[[package]]"); target = 0 }
    in_package && line == "name = \"marshal-cli\"" { target = 1 }
    in_package && target && line ~ /^version = "[^"]*"$/ {
      found++
      target = 0
      value = substr(line, 12, length(line) - 12)
      if (found == 1) first = value
      line = "version = \"" version "\""
    }
    { printf "%s%s\n", line, cr > out }
    END { print found + 0, first }
  ' "$1"
}

failed=0
# check <file> <function that selects its version line> <what that line is>
check() {
  local file="$1" select="$2" what="$3" result count value
  if [[ ! -f "$file" ]]; then
    echo "error: ${file} does not exist" >&2
    failed=1
    return
  fi
  result="$("$select" "$file" "${work}/$(basename "$file")")"
  count="${result%% *}"
  value="${result#* }"
  if [[ "$count" -ne 1 ]]; then
    echo "error: ${file} has ${count} ${what} lines, not exactly one" >&2
    failed=1
  elif [[ "$value" != "$placeholder" ]]; then
    echo "error: ${file} holds ${value} as ${what}, not the placeholder ${placeholder}" >&2
    failed=1
  fi
}

check "$manifest" manifest_version "[package] version"
check "$lockfile" lockfile_version "marshal-cli version"
if [[ $failed -ne 0 ]]; then
  exit 1
fi

# cat over the original, not mv, so the files keep their mode and owner.
cat "${work}/Cargo.toml" >"$manifest"
cat "${work}/Cargo.lock" >"$lockfile"
echo "Set the CLI's version to ${version} in ${manifest} and ${lockfile}."
