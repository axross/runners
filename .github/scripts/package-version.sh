#!/usr/bin/env bash
# Prints the version in the [package] table of the Cargo manifest on standard input,
# and fails when that table names none. The release tag check and the release build
# both read the CLI's version through this script, so a tag and the asset names it
# publishes cannot be derived from two readings that disagree.
#
# Usage: package-version.sh < cli/Cargo.toml
set -euo pipefail

version="$(awk '
  /^[[:space:]]*\[/ { in_package = ($0 ~ /^[[:space:]]*\[package\][[:space:]]*$/); next }
  in_package && /^[[:space:]]*version[[:space:]]*=/ && match($0, /"[^"]*"/) {
    print substr($0, RSTART + 1, RLENGTH - 2)
    exit
  }
')"
if [[ -z "$version" ]]; then
  echo "error: the Cargo manifest names no version in its [package] table" >&2
  exit 1
fi
printf '%s\n' "$version"
